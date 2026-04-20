"""sparkid - Fast, time-sortable, 21-char Base58 unique ID generator."""

import os
import weakref
from datetime import datetime, timezone

from sparkid._sparkid import (  # isort: skip
    IdGenerator as _RustIdGenerator,
    MAX_TIMESTAMP as MAX_TIMESTAMP,  # noqa: F401
    extract_timestamp_ms as _extract_timestamp_ms,
    from_bytes,
    generate_id,
    generate_id_at as _generate_id_at_ms,
    reset_thread_local as _reset_thread_local,
    to_bytes,
)

# Track all live generators for fork-safety reset.
_all_generators: weakref.WeakSet["IdGenerator"] = weakref.WeakSet()

# Re-export constants used by tests
ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
BASE = 58


class IdGenerator:
    """Generates 21-char, Base58, time-sortable, collision-resistant unique IDs.

    Each ID is composed of three parts:
      - 8-char timestamp prefix  (milliseconds, Base58-encoded, sortable)
      - 6-char monotonic counter (randomly seeded each millisecond, incremented)
      - 7-char random tail       (independently random per ID, from os.urandom)

    IDs are strictly monotonically increasing within a single generator instance:
    across milliseconds by the timestamp prefix, and within the same millisecond
    by incrementing the counter (seeded from os.urandom at the start of each new
    millisecond). On counter overflow the timestamp is bumped forward and the
    counter is reseeded. Because the counter is randomly seeded (not starting
    at zero), the probability of overflow when generating n IDs in one
    millisecond is n / 58^6 (≈ n / 38 billion).

    The random tail is freshly generated for every ID, making individual IDs
    unpredictable even when the counter value can be inferred.

    On the hot path (same millisecond, no carry), the counter tail is a single
    string char bumped via a successor lookup, and the random tail is decoded
    from the pre-sampled byte buffer. The return is a 3-part string concat:
    prefix_plus_counter_head (13 chars) + counter_tail (1 char) + random (7 chars).

    Properties:
      - 21 characters, fixed length
      - Lexicographically sortable by creation time
      - Monotonically increasing (within a single generator instance)
      - URL-safe, no ambiguous characters
      - ~58^13 (~8.4 x 10^22) total combinations per millisecond
      - Cryptographically secure randomness (os.urandom)

    Thread safety: each IdGenerator instance is designed for use by a single
    thread. The module-level generate_id() uses threading.local to automatically
    create one instance per thread. IDs are strictly monotonic within each
    instance; across threads they are unique but unordered. For process-wide
    monotonic ordering, protect a single shared instance with a lock.
    """

    __slots__ = ("__weakref__", "_inner")

    def __init__(self) -> None:
        self._inner = _RustIdGenerator()
        _all_generators.add(self)

    def generate_at(self, timestamp: "datetime | int") -> str:
        """Generate an ID using a caller-supplied timestamp.

        Advances the generator's internal state using the given timestamp. If
        the supplied timestamp is earlier than the last-seen timestamp, the
        generator treats it as a clock regression — it preserves monotonicity
        by incrementing the counter instead of encoding the earlier timestamp.

        Args:
            timestamp: Either a timezone-aware ``datetime`` or an ``int``
                representing epoch milliseconds.

        Returns:
            A 21-character Base58 sparkid string.

        Raises:
            TypeError: If *timestamp* is not a ``datetime`` or ``int``, or is
                a ``bool``.
            ValueError: If a ``datetime`` is naive (no tzinfo).
        """
        if isinstance(timestamp, bool):
            raise TypeError(
                "timestamp must be a datetime or int, got bool"
            )
        if isinstance(timestamp, datetime):
            if timestamp.tzinfo is None:
                raise ValueError("timestamp must be timezone-aware")
            timestamp_ms = int(timestamp.timestamp() * 1000)
        elif isinstance(timestamp, int):
            timestamp_ms = timestamp
        else:
            raise TypeError(
                f"timestamp must be a datetime or int,"
                f" got {type(timestamp).__name__}"
            )
        return self._inner.generate_at(timestamp_ms)

    def __call__(self) -> str:
        return self._inner.generate()


def generate_id_at(timestamp: "datetime | int") -> str:
    """Generate a unique, time-sortable, 21-char Base58 ID at a given timestamp.

    Thread-safe via threading.local. IDs are strictly monotonically increasing
    within each thread; across threads they are unique but unordered.

    Args:
        timestamp: Either a timezone-aware ``datetime`` or an ``int``
            representing epoch milliseconds.

    Returns:
        A 21-character Base58 sparkid string.

    Raises:
        TypeError: If *timestamp* is not a ``datetime`` or ``int``, or is a
            ``bool``.
        ValueError: If a ``datetime`` is naive (no tzinfo), or the resulting
            epoch milliseconds is negative or exceeds MAX_TIMESTAMP.
    """
    if isinstance(timestamp, bool):
        raise TypeError(
            "timestamp must be a datetime or int, got bool"
        )
    if isinstance(timestamp, datetime):
        if timestamp.tzinfo is None:
            raise ValueError("timestamp must be timezone-aware")
        timestamp_ms = int(timestamp.timestamp() * 1000)
    elif isinstance(timestamp, int):
        timestamp_ms = timestamp
    else:
        raise TypeError(
            f"timestamp must be a datetime or int,"
            f" got {type(timestamp).__name__}"
        )
    return _generate_id_at_ms(timestamp_ms)


def extract_timestamp(id: str) -> datetime:
    """Extract the embedded timestamp from a sparkid as a UTC datetime.

    Decodes the first 8 Base58 characters back to the original millisecond
    timestamp and returns it as a timezone-aware ``datetime`` (UTC).

    Args:
        id: A 21-character Base58 sparkid string.

    Returns:
        A ``datetime`` corresponding to the embedded timestamp.

    Raises:
        ValueError: If *id* is not a valid 21-char Base58 string.
    """
    ms = _extract_timestamp_ms(id)
    return datetime.fromtimestamp(ms / 1000, tz=timezone.utc)


def _after_fork_in_child() -> None:
    """Reset all generator state after fork to prevent duplicate IDs."""
    _reset_thread_local()
    for gen in _all_generators:
        gen._inner.reset()


if hasattr(os, "register_at_fork"):
    os.register_at_fork(after_in_child=_after_fork_in_child)


__all__ = [
    "generate_id",
    "generate_id_at",
    "extract_timestamp",
    "to_bytes",
    "from_bytes",
    "IdGenerator",
]
