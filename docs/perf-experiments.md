# Performance Experiments Log

A record of optimization experiments that **did not work**, so they are not
blindly re-attempted. Each entry states what was tried, the measured result, and
*why* it failed (the root cause is the part worth keeping).

## Methodology

Steady-state ID generation is tens of ns per ID (varies by language and
machine); at that scale, run-to-run noise and machine drift easily exceed the
effect being measured. Lessons that shaped how results below were judged:

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
- **Benchmark random-dependent code with fresh random data.** A
  micro-benchmark that reuses one fixed random buffer lets the branch predictor
  memorize data-dependent branches. In `refillRandom`'s compaction loop that
  hid most of a ~2.5x gap between `if` and branch-free compaction (loop only,
  excluding `getRandomValues`: fresh data 14.2 vs 5.8 µs per refill; fixed
  buffer ~6.4 vs ~5.8 µs).
- **Benchmark each variant in its own function and process.** A harness that
  calls every variant from one shared benchmark loop lets TurboFan inline them
  into that loop and compile them differently from standalone code. This made
  `+(bool)` look ~1.5x slower than it really is (see the ternary entry below).
- **Check machine load before and during a run.** Other processes (another
  session's browser or node workloads) inflated Criterion timings by 20–40%
  mid-run while load average sat at 8–11. Discard any round taken under load;
  `sysctl -n vm.loadavg` is a cheap gate.
- **Check the disassembly before attributing a result to codegen.** An earlier
  entry blamed lost auto-vectorization for a regression in a loop that was
  never vectorized (see the buffer-merge entry below).
- **Memory changes are judged on bytes, not throughput.** They show up as
  "neutral" on a throughput benchmark by design; kept or rejected on allocation
  size and code clarity instead.

## Rejected experiments

| Experiment | Lang | Measured result | Root cause |
| --- | --- | --- | --- |
| Merge raw + sampled random buffers | Rust | −2% to −8% | Unknown — the vectorization explanation was wrong (loop is scalar) |
| Prefix-sum compaction in `refill_random` | Rust | Refill 3–6% slower (one-shot) | Not diagnosed; shorter `count` chain did not pay off |
| Bind `random_buffer` to a local in `refill_random` | Rust | Neutral on refill (one-shot) | Removes per-store reloads, but they were not the bottleneck |
| `RANDOM_BATCH_SIZE` 16384 → 8192 | Rust | Throughput-neutral, **broke a test** | Hardcoded yield-ratio assertion; marginal memory gain |
| Single `fromCharCode(21)` emit, prefix kept as string | JS | −30% | `charCodeAt` on a cons-string prefix is not O(1) |
| Full buffer rewrite (prefix as `Uint8Array`) | JS | −16% | Destroys per-millisecond prefix amortization |
| Merge raw + sampled random buffers | JS | Noise (neutral) | One-time 16 KB; adds aliasing subtlety for no speed |
| Ternary as branch-free compaction advance | JS | ≈ `if` (16–18 vs 16–17 µs/refill) | V8 compiles `x !== 0 ? 1 : 0` back into a branch |
| Alias module-level random buffers to locals | JS | Noise (neutral) | `let` module buffers are not the refill bottleneck |

### Rust — merge `raw_buffer` and `random_buffer`

> **Predates the 3-bytes → 4-candidates split.** This entry describes the old
> one-byte-per-candidate refill (16 KB raw buffer). Its in-place premise no
> longer holds: chunk *k* reads bytes 3k..3k+2 but can write up to index 4k+3,
> so writes outrun reads and a naive in-place merge would corrupt unread input.

**Idea.** `refill_random` reads raw RNG bytes from one 16 KB buffer and writes
rejection-sampled survivors into a second. Because the write index (`count`)
never exceeds the read index (`i`), compaction is safe in place — collapse to a
single buffer and halve per-generator memory (32 KB → 16 KB).

**Result.** Regressed every benchmark: `next_id` +2.1%, `SparkId::new` +1.9%,
webserver +7.6%.

**Why — correction.** This entry originally said two separate buffers let LLVM
vectorize the mask-and-compact loop and one buffer blocked it. That is wrong.
Disassembly of the two-buffer build (arm64, rustc 1.98, `lto = true`,
`codegen-units = 1`) shows a scalar loop that handles one byte per
iteration: `ldrb` / `and #0x3f` / `strb` / `cmp #58` / `cinc`. It has no
data-dependent branch (only the bounds check and loop back-edge) and contains
no NEON instructions. Nothing else in `refill_random`
uses NEON either, including rand's inlined ChaCha12 core. A stream compaction
whose write index depends on the data is not something LLVM auto-vectorizes, so
the two-buffer version had no vectorization to lose. The regression's real
cause was not re-measured. Whatever it was, a 16 KB saving is not worth a 2–8%
throughput hit, so the rejection stands. If revisited, diff the disassembly of
both variants before explaining the result.

### Rust — prefix-sum compaction in `refill_random`

**How measured.** An unpublished Criterion microbench calling
`refill_random_test()` in a loop (via `test-internals`), plus `fill_bytes` of a
same-sized buffer alone to split RNG from compaction cost. Apple Silicon
(arm64), rustc 1.98. Each variant ran once, sequentially, not interleaved, so
small differences here are one-shot numbers, not A/B-confirmed. The same
applies to the next entry.

**Context.** `refill_random` packs 3 RNG bytes into 4 six-bit candidates. On
arm64, `StdRng::fill_bytes` of the 12,288 raw bytes takes ~7.8 µs and
compaction into ~14,850 survivors takes ~4.3 µs (~0.26 ns, about one cycle, per
candidate). That looks bound by the serial `count += valid` chain: each `cinc`
waits for the previous one.

**Idea.** Per 3-byte chunk, compute the in-chunk offsets
(`offset_1 = valid_0`, `offset_2 = offset_1 + valid_1`, ...) independently of
`count`. Then store all four candidates at `count + offset_k` and advance
`count` once per chunk, aiming for a loop-carried chain of one add per 4
candidates (the compiled code got it down to two).

**Result.** Slower: refill 12.78 µs (12.49 µs with the buffer bound to a
local) vs 12.05–12.10 µs for the plain per-candidate `cinc` loop.

**Why — not diagnosed.** Disassembly of the variant shows what the idea
intended: the same four bounds checks per chunk as the baseline, and the
`count` chain cut to two adds per chunk (offsets come from `cset`/`cinc` off
that chain). It still measured slower, so the `count` chain was evidently not
the binding constraint; the actual bottleneck was not identified. Since the
gap is a one-shot 3–6%, read this as "no gain", not as a confirmed
regression. A real gain would probably need explicit NEON compaction
(table-lookup compress), which is a lot of arch-specific `unsafe` code for at
most ~2 ns/ID.

### Rust — bind `random_buffer` to a local in `refill_random`

**Idea.** The compaction loop writes through `self.random_buffer[count]`.
Because a `u8` store may alias `self`, LLVM reloads the buffer pointer and
length before every store. Binding `let random_buffer = &mut *self.random_buffer`
before the loop lets it hoist both.

**Result.** Neutral: refill 12.10 µs vs 12.06 µs (one-shot, see the
prefix-sum entry for how this was measured).

**Why.** The reloads hit L1 and were not the bottleneck, so removing them
changes nothing measurable.

### Rust — shrink `RANDOM_BATCH_SIZE` 16384 → 8192

**Idea.** Halve the random batch buffer. 8192 candidates still yields ~1060 IDs per
refill; refill is `#[cold]` and amortized, so throughput should be flat while
memory halves.

**Result.** Throughput was flat as predicted, but `test_expected_yield` failed —
it asserts `random_count / 16384.0 ∈ [0.80, 0.98]` with the batch size
hardcoded. The behavior was correct (yield ratio still ~0.906); only the test's
constant was coupled to the old size.

**Why rejected.** The protocol rule is "tests must pass — don't force marginal
wins." Editing a test to land a memory-only change wasn't worth it. If revisited,
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

**Result.** Neutral — the two A/B distributions overlapped (noise). The
compaction loop is not vectorized in JS (and, despite what the Rust entry once
claimed, not in Rust either), so there was no vectorization for aliasing to
block.

**Why rejected.** The only benefit is a one-time 16 KB saving (buffers are
process-global, allocated once), and it aliases two conceptually distinct
buffers (raw bytes vs. sampled codes) for no measurable speed. Not worth the
clarity cost.

### JS — ternary as the branch-free compaction advance

**Idea.** `refillRandom`'s compaction always stores and advances the write
position only for accepted bytes. The obvious way to write the advance is
`count += charCode !== 0 ? 1 : 0`.

**Result.** No faster than the `if` it replaces. Each form run standalone in
its own process on fresh random data (Node 26.10 / Apple M5 Max; µs per 16 KB
refill, including ~1.7 µs of `getRandomValues`): `if` 15.8–17.2, ternary
16.1–18.3, `+(charCode !== 0)` 7.2–9.7, `(charCode + 255) >> 8` 7.5–9.3.
Bun (JavaScriptCore) showed the same pattern.

**Why.** TurboFan's optimized code for the ternary has a conditional branch
per byte (`cbnz`), so it still mispredicts on crypto-random rejections. Both
`+(bool)` and the shift compile to straight-line code (`cset` / `add … asr`).
The kept form is `+(charCode !== 0)`. It's as fast as the shift and needs no
bit trick.

**Measurement trap.** An earlier harness that called all variants from one
shared benchmark function had `+(bool)` at 13.8 µs, which looked like only a
partial fix. TurboFan had inlined it into that loop and compiled it
differently. Run standalone, and inside `refillRandom` itself, it is
branch-free.

### JS — alias random buffers to function locals

**Idea.** `refillRandom` reads two module-level `let` typed arrays. Copy them
into function-local `const`s before the loop, on the theory that V8 optimizes
const locals better.

**Result.** Neutral. In an interleaved A/B of `generateId()` (own child process
per variant, median of 5×2M IDs, 6 paired rounds) the aliased variant tracked
the baseline within noise (51.8–57.3 vs 52.3–55.8 ns/ID), while the branch-free
compaction in the same rounds ran 44.9–47.0 ns/ID. That branch-free result is
a positive control: it shows this setup can detect a real ~15% change. Copying
the buffers into locals on top of the branch-free version was also neutral.

**Why rejected.** The loop's cost was branch mispredicts, not module-variable
access. The aliases added lines and no speed.

## Off the table (not attempted)

- **Caching / coarsening the clock read.** `next_id` spends ~18 ns of its ~32 ns
  in `SystemTime::now()` / `Date.now()`. This dominates per-ID cost, but it
  cannot be cached or coarsened without changing observable timestamps and
  monotonicity guarantees — out of scope for an API- and behavior-preserving
  pass.
