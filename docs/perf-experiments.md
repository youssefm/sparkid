# Performance Experiments Log

A record of optimization experiments — primarily the ones that **did not work** —
so they are not blindly re-attempted. Each entry states what was tried, the
measured result, and *why* it failed (the root cause is the part worth keeping).

For the experiments that were kept, see commit `cb2890c`
(`perf: LTO + inlining + lazy buffer alloc`).

## Methodology

Steady-state ID generation is ~15–33 ns; at that scale, run-to-run noise and
machine drift easily exceed the effect being measured. Lessons that shaped how
results below were judged:

- **Trust interleaved A/B, not sequential before/after.** Absolute timings
  drifted ~13% between runs on the same machine (e.g. `next_id` measured
  30.6 ns and 34.6 ns minutes apart with identical code) due to CPU frequency
  scaling, thermal state, and P-core/E-core scheduling (un-pinnable on macOS).
  A one-shot before/after cannot distinguish a real gain from the machine
  settling. Toggling the change on/off across several interleaved rounds can:
  if every "after" round beats its paired "before" round with no overlap
  between the two groups, the direction is real regardless of absolute drift.
- **A "win" must clear the noise floor.** Within-run variance was measured per
  language: Rust/Criterion ~0.2–2 ns, Python wall-clock ~4%, JS/V8 ~6%.
  Deltas smaller than that floor are noise, not signal.
- **Memory changes are judged on bytes, not throughput.** They show up as
  "neutral" on a throughput benchmark by design; kept or rejected on allocation
  size and code clarity instead.

## Rejected experiments

| Experiment | Lang | Measured result | Root cause |
| --- | --- | --- | --- |
| Merge raw + sampled random buffers | Rust | −2% to −8% | Aliasing blocks LLVM auto-vectorization |
| `RANDOM_BATCH_SIZE` 16384 → 8192 | Rust | Throughput-neutral, **broke a test** | Hardcoded yield-ratio assertion; marginal memory gain |
| Single `fromCharCode(21)` emit, prefix kept as string | JS | −30% | `charCodeAt` on a cons-string prefix is not O(1) |
| Full buffer rewrite (prefix as `Uint8Array`) | JS | −16% | Destroys per-millisecond prefix amortization |
| Merge raw + sampled random buffers | JS | Noise (neutral) | One-time 16 KB; adds aliasing subtlety for no speed |

### Rust — merge `raw_buffer` and `random_buffer`

**Idea.** `refill_random` reads raw RNG bytes from one 16 KB buffer and writes
rejection-sampled survivors into a second. Because the write index (`count`)
never exceeds the read index (`i`), compaction is safe in place — collapse to a
single buffer and halve per-generator memory (32 KB → 16 KB).

**Result.** Regressed every benchmark: `next_id` +2.1%, `SparkId::new` +1.9%,
webserver +7.6%.

**Why.** Two separate buffers let LLVM prove the read stream and write stream do
not alias, so it vectorizes the mask-and-compact loop. One buffer creates a
read-after-write hazard on the same memory, which blocks vectorization in the
`#[cold]` refill. The webserver path (most refills) paid the most. A 16 KB
saving is not worth a 2–8% throughput hit.

### Rust — shrink `RANDOM_BATCH_SIZE` 16384 → 8192

**Idea.** Halve the random batch buffer. 8192 bytes still yields ~1060 IDs per
refill; refill is `#[cold]` and amortized, so throughput should be flat while
memory halves.

**Result.** Throughput was flat as predicted, but `test_expected_yield` failed —
it asserts `random_count / 16384.0 ∈ [0.80, 0.98]` with the batch size
hardcoded. The behavior was correct (yield ratio still ~0.906); only the test's
constant was coupled to the old size.

**Why rejected.** The protocol rule is "tests must pass — don't force marginal
wins." Editing a test to land a memory-only change that the lazy-allocation work
(kept) already addresses for unused generators wasn't worth it. If revisited,
make `RANDOM_BATCH_SIZE` the single source of truth the test reads from, and
treat it as a memory-vs-refill-frequency knob (keep 16384 for low thread counts;
8192 only when per-thread memory dominates).

### JS — single `fromCharCode(21)` emit (two variants)

**Idea.** The hot path returns `prefixPlusCounterHead + fromCharCode(8 chars)`,
creating an 8-char temporary and a V8 cons-string per ID. Emit all 21 chars in
one `fromCharCode` instead.

An isolated micro-benchmark of just the emit step predicted **+31%**, which
motivated two real attempts:

1. **Lite** — keep the 13-char prefix as a string, read it via `charCodeAt`
   inside one `fromCharCode(21)`. **Regressed ~30%** (20.6M → 14.5M ids/sec).
2. **Full** — store the whole ID in a persistent 21-byte `Uint8Array`, rewrite
   only the changed region per ID, emit with one `fromCharCode` over the buffer.
   This required rewriting `encodeTimestamp`, `incrementEncodedTimestamp`,
   `seedCounter`, and `incrementCarry` to operate on the buffer. All 56 tests
   passed, but it **regressed ~16%** (0.0478 → 0.0555 µs/call).

**Why both failed — and why the micro lied.** The real `prefixPlusCounterHead`
is built by string concatenation, so it is a **cons-string (rope)**, not a flat
interned string. `charCodeAt` on a rope is far from O(1) (lite). More
fundamentally, the existing design **amortizes** the 13-char prefix: it is built
once per millisecond and reused unchanged across thousands of IDs, so the steady
state is only an 8-char build + one cheap concat per ID. Both rewrites discard
that amortization and rebuild all 21 chars every single ID (full). The isolated
micro-benchmark used a flat constant prefix and rebuilt nothing per millisecond,
so it modeled neither the rope cost nor the amortization advantage — it measured
the wrong thing and predicted the opposite of reality.

**Takeaway.** The current `prefix + fromCharCode(8)` design is already well
matched to V8. A micro-benchmark that does not replicate the caller's state
(here: rope prefix + per-ms reuse) can be confidently wrong.

### JS — merge raw and sampled random buffers

**Idea.** Same in-place compaction as the Rust attempt, applied to
`refillRandom`'s two module-global buffers.

**Result.** Neutral — the two A/B distributions overlapped (noise). Unlike Rust,
JIT'd JS does not auto-vectorize the compaction loop, so aliasing blocks nothing.

**Why rejected.** The only benefit is a one-time 16 KB saving (buffers are
process-global, allocated once), and it aliases two conceptually distinct
buffers (raw bytes vs. sampled codes) for no measurable speed. Not worth the
clarity cost.

## Off the table (not attempted)

- **Caching / coarsening the clock read.** `next_id` spends ~18 ns of its ~32 ns
  in `SystemTime::now()` / `Date.now()`. This dominates per-ID cost, but it
  cannot be cached or coarsened without changing observable timestamps and
  monotonicity guarantees — out of scope for an API- and behavior-preserving
  pass.

## Binary-size note

The kept Rust changes (`#[inline]` ×3, lazy alloc, crate `[profile.release]`
LTO) add **0 bytes** to a downstream consumer binary (verified with a throwaway
consumer crate, `cargo clean` rebuild, stripped, both no-LTO and LTO profiles).

- `#[inline]` on `next_id`/`new`/`next_id_at` did not grow the binary: at
  `opt-level=3` LLVM already inlined these trivial functions across the crate
  boundary via the rlib's MIR, with or without the attribute.
- The crate's `[profile.release]` is ignored for consumers — Cargo only applies
  `[profile.*]` from the root package.

A first measurement showed +320 bytes; that was incremental-compilation noise
(a partial rebuild). The `cargo clean` numbers (+0 bytes) are the trustworthy
ones — a reminder to force a clean rebuild before comparing artifact sizes.
