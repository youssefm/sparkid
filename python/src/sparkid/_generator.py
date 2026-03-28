import os
import threading
import time
import weakref

# Base58 alphabet — excludes visually ambiguous characters (0, O, I, l)
ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
BASE = len(ALPHABET)  # 58

# ID structure: [8-char timestamp][6-char counter][8-char random] = 22 chars
_COUNTER_CHAR_COUNT = 6
_RANDOM_CHAR_COUNT = 8

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

# Successor table: char code -> next Base58 char (string), or "" if carry needed.
_SUCCESSOR_STR: list[str] = [""] * 123  # ord('z') + 1
for _i in range(len(ALPHABET) - 1):
    _SUCCESSOR_STR[ord(ALPHABET[_i])] = ALPHABET[_i + 1]
# Last char ('z') stays "" (carry)

# Successor table: byte -> next Base58 byte, or -1 if carry needed.
# Used in carry propagation which still operates on the bytearray.
_SUCCESSOR = [-1] * 123
for _i in range(len(ALPHABET) - 1):
    _SUCCESSOR[ord(ALPHABET[_i])] = ord(ALPHABET[_i + 1])

_FIRST_CHAR = ALPHABET[0]
_FIRST_BYTE = ord(ALPHABET[0])

_time_ns = time.time_ns
_urandom = os.urandom


# Track all live generators for fork-safety reset.
_all_generators: weakref.WeakSet["IdGenerator"] = weakref.WeakSet()


class IdGenerator:
    """Generates 22-char, Base58, time-sortable, collision-resistant unique IDs.

    Each ID is composed of three parts:
      - 8-char timestamp prefix  (milliseconds, Base58-encoded, sortable)
      - 6-char monotonic counter (randomly seeded each millisecond, incremented)
      - 8-char random tail       (independently random per ID, from os.urandom)

    IDs are strictly monotonically increasing within a single generator instance:
    across milliseconds by the timestamp prefix, and within the same millisecond
    by incrementing the counter (seeded from os.urandom at the start of each new
    millisecond). On the practically-impossible counter overflow (~58^6 ≈ 38
    billion increments within 1ms), the timestamp is bumped forward and the
    counter is reseeded.

    The random tail is freshly generated for every ID, making individual IDs
    unpredictable even when the counter value can be inferred.

    On the hot path (same millisecond, no carry), the counter tail is a single
    string char bumped via a successor lookup, and the random tail is decoded
    from the pre-sampled byte buffer. The return is a 3-part string concat:
    prefix_plus_counter_head (13 chars) + counter_tail (1 char) + random (8 chars).

    Properties:
      - 22 characters, fixed length
      - Lexicographically sortable by creation time
      - Monotonically increasing (within a single generator instance)
      - URL-safe, no ambiguous characters
      - ~58^14 (~1.8 x 10^24) total combinations per millisecond
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
        self._prefix_plus_counter_head = _FIRST_CHAR * 13
        self._counter_tail = _FIRST_CHAR
        # Bytearray for carry propagation (counter head, 5 bytes)
        self._counter_head_buf = bytearray(b"1" * 5)
        self._random_char_buffer = ""
        self._random_byte_buffer = b""
        self._random_byte_position = 0
        self._random_byte_len = 0

    def __call__(self) -> str:
        timestamp = _time_ns() // 1_000_000

        if timestamp > self._timestamp_cache_ms:
            # New millisecond (or first call): encode timestamp, seed counter.
            self._timestamp_cache_ms = timestamp
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
        end = position + _RANDOM_CHAR_COUNT
        if end > self._random_byte_len:
            self._refill_random()
            position = 0
            end = _RANDOM_CHAR_COUNT
        self._random_byte_position = end

        return (
            self._prefix_plus_counter_head
            + self._counter_tail
            + self._random_char_buffer[position:end]
        )

    def _seed_counter(self) -> None:
        """Seed the counter from the random byte buffer."""
        position = self._random_byte_position
        end = position + _COUNTER_CHAR_COUNT
        if end > self._random_byte_len:
            self._refill_random()
            position = 0
            end = _COUNTER_CHAR_COUNT
        self._random_byte_position = end
        counter_bytes = self._random_byte_buffer[position:end]
        self._counter_head_buf[:] = counter_bytes[:5]
        self._prefix_plus_counter_head = self._prefix_plus_counter_head[
            :8
        ] + counter_bytes[:5].decode("ascii")
        self._counter_tail = chr(counter_bytes[5])

    def _increment_counter_carry(self) -> None:
        """Handle carry propagation through counter head bytes.

        Called when the counter tail overflows. On full overflow
        (all 6 counter chars at max — practically impossible at ~38 billion
        increments per ms), bump the timestamp forward by 1ms and reseed.
        """
        buf = self._counter_head_buf
        successor = _SUCCESSOR
        for i in range(4, -1, -1):
            nxt = successor[buf[i]]
            if nxt >= 0:
                buf[i] = nxt
                for j in range(i + 1, 5):
                    buf[j] = _FIRST_BYTE
                self._counter_tail = _FIRST_CHAR
                self._prefix_plus_counter_head = self._prefix_plus_counter_head[
                    :8
                ] + buf.decode("ascii")
                return
        # Overflow: bump timestamp, reseed.
        self._timestamp_cache_ms += 1
        self._encode_timestamp(self._timestamp_cache_ms)
        self._seed_counter()

    def _encode_timestamp(self, timestamp: int) -> None:
        """Base58-encode millisecond timestamp into the prefix string."""
        lookup = _TIMESTAMP_LOOKUP
        timestamp, r7 = divmod(timestamp, BASE)
        timestamp, r6 = divmod(timestamp, BASE)
        timestamp, r5 = divmod(timestamp, BASE)
        timestamp, r4 = divmod(timestamp, BASE)
        timestamp, r3 = divmod(timestamp, BASE)
        timestamp, r2 = divmod(timestamp, BASE)
        timestamp, r1 = divmod(timestamp, BASE)
        ts_prefix = (
            lookup[timestamp]
            + lookup[r1]
            + lookup[r2]
            + lookup[r3]
            + lookup[r4]
            + lookup[r5]
            + lookup[r6]
            + lookup[r7]
        )
        # Update the prefix, preserving the counter head portion.
        self._prefix_plus_counter_head = ts_prefix + self._prefix_plus_counter_head[8:]

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
    """Generate a unique, time-sortable, 22-char Base58 ID.

    Thread-safe via threading.local. IDs are strictly monotonically increasing
    within each thread; across threads they are unique but unordered.
    """
    return _local.gen()
