// Comprehensive tests for the sparkid ID generator.
// Uses Node.js built-in test runner (node:test) — zero dependencies.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { generateId, generateIdAt, extractTimestamp } from "../src/index.ts";
import { MAX_TIMESTAMP } from "../src/constants.ts";
import { toBytes, fromBytes } from "../src/binary.ts";

const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE = ALPHABET.length; // 58
const VALID_CHARS = new Set(ALPHABET);

// ---------------------------------------------------------------------------
// Boundary-value tests for generateIdAt (must run first — before state advances)
// ---------------------------------------------------------------------------

describe("generateIdAt boundary values", () => {
  it("epoch zero (new Date(0)) succeeds and round-trips", () => {
    const id = generateIdAt(new Date(0));
    assert.equal(id.length, 21);
    const extracted = extractTimestamp(id);
    assert.equal(extracted.getTime(), 0);
  });
});

// ---------------------------------------------------------------------------
// Helper: decode a Base58-encoded timestamp prefix back to a number
// ---------------------------------------------------------------------------
function decodeTimestamp(encoded: string): number {
  let val = 0;
  for (const ch of encoded) {
    val = val * BASE + ALPHABET.indexOf(ch);
  }
  return val;
}

// ---------------------------------------------------------------------------
// ID format
// ---------------------------------------------------------------------------

describe("ID format", () => {
  it("is always 21 characters", () => {
    for (let i = 0; i < 1000; i++) {
      assert.equal(generateId().length, 21);
    }
  });

  it("uses only Base58 characters (no 0, O, I, l)", () => {
    const forbidden = new Set("0OIl");
    for (let i = 0; i < 1000; i++) {
      const id = generateId();
      for (const ch of id) {
        assert.ok(VALID_CHARS.has(ch), `invalid char '${ch}' in ${id}`);
        assert.ok(!forbidden.has(ch), `ambiguous char '${ch}' in ${id}`);
      }
    }
  });

  it("uses only alphanumeric URL-safe characters", () => {
    for (let i = 0; i < 1000; i++) {
      const id = generateId();
      assert.ok(/^[a-zA-Z0-9]+$/.test(id), `non-alphanumeric chars in ${id}`);
    }
  });

  it("has 8-char timestamp, 6-char counter, 7-char random", () => {
    const id = generateId();
    const ts = id.slice(0, 8);
    const counter = id.slice(8, 14);
    const random = id.slice(14);
    assert.equal(ts.length, 8);
    assert.equal(counter.length, 6);
    assert.equal(random.length, 7);
    for (const part of [ts, counter, random]) {
      for (const ch of part) {
        assert.ok(VALID_CHARS.has(ch));
      }
    }
  });
});

// ---------------------------------------------------------------------------
// Timestamp encoding (verified through generated IDs)
// ---------------------------------------------------------------------------

describe("Timestamp encoding", () => {
  it("timestamp prefix encodes the current millisecond", () => {
    const before = Date.now();
    const id = generateId();
    const after = Date.now();
    const decoded = decodeTimestamp(id.slice(0, 8));
    assert.ok(
      decoded >= before && decoded <= after,
      `decoded timestamp ${decoded} not in [${before}, ${after}]`,
    );
  });

  it("timestamp prefix increases across millisecond boundaries", async () => {
    const idA = generateId();
    await new Promise((resolve) => setTimeout(resolve, 30));
    const idB = generateId();
    assert.ok(idB.slice(0, 8) > idA.slice(0, 8));
  });

  it("timestamp prefix is lexicographically sortable (matches numeric order)", () => {
    // Generate IDs across several milliseconds and verify prefix ordering
    const prefixes: string[] = [];
    const decoded: number[] = [];
    for (let round = 0; round < 20; round++) {
      const id = generateId();
      prefixes.push(id.slice(0, 8));
      decoded.push(decodeTimestamp(id.slice(0, 8)));
      // Busy-wait to cross a millisecond boundary occasionally
      const start = Date.now();
      while (Date.now() === start) {
        /* spin */
      }
    }
    // Lexicographic order must match numeric order
    for (let i = 1; i < prefixes.length; i++) {
      if (decoded[i] > decoded[i - 1]) {
        assert.ok(
          prefixes[i] > prefixes[i - 1],
          `prefix order doesn't match numeric order at ${i}`,
        );
      }
    }
  });

  it("timestamp prefix round-trips through decode", () => {
    const ts = Date.now();
    // Generate an ID and verify the prefix decodes back to a plausible timestamp
    const id = generateId();
    const decoded = decodeTimestamp(id.slice(0, 8));
    // Should be within a small window of the current time
    assert.ok(Math.abs(decoded - ts) < 100);
  });
});

// ---------------------------------------------------------------------------
// Counter monotonicity
// ---------------------------------------------------------------------------

describe("Counter monotonicity", () => {
  it("burst of 50,000 IDs is strictly increasing", () => {
    const ids: string[] = [];
    for (let i = 0; i < 50_000; i++) {
      ids.push(generateId());
    }
    for (let i = 1; i < ids.length; i++) {
      assert.ok(ids[i] > ids[i - 1], `not monotonic at ${i}`);
    }
  });

  it("counter portion increments within the same millisecond", () => {
    // Generate a large burst and pick the timestamp that appears most often,
    // so the test passes even on slow CI runners where the first ms boundary
    // is crossed very quickly.
    const ids: string[] = [];
    for (let i = 0; i < 10_000; i++) {
      ids.push(generateId());
    }
    const groups = new Map<string, string[]>();
    for (const id of ids) {
      const ts = id.slice(0, 8);
      let arr = groups.get(ts);
      if (!arr) {
        arr = [];
        groups.set(ts, arr);
      }
      arr.push(id);
    }
    let sameTs: string[] = [];
    for (const arr of groups.values()) {
      if (arr.length > sameTs.length) sameTs = arr;
    }
    assert.ok(sameTs.length > 1, "need multiple IDs in same ms for this test");
    for (let i = 1; i < sameTs.length; i++) {
      assert.ok(
        sameTs[i].slice(8, 14) > sameTs[i - 1].slice(8, 14),
        `counter not increasing at ${i}`,
      );
    }
  });

  it("sort order matches generation order", () => {
    const ids: string[] = [];
    for (let i = 0; i < 10_000; i++) {
      ids.push(generateId());
    }
    const sorted = [...ids].sort();
    assert.deepEqual(ids, sorted);
  });

  it("counter is randomly seeded each millisecond (different starting values)", async () => {
    const seeds: string[] = [];
    for (let round = 0; round < 10; round++) {
      const id = generateId();
      seeds.push(id.slice(8, 14));
      // Force a new millisecond
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    // With random seeding, it's astronomically unlikely all 10 are identical
    const unique = new Set(seeds);
    assert.ok(unique.size > 1, "counter seeds should vary across milliseconds");
  });
});

// ---------------------------------------------------------------------------
// Counter carry propagation
// ---------------------------------------------------------------------------

describe("Counter carry", () => {
  it("maintains monotonicity when counter tail wraps", () => {
    // A burst of 200 IDs within one ms will cycle the tail through
    // multiple carry events (tail has 58 values before carrying)
    const ids: string[] = [];
    for (let i = 0; i < 200; i++) {
      ids.push(generateId());
    }
    for (let i = 1; i < ids.length; i++) {
      assert.ok(ids[i] > ids[i - 1], `not monotonic at ${i}`);
    }
  });

  it("counter string increases across carry (observable in same-ms burst)", () => {
    const ids: string[] = [];
    for (let i = 0; i < 5000; i++) {
      ids.push(generateId());
    }
    const ts = ids[0].slice(0, 8);
    const sameTs = ids.filter((id) => id.slice(0, 8) === ts);
    // With 5000 IDs in same ms, the tail wraps ~86 times (5000/58)
    // Every counter value must still strictly increase
    for (let i = 1; i < sameTs.length; i++) {
      assert.ok(sameTs[i].slice(8, 14) > sameTs[i - 1].slice(8, 14));
    }
  });
});

// ---------------------------------------------------------------------------
// Uniqueness
// ---------------------------------------------------------------------------

describe("Uniqueness", () => {
  it("100,000 IDs have no duplicates", () => {
    const ids = new Set<string>();
    for (let i = 0; i < 100_000; i++) {
      ids.add(generateId());
    }
    assert.equal(ids.size, 100_000);
  });
});

// ---------------------------------------------------------------------------
// Cross-millisecond sortability
// ---------------------------------------------------------------------------

describe("Cross-millisecond sortability", () => {
  it("later batch sorts entirely after earlier batch", async () => {
    const batchA: string[] = [];
    for (let i = 0; i < 50; i++) {
      batchA.push(generateId());
    }
    await new Promise((resolve) => setTimeout(resolve, 30));
    const batchB: string[] = [];
    for (let i = 0; i < 50; i++) {
      batchB.push(generateId());
    }
    const maxA = batchA.sort()[batchA.length - 1];
    const minB = batchB.sort()[0];
    assert.ok(maxA < minB, `max(A)=${maxA} should be < min(B)=${minB}`);
  });

  it("interleaved generation across ms boundaries stays sorted", async () => {
    const all: string[] = [];
    for (let round = 0; round < 5; round++) {
      for (let i = 0; i < 20; i++) {
        all.push(generateId());
      }
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    const sorted = [...all].sort();
    assert.deepEqual(all, sorted);
  });
});

// ---------------------------------------------------------------------------
// Clock regression (backward clock)
// ---------------------------------------------------------------------------

describe("Clock regression", () => {
  it("monotonicity preserved across rapid generation (covers same-ms path)", () => {
    // Generate IDs as fast as possible — many will share a timestamp,
    // exercising the same-ms counter-increment path. If Date.now() ever
    // returned a stale/earlier value, the code must still produce
    // increasing IDs.
    const ids: string[] = [];
    for (let i = 0; i < 100_000; i++) {
      ids.push(generateId());
    }
    for (let i = 1; i < ids.length; i++) {
      assert.ok(ids[i] > ids[i - 1], `not monotonic at ${i}`);
    }
  });
});

// ---------------------------------------------------------------------------
// Random tail properties
// ---------------------------------------------------------------------------

describe("Random tail", () => {
  it("is uniformly distributed across all 58 chars (chi-squared test)", () => {
    const n = 100_000;
    const counts = new Map<string, number>();
    for (let i = 0; i < n; i++) {
      const id = generateId();
      for (let j = 14; j < 21; j++) {
        const ch = id[j];
        counts.set(ch, (counts.get(ch) || 0) + 1);
      }
    }
    assert.equal(counts.size, 58, "all 58 Base58 chars should appear");
    const total = Array.from(counts.values()).reduce((a, b) => a + b, 0);
    const expected = total / 58;
    let chiSq = 0;
    for (const count of counts.values()) {
      chiSq += (count - expected) ** 2 / expected;
    }
    // With 57 df, chi^2 > 120 would be extreme
    assert.ok(
      chiSq < 120,
      `chi^2=${chiSq.toFixed(1)} suggests non-uniform distribution`,
    );
  });

  it("random tails are distinct within same millisecond", () => {
    const ids: string[] = [];
    for (let i = 0; i < 100; i++) {
      ids.push(generateId());
    }
    const tails = ids.map((id) => id.slice(14));
    assert.equal(new Set(tails).size, tails.length);
  });

  it("no modulo bias — no char appears significantly more than others", () => {
    const n = 200_000;
    const counts = new Map<string, number>();
    for (let i = 0; i < n; i++) {
      const id = generateId();
      for (let j = 14; j < 21; j++) {
        counts.set(id[j], (counts.get(id[j]) || 0) + 1);
      }
    }
    const values = Array.from(counts.values());
    const min = Math.min(...values);
    const max = Math.max(...values);
    // With uniform distribution, max/min ratio should be very close to 1
    assert.ok(
      max / min < 1.05,
      `max/min ratio ${(max / min).toFixed(4)} suggests bias`,
    );
  });
});

// ---------------------------------------------------------------------------
// Stress test
// ---------------------------------------------------------------------------

describe("Stress test", () => {
  it("2,000,000 IDs: all valid, unique, monotonic", () => {
    const n = 2_000_000;
    let prev = "";
    const seen = new Set<string>();
    let lengthBugs = 0;
    let monoBugs = 0;
    let dupBugs = 0;

    for (let i = 0; i < n; i++) {
      const id = generateId();
      if (id.length !== 21) lengthBugs++;
      if (id <= prev) monoBugs++;
      if (seen.has(id)) dupBugs++;
      prev = id;
      // Only track uniqueness for first 500k to limit memory
      if (i < 500_000) seen.add(id);
    }

    assert.equal(lengthBugs, 0, "all IDs should be 21 chars");
    assert.equal(monoBugs, 0, "all IDs should be strictly increasing");
    assert.equal(dupBugs, 0, "no duplicates in first 500k");
  });
});

// ---------------------------------------------------------------------------
// Binary encoding (toBytes / fromBytes)
// ---------------------------------------------------------------------------

describe("toBytes / fromBytes", () => {
  it("round-trips: fromBytes(toBytes(id)) === id", () => {
    for (let i = 0; i < 10_000; i++) {
      const id = generateId();
      const bin = toBytes(id);
      assert.equal(fromBytes(bin), id);
    }
  });

  it("produces exactly 16 bytes", () => {
    for (let i = 0; i < 100; i++) {
      assert.equal(toBytes(generateId()).length, 16);
    }
  });

  it("padding bits are always zero", () => {
    for (let i = 0; i < 1000; i++) {
      const bin = toBytes(generateId());
      assert.equal(bin[15] & 0x03, 0, "last 2 bits must be zero padding");
    }
  });

  it("sort order is preserved (binary memcmp matches string comparison)", () => {
    const ids: string[] = [];
    for (let i = 0; i < 1000; i++) {
      ids.push(generateId());
    }
    // Compare all consecutive pairs
    for (let i = 1; i < ids.length; i++) {
      const binA = toBytes(ids[i - 1]);
      const binB = toBytes(ids[i]);
      // Since ids are monotonic, binA < binB (lexicographic byte compare)
      let cmp = 0;
      for (let j = 0; j < 16; j++) {
        if (binA[j] !== binB[j]) {
          cmp = binA[j] < binB[j] ? -1 : 1;
          break;
        }
      }
      assert.equal(cmp, -1, `binary sort mismatch at index ${i}`);
    }
  });

  it("sort order preserved across millisecond boundaries", async () => {
    const batchA: string[] = [];
    for (let i = 0; i < 50; i++) batchA.push(generateId());
    await new Promise((resolve) => setTimeout(resolve, 30));
    const batchB: string[] = [];
    for (let i = 0; i < 50; i++) batchB.push(generateId());

    const maxBinA = toBytes(batchA.sort()[batchA.length - 1]);
    const minBinB = toBytes(batchB.sort()[0]);
    let cmp = 0;
    for (let j = 0; j < 16; j++) {
      if (maxBinA[j] !== minBinB[j]) {
        cmp = maxBinA[j] < minBinB[j] ? -1 : 1;
        break;
      }
    }
    assert.equal(cmp, -1, "binary sort mismatch across ms boundaries");
  });

  it("all 58 alphabet characters encode and decode correctly", () => {
    // Build a synthetic valid ID for each character in position 0
    for (let i = 0; i < 58; i++) {
      const id = ALPHABET[i] + "1".repeat(20);
      const bin = toBytes(id);
      const recovered = fromBytes(bin);
      assert.equal(recovered, id, `round-trip failed for char '${ALPHABET[i]}'`);
    }
  });

  it("deterministic: same input always produces same binary", () => {
    const id = generateId();
    const bin1 = toBytes(id);
    const bin2 = toBytes(id);
    assert.deepEqual(bin1, bin2);
  });
});

// ---------------------------------------------------------------------------
// toBytes validation
// ---------------------------------------------------------------------------

describe("toBytes validation", () => {
  it("throws RangeError for wrong length", () => {
    assert.throws(() => toBytes("abc"), RangeError);
    assert.throws(() => toBytes("1".repeat(20)), RangeError);
    assert.throws(() => toBytes("1".repeat(22)), RangeError);
    assert.throws(() => toBytes(""), RangeError);
  });

  it("throws RangeError for invalid characters", () => {
    assert.throws(() => toBytes("0" + "1".repeat(20)), RangeError);
    assert.throws(() => toBytes("O" + "1".repeat(20)), RangeError);
    assert.throws(() => toBytes("I" + "1".repeat(20)), RangeError);
    assert.throws(() => toBytes("l" + "1".repeat(20)), RangeError);
    assert.throws(() => toBytes("!" + "1".repeat(20)), RangeError);
    assert.throws(() => toBytes(" " + "1".repeat(20)), RangeError);
  });
});

// ---------------------------------------------------------------------------
// fromBytes validation
// ---------------------------------------------------------------------------

describe("fromBytes validation", () => {
  it("throws RangeError for wrong length", () => {
    assert.throws(() => fromBytes(new Uint8Array(15)), RangeError);
    assert.throws(() => fromBytes(new Uint8Array(17)), RangeError);
    assert.throws(() => fromBytes(new Uint8Array(0)), RangeError);
  });

  it("throws RangeError for out-of-range 6-bit indices", () => {
    // Set first 6-bit field to 63 (>= 58)
    const bad = new Uint8Array(16);
    bad[0] = 0xff; // first index = 63
    assert.throws(() => fromBytes(bad), RangeError);
  });

  it("throws RangeError for non-zero padding", () => {
    // Valid ID packed, then corrupt padding bits
    const id = "1".repeat(21);
    const bin = toBytes(id);
    bin[15] = bin[15] | 0x01; // set lowest bit
    assert.throws(() => fromBytes(bin), RangeError);
  });
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

describe("Public API", () => {
  it("generateId returns a string", () => {
    assert.equal(typeof generateId(), "string");
  });

  it("generateId, generateIdAt, and extractTimestamp are the named exports", async () => {
    const mod = await import("../src/index.ts");
    const exports = Object.keys(mod);
    assert.deepEqual(exports.sort(), [
      "extractTimestamp",
      "generateId",
      "generateIdAt",
    ]);
  });

  it("binary module exports toBytes and fromBytes", async () => {
    const mod = await import("../src/binary.ts");
    const exports = Object.keys(mod);
    assert.deepEqual(exports.sort(), ["fromBytes", "toBytes"]);
  });
});

// ---------------------------------------------------------------------------
// extractTimestamp
// ---------------------------------------------------------------------------

describe("extractTimestamp", () => {
  it("round-trips: extracted timestamp is within ±5ms of Date.now()", () => {
    const before = Date.now();
    const id = generateId();
    const after = Date.now();
    const ts = extractTimestamp(id);
    assert.ok(
      ts.getTime() >= before - 5 && ts.getTime() <= after + 5,
      `extracted ${ts.getTime()} not in [${before - 5}, ${after + 5}]`,
    );
  });

  it("decodes a known timestamp correctly", () => {
    // Encode 1700000000000 into first 8 chars, pad rest with valid Base58
    const knownMs = 1700000000000;
    let encoded = "";
    let val = knownMs;
    const chars: string[] = [];
    for (let i = 0; i < 8; i++) {
      chars.push(ALPHABET[val % BASE]);
      val = Math.trunc(val / BASE);
    }
    encoded = chars.reverse().join("");
    const id = encoded + "1".repeat(13); // pad counter + random with '1's
    const ts = extractTimestamp(id);
    assert.equal(ts.getTime(), knownMs);
  });

  it("throws TypeError for wrong length", () => {
    assert.throws(() => extractTimestamp("abc"), TypeError);
    assert.throws(() => extractTimestamp("1".repeat(20)), TypeError);
    assert.throws(() => extractTimestamp("1".repeat(22)), TypeError);
  });

  it("throws TypeError for invalid characters", () => {
    assert.throws(() => extractTimestamp("0" + "1".repeat(20)), TypeError);
    assert.throws(() => extractTimestamp("O" + "1".repeat(20)), TypeError);
    assert.throws(() => extractTimestamp("I" + "1".repeat(20)), TypeError);
    assert.throws(() => extractTimestamp("l" + "1".repeat(20)), TypeError);
    assert.throws(() => extractTimestamp("~" + "1".repeat(20)), TypeError);
    assert.throws(() => extractTimestamp("{" + "1".repeat(20)), TypeError);
    assert.throws(() => extractTimestamp("1".repeat(20) + "|"), TypeError);
    assert.throws(() => extractTimestamp("€" + "1".repeat(20)), TypeError);
  });

  it("throws TypeError for non-string input", () => {
    assert.throws(() => extractTimestamp(123 as any), TypeError);
    assert.throws(() => extractTimestamp(null as any), TypeError);
    assert.throws(() => extractTimestamp(undefined as any), TypeError);
  });
});

// ---------------------------------------------------------------------------
// generateIdAt
// ---------------------------------------------------------------------------

describe("generateIdAt", () => {
  it("produces valid 21-char Base58 IDs", () => {
    const ts = new Date();
    for (let i = 0; i < 100; i++) {
      const id = generateIdAt(ts);
      assert.equal(id.length, 21);
      for (const ch of id) {
        assert.ok(VALID_CHARS.has(ch), `invalid char '${ch}' in ${id}`);
      }
    }
  });

  it("encodes the provided timestamp correctly", () => {
    const knownMs = 2100000000000; // far future, guaranteed to advance state
    const ts = new Date(knownMs);
    const id = generateIdAt(ts);
    const decoded = decodeTimestamp(id.slice(0, 8));
    assert.equal(decoded, knownMs);
  });

  it("monotonicity within same ms (multiple IDs with same Date)", () => {
    const ts = new Date(2300000000000); // far future, advances state
    const ids: string[] = [];
    for (let i = 0; i < 100; i++) {
      ids.push(generateIdAt(ts));
    }
    for (let i = 1; i < ids.length; i++) {
      assert.ok(ids[i] > ids[i - 1], `not monotonic at ${i}`);
    }
  });

  it("monotonicity across increasing timestamps", () => {
    const ids: string[] = [];
    const baseMs = 2400000000000; // far future
    for (let i = 0; i < 50; i++) {
      ids.push(generateIdAt(new Date(baseMs + i * 10)));
    }
    for (let i = 1; i < ids.length; i++) {
      assert.ok(ids[i] > ids[i - 1], `not monotonic at ${i}`);
    }
  });

  it("clock regression: ID still increases when timestamp goes backward", () => {
    const t1 = new Date(2600000000000); // advance state
    const t0 = new Date(2500000000000); // earlier than t1
    const idA = generateIdAt(t1);
    const idB = generateIdAt(t0); // should still be > idA
    assert.ok(idB > idA, `expected ${idB} > ${idA} despite backward timestamp`);
  });

  it("Date input works correctly", () => {
    const knownMs = 2700000000000; // far future, guaranteed to advance state
    const d = new Date(knownMs);
    const id = generateIdAt(d);
    const extracted = extractTimestamp(id);
    assert.equal(extracted.getTime(), knownMs);
  });

  it("throws TypeError for non-Date input", () => {
    assert.throws(() => generateIdAt(123 as any), TypeError);
    assert.throws(() => generateIdAt("2024-01-01" as any), TypeError);
    assert.throws(() => generateIdAt(null as any), TypeError);
    assert.throws(() => generateIdAt(undefined as any), TypeError);
    assert.throws(() => generateIdAt({} as any), TypeError);
  });

  it("throws RangeError for invalid Date (NaN)", () => {
    assert.throws(() => generateIdAt(new Date(NaN)), RangeError);
    assert.throws(() => generateIdAt(new Date("garbage")), RangeError);
  });

  it("throws RangeError for timestamp > MAX_TIMESTAMP", () => {
    assert.throws(
      () => generateIdAt(new Date(MAX_TIMESTAMP + 1)),
      RangeError,
    );
  });

  it("treats negative timestamp as clock regression", () => {
    // A negative timestamp is in the past relative to any prior call,
    // so the generator treats it as clock regression and increments the counter.
    const before = generateIdAt(new Date(1_000_000));
    const result = generateIdAt(new Date(-1));
    assert.strictEqual(result.length, 21);
    assert.ok(result > before, "monotonicity preserved despite negative timestamp");
  });

  it("interaction: generateIdAt then generateId maintains monotonicity", () => {
    // Use a recent timestamp so generateId (which uses Date.now()) sees same-ms or later
    const recent = new Date();
    const idA = generateIdAt(recent);
    const idB = generateId();
    assert.ok(idB > idA, `expected generateId() > generateIdAt(): ${idB} > ${idA}`);
  });

  it("interaction: generateId then generateIdAt maintains monotonicity", () => {
    const idA = generateId();
    // Use same-ms timestamp — counter should increment
    const now = new Date();
    const idB = generateIdAt(now);
    assert.ok(idB > idA, `expected generateIdAt() > generateId(): ${idB} > ${idA}`);
  });
});

// ---------------------------------------------------------------------------
// generateIdAt MAX_TIMESTAMP boundary (advances state — must be near end)
// ---------------------------------------------------------------------------

describe("generateIdAt MAX_TIMESTAMP boundary", () => {
  it("MAX_TIMESTAMP succeeds and round-trips", () => {
    const id = generateIdAt(new Date(MAX_TIMESTAMP));
    assert.equal(id.length, 21);
    const extracted = extractTimestamp(id);
    assert.equal(extracted.getTime(), MAX_TIMESTAMP);
  });
});
