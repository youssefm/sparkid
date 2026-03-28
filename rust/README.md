# sparkid

Fast, monotonic, time-sortable, 22-char Base58 unique ID generator. Only dependency is `rand`.

```
1ocmpHE1bFnygEBAPTzMK4
1ocmpHE1bFnygFv4Wp4dL2
1ocmpHE1bFnygGoUXUL7Xo
```

## Install

```bash
cargo add sparkid
```

## Usage

```rust
use sparkid::SparkId;

let id = SparkId::new();
// => "1ocmpHE1bFnygEBAPTzMK4"

println!("{id}");              // Display, no heap allocation
let s: &str = &id;             // Deref to &str, zero-cost
let owned: String = id.into(); // Into<String> when needed
```

## Properties

| Property | Value |
|---|---|
| **Length** | 22 characters, fixed |
| **Alphabet** | Base58 (no `0`, `O`, `I`, `l`) |
| **Sortable** | Lexicographically, by creation time |
| **Monotonic** | Strictly increasing within each thread |
| **URL-safe** | Yes |
| **Collision resistance** | ~58^14 (~1.8 x 10^24) combinations per millisecond |
| **Randomness** | Cryptographically secure (`rand` / ChaCha12, seeded from OS) |
| **Thread-safe** | Yes (via `thread_local!`) |

## How it works

Each ID is composed of two parts:

```
[8-char timestamp][14-char suffix]
```

- **Timestamp** (8 chars): Current time in milliseconds, Base58-encoded. IDs generated in a later millisecond always sort after earlier ones.
- **Suffix** (14 chars): Seeded from a cryptographically secure PRNG (rejection-sampled, no modulo bias) at the start of each millisecond, then monotonically incremented for each subsequent ID within that millisecond. This guarantees strict ordering even when multiple IDs share a timestamp.

## `SparkId` type

`SparkId` is a stack-allocated, `Copy` type — no heap allocation on creation. It implements `Deref<Target = str>`, `Display`, `Ord`, `Hash`, and `Into<String>`, so it works anywhere a string is expected.

```rust
use sparkid::SparkId;

let a = SparkId::new();
let b = SparkId::new();
assert!(b > a);                      // Ord — monotonically increasing
println!("{a}");                      // Display — no allocation
let set: std::collections::HashSet<SparkId> = [a, b].into(); // Hash
```

## Ordering guarantees

IDs from a single `IdGenerator` instance (or a single thread using `SparkId::new()`) are **strictly monotonically increasing** — every ID is lexicographically greater than the one before it.

Across threads, IDs are **unique but not ordered** relative to each other. Each thread gets its own generator via `thread_local!`, so there is no cross-thread coordination.

If you need process-wide monotonic ordering across threads, wrap a single `IdGenerator` in a `Mutex`:

```rust
use std::sync::{LazyLock, Mutex};
use sparkid::IdGenerator;

static GEN: LazyLock<Mutex<IdGenerator>> =
    LazyLock::new(|| Mutex::new(IdGenerator::new()));

fn generate_id_monotonic() -> sparkid::SparkId {
    GEN.lock().unwrap().next_id()
}
```

## Advanced usage

For manual control, use the `IdGenerator` struct directly:

```rust
use sparkid::IdGenerator;

let mut gen = IdGenerator::new();
let id = gen.next_id();
```

`IdGenerator` also implements `Iterator<Item = SparkId>`:

```rust
let mut gen = sparkid::IdGenerator::new();
let ids: Vec<sparkid::SparkId> = gen.take(100).collect();
```

## Performance

~27 million IDs/sec with `SparkId::new()` (~37 ns/call, zero-allocation):

```bash
cargo bench
```

## License

MIT
