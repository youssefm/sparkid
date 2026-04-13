# ⚡ sparkid

Fast, monotonic, time-sortable, 21-char Base58 unique ID generator. Only dependency is `rand`.

```
1ocmpHE1bFnygEBAPTzMK
1ocmpHE1bFnygFv4Wp4dL
1ocmpHE1bFnygGoUXUL7X
```

## Install

```bash
cargo add sparkid
```

## Usage

```rust
use sparkid::SparkId;

let id = SparkId::new();
// => "1ocmpHE1bFnygEBAPTzMK"

println!("{id}");              // Display, no heap allocation
let s = id.as_str();           // SparkIdStr — stack-allocated, Deref<str>
let owned: String = id.into(); // Into<String> when needed
```

## Properties

| Property | Value |
|---|---|
| **Length** | 21 characters, fixed |
| **Alphabet** | Base58 (no `0`, `O`, `I`, `l`) |
| **Sortable** | Lexicographically, by creation time |
| **Monotonic** | Strictly increasing within each thread |
| **URL-safe** | Yes |
| **Collision resistance** | ~58^13 (~8.4 x 10^22) combinations per millisecond |
| **Randomness** | Cryptographically secure (`rand` / ChaCha12, seeded from OS) |
| **Thread-safe** | Yes (via `thread_local!`) |

## How it works

Each ID is composed of two parts:

```
[8-char timestamp][13-char suffix]
```

- **Timestamp** (8 chars): Current time in milliseconds, Base58-encoded. IDs generated in a later millisecond always sort after earlier ones.
- **Suffix** (13 chars): Seeded from a cryptographically secure PRNG (rejection-sampled, no modulo bias) at the start of each millisecond, then monotonically incremented for each subsequent ID within that millisecond. This guarantees strict ordering even when multiple IDs share a timestamp.

## `SparkId` type

`SparkId` is a stack-allocated, `Copy` type backed by a `u128` — no heap allocation on creation. The 21 Base58 characters are bit-packed into 128 bits (6 bits per character), preserving sort order. It implements `Display`, `Ord`, `Hash`, `FromStr`, and `Into<String>`.

Use `as_str()` to get a `SparkIdStr` — a stack-allocated, `Copy` wrapper that dereferences to `&str`:

```rust
use sparkid::SparkId;

let a = SparkId::new();
let b = SparkId::new();
assert!(b > a);                      // Ord — monotonically increasing
println!("{a}");                      // Display — no allocation
let s = a.as_str();                  // SparkIdStr — Deref<str>, no heap
let slice: &str = &s;               // zero-cost &str access
let set: std::collections::HashSet<SparkId> = [a, b].into(); // Hash
```

### Parse from string

```rust
use sparkid::SparkId;

let id = SparkId::new();
let parsed: SparkId = id.to_string().parse().unwrap();
assert_eq!(id, parsed);
```

Parsing validates that the input is exactly 21 characters and all characters are in the Base58 alphabet. Returns a `ParseSparkIdError` on failure.

### Binary representation

`SparkId` is backed by a `u128` (16 bytes) — smaller and faster to compare/sort than the 21-char string form. You can serialize the binary representation directly:

```rust
use sparkid::SparkId;

let id = SparkId::new();

// 16-byte big-endian binary — sort-preserving, memcmp-comparable
let bytes: [u8; 16] = id.to_bytes();
let restored = SparkId::from_bytes(bytes).unwrap();
assert_eq!(id, restored);

// Raw u128 — branchless comparison, ideal for in-memory sorting
let raw: u128 = id.as_u128();
let also_restored = SparkId::from_u128(raw).unwrap();
assert_eq!(id, also_restored);
```

### Extract timestamp

```rust
use sparkid::SparkId;

let id = SparkId::new();

// As milliseconds since epoch (available in no_std)
let ms = id.timestamp_ms();

// As SystemTime (requires std)
let ts = id.timestamp();
```

### Serde

Enable the `serde` feature for `Serialize` and `Deserialize` on both `SparkId` and `SparkIdStr`:

```bash
cargo add sparkid --features serde
```

Human-readable formats (JSON, TOML, etc.) serialize as the 21-char Base58 string. Binary formats (postcard, bincode, etc.) serialize as the 16-byte packed representation.

```rust
use sparkid::SparkId;

#[derive(serde::Serialize, serde::Deserialize)]
struct Record {
    id: SparkId,
    name: String,
}
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

```bash
cargo bench
```

## License

MIT
