# ⚡ sparkid

Fast, monotonic, time-sortable, 21-char Base58 unique ID generator. Zero dependencies.

```
1ocmpHE1bFnygEBAPTzMK
1ocmpHE1bFnygFv4Wp4dL
1ocmpHE1bFnygGoUXUL7X
```

## Install

```bash
pip install sparkid
```

## Usage

```python
from sparkid import generate_id

id = generate_id()
# => "1ocmpHE1bFnygEBAPTzMK"
```

### Extract timestamp

```python
from sparkid import generate_id, extract_timestamp_ms

id = generate_id()
ms = extract_timestamp_ms(id)
print(ms)
# => 1731622400000
```

### Binary representation

Pack IDs into 16 bytes for compact storage. Sort order is preserved — byte-wise comparison on the binary form gives the same ordering as string comparison.

```python
from sparkid import generate_id, to_bytes, from_bytes

id = generate_id()

data = to_bytes(id)         # bytes, length 16
restored = from_bytes(data)
assert id == restored
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
| **Randomness** | Cryptographically secure (CSPRNG seeded from OS entropy) |
| **Thread-safe** | Yes (one generator per OS thread) |

## How it works

Each ID is composed of three parts:

```
[8-char timestamp][6-char counter][7-char random]
```

- **Timestamp** (8 chars): Current time in milliseconds, Base58-encoded. IDs generated in a later millisecond always sort after earlier ones.
- **Counter** (6 chars): Randomly seeded at the start of each millisecond, then incremented for each subsequent ID within that millisecond. This guarantees strict ordering even when multiple IDs share a timestamp.
- **Random** (7 chars): Freshly generated for every ID from a CSPRNG seeded from OS entropy, and rejection-sampled to avoid modulo bias. Within a millisecond, the counter alone would make each ID predictable from the previous one; the random tail prevents that.

## Ordering guarantees

IDs from a single `IdGenerator` instance (or a single thread using `generate_id()`) are **strictly monotonically increasing** — every ID is lexicographically greater than the one before it.

Across threads, IDs are **unique but not ordered** relative to each other. Each OS thread gets its own generator, so there is no cross-thread coordination. This is the same guarantee provided by most UUID v7 libraries.

If you need process-wide monotonic ordering across threads, wrap a single `IdGenerator` in a lock:

```python
import threading
from sparkid import IdGenerator

_lock = threading.Lock()
_generator = IdGenerator()

def generate_id_monotonic():
    with _lock:
        return _generator()
```

## Advanced usage

For manual control, use the `IdGenerator` class directly:

```python
from sparkid import IdGenerator

generator = IdGenerator()
id = generator()
```

Each `IdGenerator` instance maintains its own internal state. The module-level `generate_id()` function automatically uses one internal generator per OS thread.

## License

MIT
