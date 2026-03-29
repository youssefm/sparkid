/// <reference types="node" />
import { generateId } from "../src/index";

const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const compareMode = process.argv.includes("--compare");
const ITERATIONS = compareMode ? 100_000 : 1_000_000;
const WARMUP = compareMode ? 1_000 : 10_000;
const TRIALS = 5;

// --- Correctness checks ---

function verify(): void {
  const valid = new Set(ALPHABET);
  const ids = new Set<string>();

  for (let i = 0; i < 50_000; i++) {
    const id = generateId();

    if (id.length !== 22) {
      throw new Error(`wrong length: ${id} (${id.length})`);
    }
    for (const char of id) {
      if (!valid.has(char)) {
        throw new Error(`invalid char in: ${id}`);
      }
    }
    if (ids.has(id)) {
      throw new Error(`duplicate: ${id}`);
    }
    ids.add(id);
  }

  console.log("correctness: 50,000 IDs valid, unique, correct charset");

  // Monotonicity (burst within same millisecond)
  let prev = generateId();
  for (let i = 1; i < 10_000; i++) {
    const curr = generateId();
    if (curr <= prev) {
      throw new Error(`not monotonic at ${i}: ${prev} >= ${curr}`);
    }
    prev = curr;
  }
  console.log("correctness: monotonicity OK (10,000 burst IDs)");

  // Sortability (across milliseconds)
  const batchA = Array.from({ length: 10 }, () => generateId());
  setTimeout(() => {
    const batchB = Array.from({ length: 10 }, () => generateId());
    const maxA = batchA.sort()[batchA.length - 1];
    const minB = batchB.sort()[0];
    if (maxA >= minB) {
      throw new Error(`sort order broken: ${maxA} >= ${minB}`);
    }
    console.log("correctness: sortability OK");
    console.log();
    if (compareMode) {
      runComparison();
    } else {
      benchmark();
    }
  }, 20);
}

// --- Core benchmarking ---

function benchCandidate(
  generate: () => string,
  verbose = false,
): { medianUs: number; medianThroughput: number } {
  for (let i = 0; i < WARMUP; i++) {
    generate();
  }

  const results: number[] = [];

  for (let trial = 0; trial < TRIALS; trial++) {
    const start = process.hrtime.bigint();
    for (let i = 0; i < ITERATIONS; i++) {
      generate();
    }
    const elapsedNs = Number(process.hrtime.bigint() - start);

    const perCallUs = elapsedNs / ITERATIONS / 1_000;
    results.push(perCallUs);

    if (verbose) {
      const throughput = Math.round(ITERATIONS / (elapsedNs / 1_000_000_000));
      console.log(
        `  trial ${trial + 1}: ${perCallUs.toFixed(3)} µs/call  ${throughput.toLocaleString()} ids/sec`,
      );
    }
  }

  results.sort((a, b) => a - b);
  const medianUs = results[Math.floor(results.length / 2)];
  return { medianUs, medianThroughput: Math.round(1_000_000 / medianUs) };
}

// --- Single-generator benchmark ---

function benchmark(): void {
  console.log(`warmup:      ${WARMUP.toLocaleString()} calls`);
  console.log(`iterations:  ${ITERATIONS.toLocaleString()} per trial`);
  console.log(`trials:      ${TRIALS}`);
  console.log();

  const { medianUs, medianThroughput } = benchCandidate(generateId, true);

  console.log();
  console.log(
    `  median: ${medianUs.toFixed(3)} µs/call  ${medianThroughput.toLocaleString()} ids/sec`,
  );
  console.log();

  console.log("sample IDs:");
  for (let i = 0; i < 5; i++) {
    console.log(`  ${generateId()}`);
  }
}

// --- Comparison benchmark ---

interface Candidate {
  name: string;
  generate: () => string;
  length: string;
  sortable: string;
  format: string;
}

async function runComparison(): Promise<void> {
  console.log("=== ID Generator Comparison ===");
  console.log();
  console.log(`warmup:      ${WARMUP.toLocaleString()} calls`);
  console.log(`iterations:  ${ITERATIONS.toLocaleString()} per trial`);
  console.log(`trials:      ${TRIALS}`);
  console.log();

  const candidates: Candidate[] = [
    {
      name: "sparkid",
      generate: generateId,
      length: "22",
      sortable: "yes",
      format: "Base58",
    },
  ];

  // uuid v4 (crypto.randomUUID — built-in, always available)
  candidates.push({
    name: "uuid v4",
    generate: () => crypto.randomUUID(),
    length: "36",
    sortable: "no",
    format: "hex+dashes",
  });

  // uuid v7 (uuid package — no stdlib equivalent)
  try {
    const { v7 } = await import("uuid");
    candidates.push({
      name: "uuid v7",
      generate: v7,
      length: "36",
      sortable: "yes",
      format: "hex+dashes",
    });
  } catch {
    console.log("  ⚠ uuid not installed, skipping uuid v7 (npm i uuid)");
  }

  // nanoid
  try {
    const { nanoid } = await import("nanoid");
    candidates.push({
      name: "nanoid",
      generate: nanoid,
      length: "21",
      sortable: "no",
      format: "URL-safe",
    });
  } catch {
    console.log("  ⚠ nanoid not installed, skipping (npm i nanoid)");
  }

  // ulid (monotonic — fair comparison since sparkid is monotonic)
  try {
    const { monotonicFactory } = await import("ulid");
    const ulidMonotonic = monotonicFactory();
    candidates.push({
      name: "ulid",
      generate: ulidMonotonic,
      length: "26",
      sortable: "yes",
      format: "Crockford Base32",
    });
  } catch {
    console.log("  ⚠ ulid not installed, skipping (npm i ulid)");
  }

  // Bench each candidate
  const rows: {
    name: string;
    medianUs: number;
    medianThroughput: number;
    sample: string;
    length: string;
    sortable: string;
    format: string;
  }[] = [];

  for (const c of candidates) {
    process.stdout.write(`  benchmarking ${c.name}...`);
    const { medianUs, medianThroughput } = benchCandidate(c.generate);
    console.log(` ${medianThroughput.toLocaleString()} ids/sec`);
    rows.push({
      name: c.name,
      medianUs,
      medianThroughput,
      sample: c.generate(),
      length: c.length,
      sortable: c.sortable,
      format: c.format,
    });
  }

  // Sort by throughput descending
  rows.sort((a, b) => b.medianThroughput - a.medianThroughput);

  // Print results table
  const nameW = Math.max(10, ...rows.map((r) => r.name.length));
  const usW = 10;
  const tpW = 14;
  const lenW = 6;
  const sortW = 8;
  const fmtW = Math.max(6, ...rows.map((r) => r.format.length));

  const header = [
    "Generator".padEnd(nameW),
    "µs/call".padStart(usW),
    "ids/sec".padStart(tpW),
    "Len".padStart(lenW),
    "Sortable".padStart(sortW),
    "Format".padEnd(fmtW),
    "Sample",
  ].join("  ");

  const separator = "-".repeat(header.length + 30);

  console.log();
  console.log(header);
  console.log(separator);

  for (const r of rows) {
    console.log(
      [
        r.name.padEnd(nameW),
        r.medianUs.toFixed(3).padStart(usW),
        r.medianThroughput.toLocaleString().padStart(tpW),
        r.length.padStart(lenW),
        r.sortable.padStart(sortW),
        r.format.padEnd(fmtW),
        r.sample,
      ].join("  "),
    );
  }

  console.log();
}

verify();
