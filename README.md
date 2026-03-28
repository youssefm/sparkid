# sparkid

[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Test](https://github.com/youssefm/sparkid/actions/workflows/test.yml/badge.svg)](https://github.com/youssefm/sparkid/actions/workflows/test.yml)
[![npm](https://img.shields.io/npm/v/sparkid)](https://www.npmjs.com/package/sparkid)
[![PyPI](https://img.shields.io/pypi/v/sparkid)](https://pypi.org/project/sparkid/)
[![crates.io](https://img.shields.io/crates/v/sparkid)](https://crates.io/crates/sparkid)

Fast, monotonic, time-sortable, 22-char Base58 unique ID generator. Zero dependencies.

```
1ocmpHE1bFnygEBAPTzMK4
1ocmpHE1bFnygFv4Wp4dL2
1ocmpHE1bFnygGoUXUL7Xo
```

Available for [JavaScript/TypeScript](#javascript), [Python](#python), and [Rust](#rust).

## Why sparkid?

| | sparkid | UUID v4 | UUID v7 | nanoid | ULID |
|---|---|---|---|---|---|
| **Length** | 22 | 36 | 36 | 21 | 26 |
| **Sortable** | Yes | No | Yes | No | Yes |
| **Monotonic** | Yes | No | No | No | No |
| **URL-safe** | Yes | No | No | Yes | Yes |
| **Alphabet** | Base58 | Hex | Hex | URL-safe | Crockford32 |

sparkid gives you compact, readable IDs that sort by creation time and are strictly monotonically increasing — no two IDs from the same generator will ever compare equal or out of order.

## How it works

Each ID is 22 characters built from three parts:

```
[8-char timestamp][6-char counter][8-char random]
```

- **Timestamp** — current millisecond, Base58-encoded. IDs from a later millisecond always sort after earlier ones.
- **Counter** — randomly seeded each new millisecond, then incremented for each ID within that millisecond. Guarantees strict monotonic ordering even at high throughput.
- **Random** — 8 characters of cryptographically secure randomness per ID, rejection-sampled to avoid modulo bias. Provides ~58^14 (~1.8 x 10^24) combinations per millisecond for collision resistance.

The Base58 alphabet (`123456789ABC...xyz`) excludes visually ambiguous characters (`0`, `O`, `I`, `l`), making IDs safe to copy, paste, and read aloud.

## JavaScript

```bash
npm install sparkid
```

```typescript
import { generateId } from "sparkid";

const id = generateId();
```

~22 million IDs/sec on Node.js. Works in Node >= 19, browsers, Deno, Bun, and Cloudflare Workers.

All IDs from `generateId()` are strictly monotonically increasing within the process. Since JavaScript is single-threaded, this means process-wide ordering with no coordination needed.

See [js/README.md](js/README.md) for full documentation.

## Python

```bash
pip install sparkid
```

```python
from sparkid import generate_id

id = generate_id()
```

~3 million IDs/sec on Python 3.14. Thread-safe via `threading.local` (one generator per thread). Fork-safe via `os.register_at_fork`.

IDs from a single thread are strictly monotonically increasing. Across threads, IDs are unique but unordered. For process-wide monotonic ordering, wrap an `IdGenerator` in a lock — see [python/README.md](python/README.md) for details.

## Rust

```bash
cargo add sparkid
```

```rust
use sparkid::SparkId;

let id = SparkId::new();
```

~27 million IDs/sec. `SparkId` is a stack-allocated, `Copy` type — no heap allocation. Thread-safe via `thread_local!` (one generator per thread). Only dependency is `rand` (userspace CSPRNG).

IDs from a single thread are strictly monotonically increasing. Across threads, IDs are unique but unordered. For manual control, use `IdGenerator` directly — see [rust/README.md](rust/README.md) for details.

## License

MIT
