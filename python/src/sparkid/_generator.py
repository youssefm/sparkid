import os
import threading
import time
import weakref
from datetime import datetime, timezone

# Base58 alphabet — excludes visually ambiguous characters (0, O, I, l)
ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
BASE = len(ALPHABET)  # 58

# ID structure: [8-char timestamp][6-char counter][7-char random] = 21 chars
_TIMESTAMP_CHAR_COUNT = 8
_COUNTER_CHAR_COUNT = 6
_RANDOM_CHAR_COUNT = 7
_ID_LENGTH = _TIMESTAMP_CHAR_COUNT + _COUNTER_CHAR_COUNT + _RANDOM_CHAR_COUNT

_COUNTER_HEAD_CHAR_COUNT = _COUNTER_CHAR_COUNT - 1  # 5
_COUNTER_HEAD_LAST_INDEX = _COUNTER_HEAD_CHAR_COUNT - 1  # 4
_PREFIX_COUNTER_HEAD_LENGTH = _TIMESTAMP_CHAR_COUNT + _COUNTER_HEAD_CHAR_COUNT  # 13

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

# Binary encoding constants
_BINARY_LENGTH = 16
_CHARACTERS_PER_GROUP = 4
_BYTES_PER_GROUP = 3
_FULL_GROUP_COUNT = 5
_PACKED_GROUPS_BYTE_COUNT = _FULL_GROUP_COUNT * _BYTES_PER_GROUP  # 15
_TAIL_CHARACTER_INDEX = _ID_LENGTH - 1  # 20
_INDEX_MASK = 0x3F
_BYTE_MASK = 0xFF
_PADDING_MASK = 0x03
_TAIL_SHIFT = 2
_INVALID_INDEX = 0xFF
_ASCII_TABLE_SIZE = 128

# Reverse lookup: ASCII code point -> Base58 index (0-57), or 0xFF if invalid.
# Used for binary packing in to_bytes.
_DECODE = [_INVALID_INDEX] * _ASCII_TABLE_SIZE
for _i, _c in enumerate(ALPHABET):
    _DECODE[ord(_c)] = _i

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
        self._prefix_plus_counter_head = _FIRST_CHAR * _PREFIX_COUNTER_HEAD_LENGTH
        self._counter_tail = _FIRST_CHAR
        # Bytearray for carry propagation (counter head)
        self._counter_head_buf = bytearray([_FIRST_BYTE] * _COUNTER_HEAD_CHAR_COUNT)
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
        self._counter_head_buf[:] = counter_bytes[:_COUNTER_HEAD_CHAR_COUNT]
        self._prefix_plus_counter_head = self._prefix_plus_counter_head[
            :_TIMESTAMP_CHAR_COUNT
        ] + counter_bytes[:_COUNTER_HEAD_CHAR_COUNT].decode("ascii")
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
                self._prefix_plus_counter_head = self._prefix_plus_counter_head[
                    :_TIMESTAMP_CHAR_COUNT
                ] + buf.decode("ascii")
                return
            buf[i] = _FIRST_BYTE
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
        self._prefix_plus_counter_head = (
            ts_prefix + self._prefix_plus_counter_head[_TIMESTAMP_CHAR_COUNT:]
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
    if len(id) != _ID_LENGTH:
        raise ValueError(
            f"extract_timestamp: expected a {_ID_LENGTH}-character string,"
            f" got {len(id)}"
        )
    index = _BASE58_INDEX
    for i, ch in enumerate(id):
        if ch not in index:
            raise ValueError(
                f"extract_timestamp: invalid Base58 character {ch!r} at position {i}"
            )
    ms = 0
    for ch in id[:_TIMESTAMP_CHAR_COUNT]:
        ms = ms * BASE + index[ch]
    return datetime.fromtimestamp(ms / 1000, tz=timezone.utc)


def to_bytes(id_string: str) -> bytes:
    """Pack a 21-char SparkId string into 16 bytes.

    The binary format stores each Base58 character as a 6-bit index,
    packed MSB-first into 16 bytes (126 data bits + 2 padding bits).
    The encoding uses only table lookups and bit shifts — no division
    or multiplication.

    Sort order is preserved: byte-wise comparison on the binary output
    gives the same ordering as string comparison on the original IDs.

    Args:
        id_string: A 21-character Base58 sparkid string.

    Returns:
        A 16-byte ``bytes`` object containing the packed binary representation.

    Raises:
        ValueError: If *id_string* is not a valid 21-char Base58 string.
    """
    if len(id_string) != _ID_LENGTH:
        raise ValueError(
            f"invalid SparkId length: expected {_ID_LENGTH}, got {len(id_string)}"
        )

    decode = _DECODE

    try:
        encoded = id_string.encode("ascii")
    except UnicodeEncodeError as error:
        raise ValueError(
            f"invalid non-ASCII character in SparkId"
        ) from error

    # Validate all characters before packing.
    for position, byte_value in enumerate(encoded):
        if decode[byte_value] == _INVALID_INDEX:
            raise ValueError(
                f"invalid character {chr(byte_value)!r} at position {position} in SparkId"
            )

    out = bytearray(_BINARY_LENGTH)

    character_index = 0
    for byte_index in range(0, _PACKED_GROUPS_BYTE_COUNT, _BYTES_PER_GROUP):
        packed = (
            (decode[encoded[character_index]] << 18)
            | (decode[encoded[character_index + 1]] << 12)
            | (decode[encoded[character_index + 2]] << 6)
            | decode[encoded[character_index + 3]]
        )
        out[byte_index] = (packed >> 16) & _BYTE_MASK
        out[byte_index + 1] = (packed >> 8) & _BYTE_MASK
        out[byte_index + 2] = packed & _BYTE_MASK
        character_index += _CHARACTERS_PER_GROUP

    out[_PACKED_GROUPS_BYTE_COUNT] = decode[encoded[_TAIL_CHARACTER_INDEX]] << _TAIL_SHIFT
    return bytes(out)


def from_bytes(data: bytes) -> str:
    """Unpack 16 bytes to a 21-char SparkId string.

    Reverses the packing performed by :func:`to_bytes`. Each 3-byte group
    is split into four 6-bit indices, which are mapped back to Base58
    characters via the alphabet table.

    Args:
        data: A 16-byte ``bytes`` object containing a packed SparkId.

    Returns:
        The 21-character Base58 sparkid string.

    Raises:
        ValueError: If *data* is not exactly 16 bytes, contains
            out-of-range 6-bit indices (≥ 58), or has non-zero padding bits.
    """
    if len(data) != _BINARY_LENGTH:
        raise ValueError(
            f"invalid binary length: expected {_BINARY_LENGTH}, got {len(data)}"
        )

    alphabet = ALPHABET
    characters: list[str] = []

    for byte_index in range(0, _PACKED_GROUPS_BYTE_COUNT, _BYTES_PER_GROUP):
        packed = (data[byte_index] << 16) | (data[byte_index + 1] << 8) | data[byte_index + 2]
        a = (packed >> 18) & _INDEX_MASK
        b = (packed >> 12) & _INDEX_MASK
        c = (packed >> 6) & _INDEX_MASK
        d = packed & _INDEX_MASK
        if a >= BASE or b >= BASE or c >= BASE or d >= BASE:
            raise ValueError(
                f"invalid 6-bit index in binary SparkId at byte {byte_index}"
            )
        characters.append(alphabet[a])
        characters.append(alphabet[b])
        characters.append(alphabet[c])
        characters.append(alphabet[d])

    last_index = (data[_PACKED_GROUPS_BYTE_COUNT] >> _TAIL_SHIFT) & _INDEX_MASK
    if last_index >= BASE or (data[_PACKED_GROUPS_BYTE_COUNT] & _PADDING_MASK) != 0:
        raise ValueError(
            "invalid tail byte or non-zero padding in binary SparkId"
        )
    characters.append(alphabet[last_index])
    return "".join(characters)
