import {
  ALPHABET,
  BASE,
  TIMESTAMP_CHAR_COUNT,
  COUNTER_CHAR_COUNT,
  RANDOM_CHAR_COUNT,
  ID_LENGTH,
  MAX_TIMESTAMP,
} from "./constants";

// How many random bytes to fetch per batch. After rejection sampling,
// ~90.6% survive (58/64), yielding ~14848 valid chars (~2121 IDs).
const RANDOM_BATCH_SIZE = 16384;

// Derived layout constants
const COUNTER_HEAD_CHAR_COUNT = COUNTER_CHAR_COUNT - 1; // 5
const COUNTER_HEAD_LAST_INDEX =
  TIMESTAMP_CHAR_COUNT + COUNTER_HEAD_CHAR_COUNT - 1; // 12
const SUCCESSOR_TABLE_SIZE = ALPHABET.charCodeAt(BASE - 1) + 1; // 123

// Timestamp encoding: remainder (0-57) -> single char
const TIMESTAMP_LOOKUP: string[] = Array.from(
  { length: BASE },
  (_, i) => ALPHABET[i],
);

// Random encoding: for each byte 0-255, mask to 6 bits (0-63).
// Values < 58 map to their alphabet char code; values >= 58 are rejected (0).
// No modulo bias.
const RANDOM_CHARCODE_LOOKUP: Uint8Array = new Uint8Array(256);
for (let byte = 0; byte < 256; byte++) {
  const value = byte & 0x3f;
  RANDOM_CHARCODE_LOOKUP[byte] = value < BASE ? ALPHABET.charCodeAt(value) : 0;
}

// Successor table: charCode -> next Base58 char code, or 0 if carry needed.
// Using char codes avoids string allocation on the hot path.
const SUCCESSOR_CC: Uint8Array = new Uint8Array(SUCCESSOR_TABLE_SIZE);
for (let i = 0; i < BASE - 1; i++) {
  SUCCESSOR_CC[ALPHABET.charCodeAt(i)] = ALPHABET.charCodeAt(i + 1);
}
// Last char ('z') stays 0 (carry)

// Successor table (string version) for carry propagation
const SUCCESSOR: string[] = new Array<string>(SUCCESSOR_TABLE_SIZE).fill("");
for (let i = 0; i < BASE - 1; i++) {
  SUCCESSOR[ALPHABET.charCodeAt(i)] = ALPHABET[i + 1];
}

// Reverse lookup: charCode -> Base58 index (0-57), or -1 if invalid.
// Used for timestamp decoding in extractTimestamp.
const BASE58_INDEX: Int8Array = new Int8Array(SUCCESSOR_TABLE_SIZE).fill(-1);
for (let i = 0; i < BASE; i++) {
  BASE58_INDEX[ALPHABET.charCodeAt(i)] = i;
}

const FIRST_CHAR = ALPHABET[0];
const FIRST_CHAR_CODE = ALPHABET.charCodeAt(0);

// Pre-allocated buffers for getRandomValues and rejection-sampled char codes.
// Allocated lazily on first use (in refillRandom) to avoid 32KB upfront cost
// at import time. Held in a const object so inner functions can pull fast
// const-local aliases (V8 optimizes const typed-array refs better than let).
let randomRaw: Uint8Array<ArrayBuffer> | undefined;
let randomCharCodes: Uint8Array<ArrayBuffer>;

// Timestamp cache — only re-encoded when the millisecond advances
let timestampCacheMs = 0;
let timestampCachePrefix = "";
let randomCharCount = 0;
let randomCharPosition = 0;

// The counter is split into a stable head (5 chars) and a frequently-changing
// tail (1 char code). On the hot path (same millisecond, no carry), only the
// tail char code is bumped via a single successor lookup — no string ops needed.
// The prefix (8 chars) and counter head (5 chars) are pre-concatenated into a
// single 13-char string so the final return is a 2-part concat:
//   prefixPlusCounterHead + String.fromCharCode(counterTailCharCode, ...7 random chars)
let prefixPlusCounterHead = "";
let counterTailCharCode = FIRST_CHAR_CODE;

function encodeTimestamp(timestamp: number): void {
  if (timestamp < 0 || timestamp > MAX_TIMESTAMP) {
    throw new RangeError(
      `Timestamp out of range: ${timestamp} (valid range: 0 to ${MAX_TIMESTAMP})`,
    );
  }
  timestampCacheMs = timestamp;
  let remainder: number;
  remainder = timestamp % BASE;
  timestamp = Math.trunc(timestamp / BASE);
  const char7 = TIMESTAMP_LOOKUP[remainder];
  remainder = timestamp % BASE;
  timestamp = Math.trunc(timestamp / BASE);
  const char6 = TIMESTAMP_LOOKUP[remainder];
  remainder = timestamp % BASE;
  timestamp = Math.trunc(timestamp / BASE);
  const char5 = TIMESTAMP_LOOKUP[remainder];
  remainder = timestamp % BASE;
  timestamp = Math.trunc(timestamp / BASE);
  const char4 = TIMESTAMP_LOOKUP[remainder];
  remainder = timestamp % BASE;
  timestamp = Math.trunc(timestamp / BASE);
  const char3 = TIMESTAMP_LOOKUP[remainder];
  remainder = timestamp % BASE;
  timestamp = Math.trunc(timestamp / BASE);
  const char2 = TIMESTAMP_LOOKUP[remainder];
  remainder = timestamp % BASE;
  timestamp = Math.trunc(timestamp / BASE);
  const char1 = TIMESTAMP_LOOKUP[remainder];
  timestampCachePrefix =
    TIMESTAMP_LOOKUP[timestamp] +
    char1 +
    char2 +
    char3 +
    char4 +
    char5 +
    char6 +
    char7;
}

/**
 * Increment the cached timestamp prefix string by `delta` (1..58).
 *
 * Decodes the last character to a Base58 index, adds delta, and replaces
 * the character. If the result carries (>= 58), falls back to full
 * re-encode since carry is rare (~1/58 for delta=1).
 */
function incrementEncodedTimestamp(delta: number): void {
  const newIndex = BASE58_INDEX[timestampCachePrefix.charCodeAt(7)] + delta;
  if (newIndex < BASE) {
    timestampCachePrefix =
      timestampCachePrefix.substring(0, 7) + TIMESTAMP_LOOKUP[newIndex];
    return;
  }
  // Carry: set last digit to remainder, propagate carry=1 backward.
  let prefix =
    timestampCachePrefix.substring(0, 7) + TIMESTAMP_LOOKUP[newIndex - BASE];
  for (let i = 6; i >= 0; i--) {
    const next = SUCCESSOR[prefix.charCodeAt(i)];
    if (next) {
      timestampCachePrefix =
        prefix.substring(0, i) + next + prefix.substring(i + 1);
      return;
    }
    prefix = prefix.substring(0, i) + FIRST_CHAR + prefix.substring(i + 1);
  }
  throw new RangeError(
    `Timestamp out of range: ${timestampCacheMs} (valid range: 0 to ${MAX_TIMESTAMP})`,
  );
}

function refillRandom(): void {
  if (randomRaw === undefined) {
    randomRaw = new Uint8Array(RANDOM_BATCH_SIZE);
    randomCharCodes = new Uint8Array(RANDOM_BATCH_SIZE);
  }
  crypto.getRandomValues(randomRaw);
  const lookup = RANDOM_CHARCODE_LOOKUP;
  let count = 0;
  for (let i = 0; i < RANDOM_BATCH_SIZE; i++) {
    const cc = lookup[randomRaw[i]];
    if (cc !== 0) {
      randomCharCodes[count++] = cc;
    }
  }
  randomCharCount = count;
  randomCharPosition = 0;
}

function seedCounter(): void {
  while (randomCharPosition + COUNTER_CHAR_COUNT > randomCharCount) {
    refillRandom();
  }
  const pos = randomCharPosition;
  randomCharPosition = pos + COUNTER_CHAR_COUNT;
  prefixPlusCounterHead =
    timestampCachePrefix +
    String.fromCharCode(
      randomCharCodes[pos],
      randomCharCodes[pos + 1],
      randomCharCodes[pos + 2],
      randomCharCodes[pos + 3],
      randomCharCodes[pos + 4],
    );
  counterTailCharCode = randomCharCodes[pos + 5];
}

/**
 * Handle carry propagation through the counter head portion.
 *
 * Called when the counter tail char overflows. Walks backward through
 * the counter head chars (positions 4 down to 0, stored at indices 12
 * down to 8 in prefixPlusCounterHead). On full overflow (all 6 counter
 * chars maxed), bumps the timestamp forward by 1 ms and reseeds.
 * Because the counter is randomly seeded each ms, overflow probability
 * is n / 58^6 for n IDs generated in that ms.
 */
function incrementCarry(): void {
  const pph = prefixPlusCounterHead;
  for (let i = COUNTER_HEAD_LAST_INDEX; i >= TIMESTAMP_CHAR_COUNT; i--) {
    const next = SUCCESSOR[pph.charCodeAt(i)];
    if (next) {
      prefixPlusCounterHead =
        pph.substring(0, i) +
        next +
        FIRST_CHAR.repeat(COUNTER_HEAD_LAST_INDEX - i);
      counterTailCharCode = FIRST_CHAR_CODE;
      return;
    }
  }
  // Full overflow: bump timestamp, reseed.
  encodeTimestamp(timestampCacheMs + 1);
  seedCounter();
}

/**
 * Generate a unique, time-sortable, 21-char Base58 ID.
 *
 * Each ID is composed of three parts:
 *   - 8-char timestamp prefix   (milliseconds, Base58-encoded, sortable)
 *   - 6-char monotonic counter  (randomly seeded each ms, incremented)
 *   - 7-char random tail        (independently random per ID)
 *
 * IDs are strictly monotonically increasing within a single process:
 * across milliseconds by the timestamp prefix, and within the same millisecond
 * by incrementing the counter (seeded from crypto.getRandomValues at the
 * start of each new millisecond). Since JavaScript is single-threaded, this
 * gives process-wide monotonicity with no additional coordination needed.
 *
 * The random tail is freshly generated for every ID, making individual IDs
 * unpredictable even when the counter value can be inferred.
 *
 * Properties:
 *   - 21 characters, fixed length
 *   - Lexicographically sortable by creation time
 *   - Monotonically increasing (within a single process)
 *   - URL-safe, no ambiguous characters
 *   - ~58^13 (~8.4 x 10^22) total combinations per millisecond
 *   - Cryptographically secure randomness (crypto.getRandomValues)
 *
 * Works in Node.js (>=19), browsers, Deno, Bun, and Cloudflare Workers.
 */
export function generateId(): string {
  const timestamp = Date.now();

  if (timestamp > timestampCacheMs) {
    // New millisecond (or first call): update timestamp, seed counter.
    const delta = timestamp - timestampCacheMs;
    if (delta <= BASE) {
      // Fast path: increment the encoded timestamp directly,
      // avoiding 8 Math.trunc/modulo operations.
      timestampCacheMs = timestamp;
      incrementEncodedTimestamp(delta);
    } else {
      // Large jump or first call: full re-encode.
      encodeTimestamp(timestamp);
    }
    seedCounter();
  } else {
    // Same millisecond (or clock went backward): increment counter tail.
    const nxt = SUCCESSOR_CC[counterTailCharCode];
    if (nxt) {
      counterTailCharCode = nxt;
    } else {
      incrementCarry();
    }
  }

  // Ensure random buffer has enough chars for the tail.
  while (randomCharPosition + RANDOM_CHAR_COUNT > randomCharCount) {
    refillRandom();
  }
  const pos = randomCharPosition;
  randomCharPosition = pos + RANDOM_CHAR_COUNT;

  // Build the 21-char ID as a 2-part concat: cached 13-char prefix + 8-char suffix.
  // The suffix is built via String.fromCharCode to produce a flat string directly.
  return (
    prefixPlusCounterHead +
    String.fromCharCode(
      counterTailCharCode,
      randomCharCodes[pos],
      randomCharCodes[pos + 1],
      randomCharCodes[pos + 2],
      randomCharCodes[pos + 3],
      randomCharCodes[pos + 4],
      randomCharCodes[pos + 5],
      randomCharCodes[pos + 6],
    )
  );
}

/**
 * Extract the embedded timestamp from a sparkid as a `Date`.
 *
 * Decodes the first 8 Base58 characters back to the original millisecond
 * timestamp and returns it as a `Date` object.
 *
 * @param id - A 21-character Base58 sparkid string.
 * @returns The `Date` corresponding to the embedded timestamp.
 * @throws {TypeError} If `id` is not a valid 21-char Base58 string.
 *
 * @example
 * ```ts
 * const id = generateId();
 * const ts = extractTimestamp(id);
 * console.log(ts.toISOString());
 * ```
 */
export function extractTimestamp(id: string): Date {
  if (typeof id !== "string") {
    throw new TypeError("extractTimestamp: expected a string argument");
  }
  if (id.length !== ID_LENGTH) {
    throw new TypeError(
      `extractTimestamp: expected a ${ID_LENGTH}-character string, got ${id.length}`,
    );
  }
  let ms = 0;
  for (let i = 0; i < TIMESTAMP_CHAR_COUNT; i++) {
    const idx = BASE58_INDEX[id.charCodeAt(i)];
    if (idx === -1) {
      throw new TypeError(
        `extractTimestamp: invalid Base58 character '${id[i]}' at position ${i}`,
      );
    }
    ms = ms * BASE + idx;
  }
  for (let i = TIMESTAMP_CHAR_COUNT; i < ID_LENGTH; i++) {
    const idx = BASE58_INDEX[id.charCodeAt(i)];
    if (idx === -1) {
      throw new TypeError(
        `extractTimestamp: invalid Base58 character '${id[i]}' at position ${i}`,
      );
    }
  }
  return new Date(ms);
}


