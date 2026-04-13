import { ALPHABET, BASE, ID_LENGTH } from "./constants";

// Binary encoding constants
const BINARY_LENGTH = 16;
const CHARACTERS_PER_GROUP = 4;
const BYTES_PER_GROUP = 3;
const FULL_GROUP_COUNT = 5;
const PACKED_GROUPS_BYTE_COUNT = FULL_GROUP_COUNT * BYTES_PER_GROUP; // 15
const TAIL_CHARACTER_INDEX = ID_LENGTH - 1; // 20
const INDEX_MASK = 0x3f;
const BYTE_MASK = 0xff;
const PADDING_MASK = 0x03;
const TAIL_SHIFT = 2;
const INVALID_INDEX = 0xff;

// Reverse lookup: byte value -> Base58 index (0-57), or 0xFF if invalid.
const DECODE = new Uint8Array(256).fill(INVALID_INDEX);
for (let i = 0; i < BASE; i++) DECODE[ALPHABET.charCodeAt(i)] = i;

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

  const d = DECODE;

  // Validate all characters before packing.
  for (let i = 0; i < ID_LENGTH; i++) {
    const charCode = id.charCodeAt(i);
    if (charCode > BYTE_MASK || d[charCode] === INVALID_INDEX) {
      throw new RangeError(
        `invalid character '${id[i]}' at position ${i} in SparkId`,
      );
    }
  }

  const out = new Uint8Array(BINARY_LENGTH);

  for (
    let characterIndex = 0, byteIndex = 0;
    byteIndex < PACKED_GROUPS_BYTE_COUNT;
    characterIndex += CHARACTERS_PER_GROUP, byteIndex += BYTES_PER_GROUP
  ) {
    const packed =
      (d[id.charCodeAt(characterIndex)] << 18) |
      (d[id.charCodeAt(characterIndex + 1)] << 12) |
      (d[id.charCodeAt(characterIndex + 2)] << 6) |
      d[id.charCodeAt(characterIndex + 3)];

    out[byteIndex] = packed >>> 16;
    out[byteIndex + 1] = (packed >>> 8) & BYTE_MASK;
    out[byteIndex + 2] = packed & BYTE_MASK;
  }

  out[PACKED_GROUPS_BYTE_COUNT] =
    d[id.charCodeAt(TAIL_CHARACTER_INDEX)] << TAIL_SHIFT;
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

  const characterCodes = new Array<number>(ID_LENGTH);

  for (
    let characterIndex = 0, byteIndex = 0;
    byteIndex < PACKED_GROUPS_BYTE_COUNT;
    characterIndex += CHARACTERS_PER_GROUP, byteIndex += BYTES_PER_GROUP
  ) {
    const packed =
      (bytes[byteIndex] << 16) | (bytes[byteIndex + 1] << 8) | bytes[byteIndex + 2];

    const a = (packed >>> 18) & INDEX_MASK;
    const b = (packed >>> 12) & INDEX_MASK;
    const c = (packed >>> 6) & INDEX_MASK;
    const d = packed & INDEX_MASK;
    if (a >= BASE || b >= BASE || c >= BASE || d >= BASE) {
      throw new RangeError(
        `invalid 6-bit index in binary SparkId at byte ${byteIndex}`,
      );
    }

    characterCodes[characterIndex] = ALPHABET.charCodeAt(a);
    characterCodes[characterIndex + 1] = ALPHABET.charCodeAt(b);
    characterCodes[characterIndex + 2] = ALPHABET.charCodeAt(c);
    characterCodes[characterIndex + 3] = ALPHABET.charCodeAt(d);
  }

  const lastIndex =
    (bytes[PACKED_GROUPS_BYTE_COUNT] >>> TAIL_SHIFT) & INDEX_MASK;
  if (
    lastIndex >= BASE ||
    (bytes[PACKED_GROUPS_BYTE_COUNT] & PADDING_MASK) !== 0
  ) {
    throw new RangeError(
      "invalid tail byte or non-zero padding in binary SparkId",
    );
  }
  characterCodes[TAIL_CHARACTER_INDEX] = ALPHABET.charCodeAt(lastIndex);
  return String.fromCharCode(...characterCodes);
}
