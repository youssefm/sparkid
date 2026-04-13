---
name: benchmark
description: Run sparkid benchmarks. Use when the user asks to benchmark, profile, or measure performance of any sparkid package. Triggers on 'benchmark', 'bench', 'perf', 'performance', 'how fast'.
---

# Benchmark Skill

Run benchmarks for sparkid packages. **Never run benchmarks concurrently** — always run one at a time, sequentially. Concurrent benchmarks compete for CPU and produce unreliable results.

When the user asks to benchmark, determine which specific benchmark they need and run **only** that one. Do not run all benchmarks unless explicitly asked.

## Available Benchmarks

### JavaScript (`js/`)

All commands run from the `js/` directory.

| Command | What it measures |
|---|---|
| `npm run bench` | sparkid `generateId()` throughput (1M iterations, 5 trials) + binary `toBytes`/`fromBytes` |
| `npm run bench:compare` | sparkid vs uuid v4, uuid v7, nanoid, ulid (100K iterations) |
| `npm run bench:binary` | Binary encoding/decoding only (`toBytes`/`fromBytes`) |

### Python (`python/`)

All commands run from the `python/` directory. Requires Python 3.14+ for `--compare` (uses `uuid.uuid7`).

| Command | What it measures |
|---|---|
| `uv run python bench/benchmark.py` | sparkid `generate_id()` throughput (1M iterations, 5 trials) |
| `uv run python bench/benchmark.py --compare` | sparkid vs uuid4, uuid7, nanoid, ulid, ksuid |
| `uv run python bench/benchmark.py --binary` | Binary encoding/decoding only (`to_bytes`/`from_bytes`) |

Ensure deps are installed first: `uv sync --all-groups`

### Rust (`rust/`)

All commands run from the `rust/` directory. Uses Criterion for statistical benchmarking.

| Command | What it measures |
|---|---|
| `cargo bench` | All Rust benchmarks (generator, thread-local, parse, comparison) |
| `cargo bench -- "IdGenerator::next_id"` | `IdGenerator::next_id` only (direct generator) |
| `cargo bench -- "SparkId::new"` | `SparkId::new` only (thread-local path) |
| `cargo bench -- "from_str"` | `SparkId::from_str` parsing only |
| `cargo bench -- "id_generators"` | Comparison group only (sparkid vs uuid v4/v7, nanoid, ulid) |

### Cross-language comparison (`repo root`)

| Command | What it measures |
|---|---|
| `python bench_compare.py` | Runs all three language benchmarks and produces a comparison chart |

## Decision Guide

- **"How fast is sparkid?"** → Run the core benchmark for the relevant language (`npm run bench`, `uv run python bench/benchmark.py`, or `cargo bench -- "IdGenerator::next_id"`)
- **"How does it compare to X?"** → Run the comparison benchmark for the relevant language
- **"Is binary encoding fast?"** → JS: `npm run bench:binary`, Python: `--binary` flag, Rust: not separately benchmarked (part of parse bench)
- **"Benchmark everything"** → Run each language's full benchmark sequentially, one after another
- **"I changed the generator"** → Run only the core benchmark for the language that was changed
