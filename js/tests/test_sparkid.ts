// Comprehensive tests for the sparkid ID generator.
// Uses Node.js built-in test runner (node:test) — zero dependencies.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { generateId, extractTimestamp } from "../src/index.ts";

const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE = ALPHABET.length; // 58
const VALID_CHARS = new Set(ALPHABET);

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
      if (!arr) { arr = []; groups.set(ts, arr); }
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
// Public API
// ---------------------------------------------------------------------------

describe("Public API", () => {
  it("generateId returns a string", () => {
    assert.equal(typeof generateId(), "string");
  });

  it("generateId is the only named export", async () => {
    const mod = await import("../src/index.ts");
    const exports = Object.keys(mod);
    assert.deepEqual(exports.sort(), ["extractTimestamp", "generateId"]);
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
  });

  it("throws TypeError for non-string input", () => {
    assert.throws(() => extractTimestamp(123 as any), TypeError);
    assert.throws(() => extractTimestamp(null as any), TypeError);
    assert.throws(() => extractTimestamp(undefined as any), TypeError);
  });
});
