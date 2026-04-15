// Base58 alphabet — excludes visually ambiguous characters (0, O, I, l)
export const ALPHABET =
  "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
export const BASE = ALPHABET.length; // 58

// ID structure: [8-char timestamp][6-char counter][7-char random] = 21 chars
export const TIMESTAMP_CHAR_COUNT = 8;
export const COUNTER_CHAR_COUNT = 6;
export const RANDOM_CHAR_COUNT = 7;
export const ID_LENGTH =
  TIMESTAMP_CHAR_COUNT + COUNTER_CHAR_COUNT + RANDOM_CHAR_COUNT;

// Maximum encodable timestamp: 58^8 - 1 (8 Base58 chars)
export const MAX_TIMESTAMP = BASE ** TIMESTAMP_CHAR_COUNT - 1; // 128_063_081_718_015
