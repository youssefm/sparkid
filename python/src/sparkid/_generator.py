import os
import threading
import time
import weakref
from datetime import datetime, timezone

from sparkid._constants import (
    ALPHABET,
    BASE,
    COUNTER_CHAR_COUNT,
    ID_LENGTH,
    MAX_TIMESTAMP,
    RANDOM_CHAR_COUNT,
    TIMESTAMP_CHAR_COUNT,
)

_COUNTER_HEAD_CHAR_COUNT = COUNTER_CHAR_COUNT - 1  # 5
_COUNTER_HEAD_LAST_INDEX = _COUNTER_HEAD_CHAR_COUNT - 1  # 4
_PREFIX_COUNTER_HEAD_LENGTH = TIMESTAMP_CHAR_COUNT + _COUNTER_HEAD_CHAR_COUNT  # 13

# How many random bytes to fetch per batch. After rejection sampling,
# ~90.6% survive (58/64), yielding ~232 valid chars.
_RANDOM_BATCH_SIZE = 256

# Timestamp encoding: remainder (0-57) -> single char
_TIMESTAMP_LOOKUP = [ALPHABET[i] for i in range(BASE)]

# Random encoding via bytes.translate() + delete for C-level rejection sampling.
# For each byte 0-255: mask to 6 bits (0-63). Values < 58 map to their alphabet
# byte; values >= 58 are deleted (rejection). No modulo bias.
_TRANSLATE_TABLE = bytes(
    ord(ALPHABET[b & 0x3F]) if (b & 0x3F) < BASE else 0 for b in range(256)
)
_DELETE_BYTES = bytes(b for b in range(256) if (b & 0x3F) >= BASE)

# Successor table size: covers all char codes up to last alphabet character.
_SUCCESSOR_TABLE_SIZE = ord(ALPHABET[-1]) + 1  # ord('z') + 1

# Successor table: char code -> next Base58 char (string), or "" if carry needed.
_SUCCESSOR_STR: list[str] = [""] * _SUCCESSOR_TABLE_SIZE
for _i in range(BASE - 1):
    _SUCCESSOR_STR[ord(ALPHABET[_i])] = ALPHABET[_i + 1]
# Last char ('z') stays "" (carry)

# Successor table: byte -> next Base58 byte, or -1 if carry needed.
# Used in carry propagation which still operates on the bytearray.
_SUCCESSOR = [-1] * _SUCCESSOR_TABLE_SIZE
for _i in range(BASE - 1):
    _SUCCESSOR[ord(ALPHABET[_i])] = ord(ALPHABET[_i + 1])

# Reverse lookup: char -> Base58 index (0-57). Used for timestamp decoding.
_BASE58_INDEX = {ch: i for i, ch in enumerate(ALPHABET)}

_FIRST_CHAR = ALPHABET[0]
_FIRST_BYTE = ord(ALPHABET[0])

_time_ns = time.time_ns
_urandom = os.urandom


# Track all live generators for fork-safety reset.
_all_generators: weakref.WeakSet["IdGenerator"] = weakref.WeakSet()


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

    __slots__ = (
        "__weakref__",
        "_timestamp_cache_ms",
        "_timestamp_cache_prefix",
        "_prefix_plus_counter_head",
        "_counter_tail",
        "_counter_head_buf",
        "_random_char_buffer",
        "_random_byte_buffer",
        "_random_byte_position",
        "_random_byte_len",
    )

    def __init__(self) -> None:
        self._reset_state()
        _all_generators.add(self)

    def _reset_state(self) -> None:
        """(Re)initialize all mutable state. Called on construction and after fork."""
        self._timestamp_cache_ms = 0
        self._timestamp_cache_prefix = _FIRST_CHAR * TIMESTAMP_CHAR_COUNT
        self._prefix_plus_counter_head = _FIRST_CHAR * _PREFIX_COUNTER_HEAD_LENGTH
        self._counter_tail = _FIRST_CHAR
        # Bytearray for carry propagation (counter head)
        self._counter_head_buf = bytearray([_FIRST_BYTE] * _COUNTER_HEAD_CHAR_COUNT)
        self._random_char_buffer = ""
        self._random_byte_buffer = b""
        self._random_byte_position = 0
        self._random_byte_len = 0

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

        # NOTE: The state-machine logic below is intentionally duplicated in
        # __call__. CPython doesn't inline method calls, so extracting a shared
        # helper adds ~30ns (~4%) overhead to the hot path. Keep both copies in
        # sync when modifying the generation algorithm.
        if timestamp_ms > self._timestamp_cache_ms:
            delta = timestamp_ms - self._timestamp_cache_ms
            if delta <= BASE:
                self._timestamp_cache_ms = timestamp_ms
                self._increment_encoded_timestamp(delta)
            else:
                self._encode_timestamp(timestamp_ms)
            self._seed_counter()
        else:
            nxt = _SUCCESSOR_STR[ord(self._counter_tail)]
            if nxt:
                self._counter_tail = nxt
            else:
                self._increment_counter_carry()

        position = self._random_byte_position
        end = position + RANDOM_CHAR_COUNT
        if end > self._random_byte_len:
            self._refill_random()
            position = 0
            end = RANDOM_CHAR_COUNT
        self._random_byte_position = end

        return (
            self._prefix_plus_counter_head
            + self._counter_tail
            + self._random_char_buffer[position:end]
        )

    def __call__(self) -> str:
        timestamp = _time_ns() // 1_000_000

        # NOTE: The state-machine logic below is intentionally duplicated in
        # generate_at. CPython doesn't inline method calls, so extracting a
        # shared helper adds ~30ns (~4%) overhead to this hot path. Keep both
        # copies in sync when modifying the generation algorithm.
        if timestamp > self._timestamp_cache_ms:
            # New millisecond (or first call): update timestamp, seed counter.
            delta = timestamp - self._timestamp_cache_ms
            if delta <= BASE:
                # Fast path: increment the encoded timestamp directly,
                # avoiding 8 divmod operations.
                self._timestamp_cache_ms = timestamp
                self._increment_encoded_timestamp(delta)
            else:
                # Large jump or first call: full re-encode.
                self._encode_timestamp(timestamp)
            self._seed_counter()
        else:
            # Same millisecond (or clock went backward): increment counter tail.
            nxt = _SUCCESSOR_STR[ord(self._counter_tail)]
            if nxt:
                self._counter_tail = nxt
            else:
                self._increment_counter_carry()

        # Refresh the random tail from the pre-sampled buffer.
        position = self._random_byte_position
        end = position + RANDOM_CHAR_COUNT
        if end > self._random_byte_len:
            self._refill_random()
            position = 0
            end = RANDOM_CHAR_COUNT
        self._random_byte_position = end

        return (
            self._prefix_plus_counter_head
            + self._counter_tail
            + self._random_char_buffer[position:end]
        )

    def _seed_counter(self) -> None:
        """Seed the counter from the random byte buffer."""
        position = self._random_byte_position
        end = position + COUNTER_CHAR_COUNT
        if end > self._random_byte_len:
            self._refill_random()
            position = 0
            end = COUNTER_CHAR_COUNT
        self._random_byte_position = end
        counter_bytes = self._random_byte_buffer[position:end]
        self._counter_head_buf[:] = counter_bytes[:_COUNTER_HEAD_CHAR_COUNT]
        self._prefix_plus_counter_head = (
            self._timestamp_cache_prefix
            + counter_bytes[:_COUNTER_HEAD_CHAR_COUNT].decode("ascii")
        )
        self._counter_tail = chr(counter_bytes[_COUNTER_HEAD_CHAR_COUNT])

    def _increment_counter_carry(self) -> None:
        """Handle carry propagation through counter head bytes.

        Called when the counter tail overflows. On full overflow
        (all 6 counter chars maxed), bump the timestamp forward by 1ms and
        reseed. Because the counter is randomly seeded each ms, overflow
        probability is n / 58^6 for n IDs generated in that ms.
        """
        buf = self._counter_head_buf
        successor = _SUCCESSOR
        for i in range(_COUNTER_HEAD_LAST_INDEX, -1, -1):
            nxt = successor[buf[i]]
            if nxt >= 0:
                buf[i] = nxt
                self._counter_tail = _FIRST_CHAR
                self._prefix_plus_counter_head = (
                    self._timestamp_cache_prefix + buf.decode("ascii")
                )
                return
            buf[i] = _FIRST_BYTE
        # Overflow: bump timestamp, reseed.
        self._encode_timestamp(self._timestamp_cache_ms + 1)
        self._seed_counter()

    def _encode_timestamp(self, timestamp: int) -> None:
        """Base58-encode millisecond timestamp into the prefix string."""
        if timestamp < 0 or timestamp > MAX_TIMESTAMP:
            raise ValueError(
                f"Timestamp out of range: {timestamp}"
                f" (valid range: 0 to {MAX_TIMESTAMP})"
            )
        self._timestamp_cache_ms = timestamp
        lookup = _TIMESTAMP_LOOKUP
        timestamp, r7 = divmod(timestamp, BASE)
        timestamp, r6 = divmod(timestamp, BASE)
        timestamp, r5 = divmod(timestamp, BASE)
        timestamp, r4 = divmod(timestamp, BASE)
        timestamp, r3 = divmod(timestamp, BASE)
        timestamp, r2 = divmod(timestamp, BASE)
        timestamp, r1 = divmod(timestamp, BASE)
        self._timestamp_cache_prefix = (
            lookup[timestamp]
            + lookup[r1]
            + lookup[r2]
            + lookup[r3]
            + lookup[r4]
            + lookup[r5]
            + lookup[r6]
            + lookup[r7]
        )

    def _increment_encoded_timestamp(self, delta: int) -> None:
        """Increment the cached timestamp prefix by delta (1..58).

        Decodes the last timestamp character to a Base58 index, adds delta,
        and replaces it. If the result carries (>= 58), propagates carry
        backward through the prefix using the successor table.
        """
        timestamp_prefix = self._timestamp_cache_prefix
        new_index = _BASE58_INDEX[timestamp_prefix[7]] + delta
        if new_index < BASE:
            self._timestamp_cache_prefix = (
                timestamp_prefix[:7] + _TIMESTAMP_LOOKUP[new_index]
            )
            return
        # Carry: scan backward to find which digit absorbs carry.
        successor = _SUCCESSOR_STR
        carry_position = -1
        for i in range(6, -1, -1):
            if successor[ord(timestamp_prefix[i])]:
                carry_position = i
                break
        if carry_position < 0:
            raise ValueError(
                f"Timestamp out of range: {self._timestamp_cache_ms}"
                f" (valid range: 0 to {MAX_TIMESTAMP})"
            )
        # Build result once: unchanged + successor + wrapped zeros + remainder.
        self._timestamp_cache_prefix = (
            timestamp_prefix[:carry_position]
            + successor[ord(timestamp_prefix[carry_position])]
            + _FIRST_CHAR * (6 - carry_position)
            + _TIMESTAMP_LOOKUP[new_index - BASE]
        )

    def _refill_random(self) -> None:
        """Fetch a batch of random bytes, rejection-sample to Base58 chars."""
        buf = _urandom(_RANDOM_BATCH_SIZE).translate(_TRANSLATE_TABLE, _DELETE_BYTES)
        self._random_byte_buffer = buf
        self._random_char_buffer = buf.decode("ascii")
        self._random_byte_len = len(buf)
        self._random_byte_position = 0


# Thread-safe default: each thread gets its own IdGenerator automatically.
class _Local(threading.local):
    def __init__(self) -> None:
        self.gen = IdGenerator()


_local = _Local()


def _after_fork_in_child() -> None:
    """Reset all generator state after fork to prevent duplicate IDs.

    After os.fork(), the child inherits the parent's random buffers and counter
    state. Without this reset, both processes would generate identical IDs until
    their random buffers were independently refilled.
    """
    # Reset every live IdGenerator — covers both the thread-local instance
    # inside _local and any user-created instances.
    for gen in _all_generators:
        gen._reset_state()


os.register_at_fork(after_in_child=_after_fork_in_child)


def generate_id() -> str:
    """Generate a unique, time-sortable, 21-char Base58 ID.

    Thread-safe via threading.local. IDs are strictly monotonically increasing
    within each thread; across threads they are unique but unordered.
    """
    return _local.gen()


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
    return _local.gen.generate_at(timestamp)


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
    if not isinstance(id, str):
        raise ValueError("extract_timestamp: expected a string argument")
    if len(id) != ID_LENGTH:
        raise ValueError(
            f"extract_timestamp: expected a {ID_LENGTH}-character string,"
            f" got {len(id)}"
        )
    index = _BASE58_INDEX
    for i, ch in enumerate(id):
        if ch not in index:
            raise ValueError(
                f"extract_timestamp: invalid Base58 character {ch!r} at position {i}"
            )
    ms = 0
    for ch in id[:TIMESTAMP_CHAR_COUNT]:
        ms = ms * BASE + index[ch]
    return datetime.fromtimestamp(ms / 1000, tz=timezone.utc)
