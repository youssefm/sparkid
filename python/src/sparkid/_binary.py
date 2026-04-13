from sparkid._constants import ALPHABET, BASE, ID_LENGTH

# Binary encoding constants
_BINARY_LENGTH = 16
_CHARACTERS_PER_GROUP = 4
_BYTES_PER_GROUP = 3
_FULL_GROUP_COUNT = 5
_PACKED_GROUPS_BYTE_COUNT = _FULL_GROUP_COUNT * _BYTES_PER_GROUP  # 15
_TAIL_CHARACTER_INDEX = ID_LENGTH - 1  # 20
_INDEX_MASK = 0x3F
_BYTE_MASK = 0xFF
_PADDING_MASK = 0x03
_TAIL_SHIFT = 2
_INVALID_INDEX = 0xFF
_ASCII_TABLE_SIZE = 128

# Reverse lookup: ASCII code point -> Base58 index (0-57), or 0xFF if invalid.
_DECODE = [_INVALID_INDEX] * _ASCII_TABLE_SIZE
for _i, _c in enumerate(ALPHABET):
    _DECODE[ord(_c)] = _i


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
    if len(id_string) != ID_LENGTH:
        raise ValueError(
            f"invalid SparkId length: expected {ID_LENGTH}, got {len(id_string)}"
        )

    decode = _DECODE

    try:
        encoded = id_string.encode("ascii")
    except UnicodeEncodeError as error:
        raise ValueError(
            "invalid non-ASCII character in SparkId"
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
