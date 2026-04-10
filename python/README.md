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
from sparkid import extract_timestamp

id = generate_id()
dt = extract_timestamp(id)
print(dt.isoformat())
# => "2025-11-14T22:13:20+00:00"
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
| **Randomness** | Cryptographically secure (`os.urandom`) |
| **Thread-safe** | Yes (via `threading.local`) |

## How it works

Each ID is composed of two parts:

```
[8-char timestamp][13-char suffix]
```

- **Timestamp** (8 chars): Current time in milliseconds, Base58-encoded. IDs generated in a later millisecond always sort after earlier ones.
- **Suffix** (13 chars): Seeded from `os.urandom` (rejection-sampled, no modulo bias) at the start of each millisecond, then monotonically incremented for each subsequent ID within that millisecond. This guarantees strict ordering even when multiple IDs share a timestamp.

## Ordering guarantees

IDs from a single `IdGenerator` instance (or a single thread using `generate_id()`) are **strictly monotonically increasing** — every ID is lexicographically greater than the one before it.

Across threads, IDs are **unique but not ordered** relative to each other. Each thread gets its own generator via `threading.local`, so there is no cross-thread coordination. This is the same guarantee provided by most UUID v7 libraries.

If you need process-wide monotonic ordering across threads, wrap a single `IdGenerator` in a lock:

```python
import threading
from sparkid import IdGenerator

_lock = threading.Lock()
_gen = IdGenerator()

def generate_id_monotonic():
    with _lock:
        return _gen()
```

## Advanced usage

For manual control, use the `IdGenerator` class directly:

```python
from sparkid import IdGenerator

gen = IdGenerator()
id = gen()
```

Each `IdGenerator` instance maintains its own internal state. The module-level `generate_id()` function uses `threading.local` to automatically create one instance per thread.

## Performance

```bash
python bench/benchmark.py
```

## License

MIT
