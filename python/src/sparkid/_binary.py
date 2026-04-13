from sparkid._constants import ALPHABET, BASE, ID_LENGTH

# Binary encoding constants
_BINARY_LENGTH = 16
_INDEX_MASK = 0x3F
_BYTE_MASK = 0xFF
_PADDING_MASK = 0x03
_TAIL_SHIFT = 2
_INVALID_INDEX = 0xFF

# Reverse lookup: byte value -> Base58 index (0-57), or 0xFF if invalid.
# 128 entries cover all ASCII values; .encode("ascii") rejects anything higher.
_DECODE = [_INVALID_INDEX] * 128
for _i, _c in enumerate(ALPHABET):
    _DECODE[ord(_c)] = _i

# Forward lookup: Base58 index (0-57) -> character, "" for invalid (58-63).
# Used for both encoding and validation: single chars are truthy, "" is falsy.
_ENCODE = list(ALPHABET) + [""] * (64 - BASE)


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
            f"invalid SparkId length: expected {ID_LENGTH},"
            f" got {len(id_string)}"
        )

    try:
        encoded = id_string.encode("ascii")
    except UnicodeEncodeError as error:
        raise ValueError(
            "invalid non-ASCII character in SparkId"
        ) from error

    d = _DECODE
    i0 = d[encoded[0]]
    i1 = d[encoded[1]]
    i2 = d[encoded[2]]
    i3 = d[encoded[3]]
    i4 = d[encoded[4]]
    i5 = d[encoded[5]]
    i6 = d[encoded[6]]
    i7 = d[encoded[7]]
    i8 = d[encoded[8]]
    i9 = d[encoded[9]]
    i10 = d[encoded[10]]
    i11 = d[encoded[11]]
    i12 = d[encoded[12]]
    i13 = d[encoded[13]]
    i14 = d[encoded[14]]
    i15 = d[encoded[15]]
    i16 = d[encoded[16]]
    i17 = d[encoded[17]]
    i18 = d[encoded[18]]
    i19 = d[encoded[19]]
    i20 = d[encoded[20]]

    if (
        i0 | i1 | i2 | i3 | i4 | i5 | i6 | i7 | i8 | i9 | i10
        | i11 | i12 | i13 | i14 | i15 | i16 | i17 | i18 | i19
        | i20
    ) == _INVALID_INDEX:
        for position, byte_value in enumerate(encoded):
            if d[byte_value] == _INVALID_INDEX:
                raise ValueError(
                    f"invalid character {chr(byte_value)!r}"
                    f" at position {position} in SparkId"
                )

    return bytes((
        (i0 << 2) | (i1 >> 4),
        ((i1 & 0xF) << 4) | (i2 >> 2),
        ((i2 & 0x3) << 6) | i3,
        (i4 << 2) | (i5 >> 4),
        ((i5 & 0xF) << 4) | (i6 >> 2),
        ((i6 & 0x3) << 6) | i7,
        (i8 << 2) | (i9 >> 4),
        ((i9 & 0xF) << 4) | (i10 >> 2),
        ((i10 & 0x3) << 6) | i11,
        (i12 << 2) | (i13 >> 4),
        ((i13 & 0xF) << 4) | (i14 >> 2),
        ((i14 & 0x3) << 6) | i15,
        (i16 << 2) | (i17 >> 4),
        ((i17 & 0xF) << 4) | (i18 >> 2),
        ((i18 & 0x3) << 6) | i19,
        i20 << _TAIL_SHIFT,
    ))


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
            f"invalid binary length: expected {_BINARY_LENGTH},"
            f" got {len(data)}"
        )

    a0 = (data[0] >> 2) & _INDEX_MASK
    a1 = ((data[0] & 0x3) << 4) | (data[1] >> 4)
    a2 = ((data[1] & 0xF) << 2) | (data[2] >> 6)
    a3 = data[2] & _INDEX_MASK
    a4 = (data[3] >> 2) & _INDEX_MASK
    a5 = ((data[3] & 0x3) << 4) | (data[4] >> 4)
    a6 = ((data[4] & 0xF) << 2) | (data[5] >> 6)
    a7 = data[5] & _INDEX_MASK
    a8 = (data[6] >> 2) & _INDEX_MASK
    a9 = ((data[6] & 0x3) << 4) | (data[7] >> 4)
    a10 = ((data[7] & 0xF) << 2) | (data[8] >> 6)
    a11 = data[8] & _INDEX_MASK
    a12 = (data[9] >> 2) & _INDEX_MASK
    a13 = ((data[9] & 0x3) << 4) | (data[10] >> 4)
    a14 = ((data[10] & 0xF) << 2) | (data[11] >> 6)
    a15 = data[11] & _INDEX_MASK
    a16 = (data[12] >> 2) & _INDEX_MASK
    a17 = ((data[12] & 0x3) << 4) | (data[13] >> 4)
    a18 = ((data[13] & 0xF) << 2) | (data[14] >> 6)
    a19 = data[14] & _INDEX_MASK
    a20 = (data[15] >> _TAIL_SHIFT) & _INDEX_MASK

    e = _ENCODE
    e0 = e[a0]
    e1 = e[a1]
    e2 = e[a2]
    e3 = e[a3]
    e4 = e[a4]
    e5 = e[a5]
    e6 = e[a6]
    e7 = e[a7]
    e8 = e[a8]
    e9 = e[a9]
    e10 = e[a10]
    e11 = e[a11]
    e12 = e[a12]
    e13 = e[a13]
    e14 = e[a14]
    e15 = e[a15]
    e16 = e[a16]
    e17 = e[a17]
    e18 = e[a18]
    e19 = e[a19]
    e20 = e[a20]

    if (data[15] & _PADDING_MASK) != 0:
        raise ValueError(
            "non-zero padding bits in binary SparkId"
        )

    # Invalid indices map to "" in _ENCODE, making the result shorter than 21.
    result = "".join((
        e0, e1, e2, e3, e4, e5, e6,
        e7, e8, e9, e10, e11, e12,
        e13, e14, e15, e16, e17, e18,
        e19, e20,
    ))
    if len(result) != ID_LENGTH:
        raise ValueError(
            "invalid 6-bit index in binary SparkId"
        )
    return result
