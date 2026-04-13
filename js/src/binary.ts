import { ALPHABET, BASE, ID_LENGTH } from "./constants";

// Binary encoding constants
const BINARY_LENGTH = 16;
const INDEX_MASK = 0x3f;
const ASCII_MASK = 0x7f;
const PADDING_MASK = 0x03;
const TAIL_SHIFT = 2;
const INVALID_INDEX = 0xff;

// Reverse lookup: byte value -> Base58 index (0-57), or 0xFF if invalid.
// 128 entries cover all ASCII values; the charCode guard rejects anything higher.
const DECODE = new Uint8Array(128).fill(INVALID_INDEX);
for (let i = 0; i < BASE; i++) DECODE[ALPHABET.charCodeAt(i)] = i;

// Validation lookup: 1 if index is valid (0-57), 0 otherwise. 64 entries for 6-bit range.
const VALID_INDEX = new Uint8Array(64);
for (let i = 0; i < BASE; i++) VALID_INDEX[i] = 1;

// Forward lookup: Base58 index (0-57) -> character code.
const ENCODE = new Uint8Array(BASE);
for (let i = 0; i < BASE; i++) ENCODE[i] = ALPHABET.charCodeAt(i);

/**
 * Pack a 21-char SparkId string into a 16-byte binary representation.
 *
 * The binary format stores each Base58 character as a 6-bit index,
 * packed MSB-first into 16 bytes (126 data bits + 2 padding bits).
 * The encoding uses only table lookups and bit shifts — no division
 * or multiplication.
 *
 * Sort order is preserved: `memcmp` on the binary bytes gives the
 * same ordering as string comparison on the original IDs.
 *
 * @param id - A 21-character Base58 sparkid string.
 * @returns A 16-byte Uint8Array containing the packed binary representation.
 * @throws {RangeError} If `id` is not a valid 21-char Base58 string.
 */
export function toBytes(id: string): Uint8Array {
  if (id.length !== ID_LENGTH)
    throw new RangeError(
      `invalid SparkId length: expected ${ID_LENGTH}, got ${id.length}`,
    );

  const c0 = id.charCodeAt(0);
  const c1 = id.charCodeAt(1);
  const c2 = id.charCodeAt(2);
  const c3 = id.charCodeAt(3);
  const c4 = id.charCodeAt(4);
  const c5 = id.charCodeAt(5);
  const c6 = id.charCodeAt(6);
  const c7 = id.charCodeAt(7);
  const c8 = id.charCodeAt(8);
  const c9 = id.charCodeAt(9);
  const c10 = id.charCodeAt(10);
  const c11 = id.charCodeAt(11);
  const c12 = id.charCodeAt(12);
  const c13 = id.charCodeAt(13);
  const c14 = id.charCodeAt(14);
  const c15 = id.charCodeAt(15);
  const c16 = id.charCodeAt(16);
  const c17 = id.charCodeAt(17);
  const c18 = id.charCodeAt(18);
  const c19 = id.charCodeAt(19);
  const c20 = id.charCodeAt(20);

  // Reject any character outside 0-127 before DECODE lookup.
  if (
    (c0 | c1 | c2 | c3 | c4 | c5 | c6 | c7 | c8 | c9 | c10 |
      c11 | c12 | c13 | c14 | c15 | c16 | c17 | c18 | c19 | c20) >
    ASCII_MASK
  ) {
    for (let i = 0; i < ID_LENGTH; i++) {
      if (id.charCodeAt(i) > ASCII_MASK) {
        throw new RangeError(
          `invalid character '${id[i]}' at position ${i} in SparkId`,
        );
      }
    }
  }

  // All char codes are in 0-127, so DECODE lookups are in-bounds.
  const d = DECODE;
  const i0 = d[c0];
  const i1 = d[c1];
  const i2 = d[c2];
  const i3 = d[c3];
  const i4 = d[c4];
  const i5 = d[c5];
  const i6 = d[c6];
  const i7 = d[c7];
  const i8 = d[c8];
  const i9 = d[c9];
  const i10 = d[c10];
  const i11 = d[c11];
  const i12 = d[c12];
  const i13 = d[c13];
  const i14 = d[c14];
  const i15 = d[c15];
  const i16 = d[c16];
  const i17 = d[c17];
  const i18 = d[c18];
  const i19 = d[c19];
  const i20 = d[c20];

  if (
    (i0 | i1 | i2 | i3 | i4 | i5 | i6 | i7 | i8 | i9 | i10 |
      i11 | i12 | i13 | i14 | i15 | i16 | i17 | i18 | i19 | i20) ===
    INVALID_INDEX
  ) {
    for (let i = 0; i < ID_LENGTH; i++) {
      if (d[id.charCodeAt(i)] === INVALID_INDEX) {
        throw new RangeError(
          `invalid character '${id[i]}' at position ${i} in SparkId`,
        );
      }
    }
  }


  const out = new Uint8Array(BINARY_LENGTH);
  out[0] = (i0 << 2) | (i1 >>> 4);
  out[1] = ((i1 & 0xf) << 4) | (i2 >>> 2);
  out[2] = ((i2 & 0x3) << 6) | i3;
  out[3] = (i4 << 2) | (i5 >>> 4);
  out[4] = ((i5 & 0xf) << 4) | (i6 >>> 2);
  out[5] = ((i6 & 0x3) << 6) | i7;
  out[6] = (i8 << 2) | (i9 >>> 4);
  out[7] = ((i9 & 0xf) << 4) | (i10 >>> 2);
  out[8] = ((i10 & 0x3) << 6) | i11;
  out[9] = (i12 << 2) | (i13 >>> 4);
  out[10] = ((i13 & 0xf) << 4) | (i14 >>> 2);
  out[11] = ((i14 & 0x3) << 6) | i15;
  out[12] = (i16 << 2) | (i17 >>> 4);
  out[13] = ((i17 & 0xf) << 4) | (i18 >>> 2);
  out[14] = ((i18 & 0x3) << 6) | i19;
  out[15] = i20 << TAIL_SHIFT;
  return out;
}

/**
 * Unpack a 16-byte binary representation back to a 21-char SparkId string.
 *
 * Reverses the packing performed by {@link toBytes}. Each 3-byte group
 * is split into four 6-bit indices, which are mapped back to Base58
 * characters via the alphabet table.
 *
 * @param bytes - A 16-byte Uint8Array containing a packed SparkId.
 * @returns The 21-character Base58 sparkid string.
 * @throws {RangeError} If `bytes` is not exactly 16 bytes, contains
 *   out-of-range 6-bit indices (≥58), or has non-zero padding bits.
 */
export function fromBytes(bytes: Uint8Array): string {
  if (bytes.length !== BINARY_LENGTH)
    throw new RangeError(
      `invalid binary length: expected ${BINARY_LENGTH}, got ${bytes.length}`,
    );

  const a0 = (bytes[0] >>> 2) & INDEX_MASK;
  const a1 = ((bytes[0] & 0x3) << 4) | (bytes[1] >>> 4);
  const a2 = ((bytes[1] & 0xf) << 2) | (bytes[2] >>> 6);
  const a3 = bytes[2] & INDEX_MASK;
  const a4 = (bytes[3] >>> 2) & INDEX_MASK;
  const a5 = ((bytes[3] & 0x3) << 4) | (bytes[4] >>> 4);
  const a6 = ((bytes[4] & 0xf) << 2) | (bytes[5] >>> 6);
  const a7 = bytes[5] & INDEX_MASK;
  const a8 = (bytes[6] >>> 2) & INDEX_MASK;
  const a9 = ((bytes[6] & 0x3) << 4) | (bytes[7] >>> 4);
  const a10 = ((bytes[7] & 0xf) << 2) | (bytes[8] >>> 6);
  const a11 = bytes[8] & INDEX_MASK;
  const a12 = (bytes[9] >>> 2) & INDEX_MASK;
  const a13 = ((bytes[9] & 0x3) << 4) | (bytes[10] >>> 4);
  const a14 = ((bytes[10] & 0xf) << 2) | (bytes[11] >>> 6);
  const a15 = bytes[11] & INDEX_MASK;
  const a16 = (bytes[12] >>> 2) & INDEX_MASK;
  const a17 = ((bytes[12] & 0x3) << 4) | (bytes[13] >>> 4);
  const a18 = ((bytes[13] & 0xf) << 2) | (bytes[14] >>> 6);
  const a19 = bytes[14] & INDEX_MASK;
  const a20 = (bytes[15] >>> TAIL_SHIFT) & INDEX_MASK;

  const v = VALID_INDEX;
  if (
    !(
      v[a0] & v[a1] & v[a2] & v[a3] & v[a4] & v[a5] & v[a6] & v[a7] &
      v[a8] & v[a9] & v[a10] & v[a11] & v[a12] & v[a13] & v[a14] &
      v[a15] & v[a16] & v[a17] & v[a18] & v[a19] & v[a20]
    )
  ) {
    throw new RangeError("invalid 6-bit index in binary SparkId");
  }

  if ((bytes[15] & PADDING_MASK) !== 0) {
    throw new RangeError("non-zero padding bits in binary SparkId");
  }

  const e = ENCODE;
  return String.fromCharCode(
    e[a0], e[a1], e[a2], e[a3], e[a4], e[a5], e[a6],
    e[a7], e[a8], e[a9], e[a10], e[a11], e[a12],
    e[a13], e[a14], e[a15], e[a16], e[a17], e[a18],
    e[a19], e[a20],
  );
}
