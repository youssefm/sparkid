"""sparkid - Fast, time-sortable, 21-char Base58 unique ID generator."""

import os
import sys
import weakref

from sparkid._native import (
    IdGenerator as _RustIdGenerator,
    extract_timestamp_ms,
    from_bytes,
    generate_id,
    generate_id_at,
    reset_thread_local as _reset_thread_local,
    to_bytes,
)

# Track all live generators for fork-safety reset.
_all_generators: weakref.WeakSet["IdGenerator"] = weakref.WeakSet()

class IdGenerator(_RustIdGenerator):
    """Generates 21-char, Base58, time-sortable, collision-resistant unique IDs.

    Each ID is composed of three parts:
      - 8-char timestamp prefix  (milliseconds, Base58-encoded, sortable)
      - 6-char monotonic counter (randomly seeded each millisecond, incremented)
      - 7-char random tail       (independently random per ID, from a CSPRNG)

    IDs are strictly monotonically increasing within a single generator instance:
    across milliseconds by the timestamp prefix, and within the same millisecond
    by incrementing the counter (randomly seeded at the start of each new
    millisecond). On counter overflow the timestamp is bumped forward and the
    counter is reseeded. Because the counter is randomly seeded (not starting
    at zero), the probability of overflow when generating n IDs in one
    millisecond is n / 58^6 (≈ n / 38 billion).

    The random tail is freshly generated for every ID, making individual IDs
    unpredictable even when the counter value can be inferred.

    Properties:
      - 21 characters, fixed length
      - Lexicographically sortable by creation time
      - Monotonically increasing (within a single generator instance)
      - URL-safe, no ambiguous characters
      - ~58^13 (~8.4 x 10^22) total combinations per millisecond
      - Cryptographically secure randomness (CSPRNG seeded from OS entropy)

    Thread safety: each IdGenerator instance is designed for use by a single
    thread. The module-level generate_id() automatically uses one internal
    generator per OS thread. IDs are strictly monotonic within each generator;
    across threads they are unique but unordered. For process-wide monotonic
    ordering, protect a single shared instance with a lock.
    """

    # _all_generators needs weakrefs. CPython needs the "__weakref__" slot for
    # that; PyPy objects always support weakrefs and raises TypeError at import
    # if the slot is declared.
    __slots__ = () if sys.implementation.name == "pypy" else ("__weakref__",)

    def __init__(self) -> None:
        super().__init__()
        _all_generators.add(self)


def _after_fork_in_child() -> None:
    """Reset all generator state after fork to prevent duplicate IDs."""
    _reset_thread_local()
    for gen in _all_generators:
        gen.reset()


if hasattr(os, "register_at_fork"):
    os.register_at_fork(after_in_child=_after_fork_in_child)


__all__ = [
    "generate_id",
    "generate_id_at",
    "extract_timestamp_ms",
    "to_bytes",
    "from_bytes",
    "IdGenerator",
]
