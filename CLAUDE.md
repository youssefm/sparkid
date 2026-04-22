# sparkid

Monorepo with three independent packages (JS, Python, and Rust) implementing the same ID generator algorithm. No shared build system — each package is self-contained.

## Repo layout

```
js/          # TypeScript package (npm: sparkid)
  src/index.ts           # Single-file implementation
  tests/test_sparkid.ts  # Tests (node:test + node:assert)
  bench/benchmark.ts     # Benchmarks + correctness checks
python/      # Python package (PyPI: sparkid) — PyO3 native extension
  rust/lib.rs              # Rust/PyO3 native extension (hot path, built with maturin)
  src/sparkid/__init__.py  # Python subclass (fork-safety) + re-exports
  tests/test_sparkid.py    # Tests (pytest)
  bench/benchmark.py       # Benchmarks + correctness checks
rust/        # Rust crate (crates.io: sparkid)
  src/lib.rs             # Crate root, re-exports public API
  src/generator.rs       # Core implementation (no_std + alloc)
  tests/test_sparkid.rs  # Integration tests
  tests/no_std_check/    # Compile-time no_std verification crate
  benches/benchmark.rs   # Criterion benchmarks
```

## Commands

**JS** (`js/`):
- `npm test` — run tests
- `npm run build` — build with tsup (dual ESM/CJS + types)
- `npm run bench` — run benchmarks
- `npm run bench:compare` — compare against uuid, nanoid, ulid

**Python** (`python/`):
- `uv run pytest tests/` — run tests
- `uv run maturin develop --release` — rebuild native extension (required after changing `rust/lib.rs`; `uv sync` alone won't rebuild)
- `uv sync --group test` — install test deps (pytest)
- `uv sync` — set up venv and install sparkid
- `uv sync --all-groups` — also install benchmark deps
- `uv run python bench/benchmark.py` — run benchmarks
- `uv run python bench/benchmark.py --compare` — compare against other generators

**Rust** (`rust/`):
- `cargo test` — run tests
- `cargo build --release` — optimized build
- `cargo bench` — run Criterion benchmarks (includes comparison)
- `cargo clippy` — lint
- `cargo build -p no_std_check` — verify no_std compatibility

**Cross-language** (repo root):
- `python3 bench_compare.py` — run all three benchmarks and generate `benchmark_comparison.png` (requires `pip install matplotlib`)

## Tests

**JS**: `npm test` (uses `node:test` + `node:assert` — zero test deps)
**Python**: `uv run pytest tests/` (requires `uv sync --group test`)
**Rust**: `cargo test` (uses built-in test framework — zero test deps)

Tests cover: ID format, timestamp encoding, counter monotonicity, carry propagation, uniqueness, cross-ms sortability, clock regression, random tail uniformity/bias, rejection sampling, thread safety, fork safety, and public API.

## Algorithm (all implementations are identical)

IDs are 21-char fixed-length Base58 strings: `[8 timestamp][6 counter][7 random]`

- **Timestamp**: milliseconds since epoch, Base58-encoded big-endian
- **Counter**: randomly seeded each new millisecond, incremented per-ID within same ms
- **Random tail**: cryptographically random bytes, rejection-sampled to Base58

Base58 alphabet: `123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz`

## Invariants — do not break

1. **Monotonicity**: IDs from a single generator instance must be strictly increasing (lexicographic) within the same millisecond and across milliseconds
2. **No modulo bias**: Random bytes are rejection-sampled (mask to 6 bits, discard >=58) — never use modulo
3. **Counter overflow**: When all 6 counter chars are maxed, bump timestamp forward 1ms and reseed — never wrap
4. **Thread safety (Python)**: `generate_id()` uses `threading.local` for per-thread generators; `IdGenerator` instances are NOT thread-safe
5. **Thread safety (Rust)**: `SparkId::new()` uses `thread_local!` for per-thread generators; `IdGenerator` instances require `&mut self` (enforced by borrow checker)
6. **Fork safety (Python)**: `os.register_at_fork` resets all live generators in child processes
7. **Zero runtime dependencies**: JS and Python must remain dependency-free; Rust uses only `rand` (userspace CSPRNG)
8. **no_std (Rust)**: Crate is `#![no_std]` + `alloc`. `std` feature (default) adds `SparkId::new()`, `next_id()`, `Iterator`. Without `std`, use `next_id_at(timestamp_ms)` instead.

## Engineering guidelines

- **No magic constants**: Extract numeric/string literals into named constants. Exceptions: 0, 1, empty string.
- **No abbreviations**: Use full words in identifiers (e.g., `timestamp` not `ts`, `position` not `pos`, `counter` not `ctr`). Common units are fine: `ms`, `KB`, `ns`.
- **`#[inline]` on Rust public accessors**: All trivial or delegating public methods on `SparkId`/`SparkIdStr` must have `#[inline]` to ensure cross-crate callers get inlining. Without it, even a one-liner pays full function call overhead from external crates.

## Git workflow

- **Never commit or push without explicit user approval.** Wait for the user to ask before committing, pushing, or creating PRs.
