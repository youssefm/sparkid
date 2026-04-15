use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sparkid::{IdGenerator, SparkId};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

const ALPHABET: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE: usize = 58;

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn decode_timestamp(encoded: &str) -> u64 {
    let mut val: u64 = 0;
    for ch in encoded.bytes() {
        let idx = ALPHABET.bytes().position(|b| b == ch).unwrap();
        val = val * BASE as u64 + idx as u64;
    }
    val
}

/// Run correctness checks before benchmarking.
fn verify() {
    let valid_chars: HashSet<char> = ALPHABET.chars().collect();
    let mut gen = IdGenerator::new();

    // 50,000 IDs: valid length, charset, uniqueness
    let mut ids: Vec<SparkId> = Vec::with_capacity(50_000);
    for _ in 0..50_000 {
        let id = gen.next_id();
        let s = id.as_str();
        assert_eq!(s.len(), 21, "wrong length");
        for ch in s.chars() {
            assert!(valid_chars.contains(&ch), "invalid char: {ch}");
        }
        ids.push(id);
    }
    let unique: HashSet<SparkId> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "duplicates found");

    // Monotonicity: 10,000 burst IDs
    let mut gen2 = IdGenerator::new();
    let burst: Vec<SparkId> = (0..10_000).map(|_| gen2.next_id()).collect();
    for i in 1..burst.len() {
        assert!(burst[i] > burst[i - 1], "not monotonic at {i}");
    }

    // Sortability: batches across milliseconds
    let mut gen3 = IdGenerator::new();
    let batch_a: Vec<SparkId> = (0..50).map(|_| gen3.next_id()).collect();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let batch_b: Vec<SparkId> = (0..50).map(|_| gen3.next_id()).collect();
    let max_a = batch_a.iter().max().unwrap();
    let min_b = batch_b.iter().min().unwrap();
    assert!(max_a < min_b, "batches not sorted across ms");

    // Timestamp accuracy
    let before = current_time_ms();
    let id = gen3.next_id();
    let after = current_time_ms();
    let decoded = decode_timestamp(&id.as_str()[..8]);
    assert!(
        before <= decoded && decoded <= after,
        "timestamp out of range"
    );

    println!("Correctness verification passed.");
}

fn bench_generator(c: &mut Criterion) {
    verify();

    let mut gen = IdGenerator::new();
    for _ in 0..10_000 {
        let _ = gen.next_id();
    }

    c.bench_function("sparkid::IdGenerator::next_id", |b| {
        b.iter(|| gen.next_id().as_str())
    });
}

fn bench_thread_local(c: &mut Criterion) {
    for _ in 0..10_000 {
        let _ = SparkId::new();
    }
    c.bench_function("sparkid::SparkId::new", |b| {
        b.iter(|| SparkId::new().as_str())
    });
}

fn bench_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("id_generators");

    // All benchmarks include string materialisation for apples-to-apples comparison.
    // sparkid: as_str() returns stack-allocated SparkIdStr (zero heap alloc)
    // uuid: encode_lower() writes into a stack buffer (zero heap alloc)
    // nanoid/ulid: no zero-alloc API, so .to_string() / nanoid!() is their best

    let mut sparkid_gen = IdGenerator::new();
    group.bench_function("sparkid", |b| {
        b.iter(|| sparkid_gen.next_id().as_str())
    });

    group.bench_function("uuid_v4", |b| {
        b.iter(|| {
            let mut buf = [0u8; uuid::fmt::Hyphenated::LENGTH];
            uuid::Uuid::new_v4().as_hyphenated().encode_lower(&mut buf);
            buf
        })
    });

    group.bench_function("uuid_v7", |b| {
        b.iter(|| {
            let mut buf = [0u8; uuid::fmt::Hyphenated::LENGTH];
            uuid::Uuid::now_v7().as_hyphenated().encode_lower(&mut buf);
            buf
        })
    });

    group.bench_function("nanoid", |b| b.iter(|| nanoid::nanoid!()));

    // ulid (monotonic — fair comparison since sparkid is monotonic)
    // No zero-alloc string API; .to_string() is the best available
    let mut ulid_gen = ulid::Generator::new();
    group.bench_function("ulid", |b| {
        b.iter(|| ulid_gen.generate().unwrap().to_string())
    });

    group.finish();

    // Print sample IDs
    let mut gen = IdGenerator::new();
    println!("\nSample IDs:");
    println!("  sparkid: {}", gen.next_id());
    println!("  uuid_v4: {}", uuid::Uuid::new_v4());
    println!("  uuid_v7: {}", uuid::Uuid::now_v7());
    println!("  nanoid:  {}", nanoid::nanoid!());
    println!("  ulid:    {}", ulid::Ulid::new());
}

/// Simulate realistic webserver traffic patterns using `next_id_at()`.
///
/// Generates a repeating timestamp sequence where:
/// - Some milliseconds have multiple IDs (bursts)
/// - Some milliseconds are skipped (idle gaps)
/// - Delta between consecutive timestamps varies: 0 (same ms), 1, 2-5, 10+
///
/// The pattern repeats every ~100 IDs covering ~30ms of simulated time,
/// producing a mix of same-ms increments (counter path) and ms-change
/// increments (timestamp encode path).
fn build_webserver_timestamps(base: u64, count: usize) -> Vec<u64> {
    // Pattern: (delta_ms, ids_at_this_timestamp)
    // Simulates ~30ms of traffic with bursts and gaps.
    let pattern: &[(u64, usize)] = &[
        (0, 3),  // 3 IDs at base+0 (burst)
        (1, 2),  // 2 IDs at base+1
        (1, 1),  // 1 ID  at base+2
        (3, 2),  // skip 2ms, 2 IDs at base+5
        (1, 1),  // 1 ID  at base+6
        (1, 4),  // 4 IDs at base+7 (burst)
        (2, 1),  // skip 1ms, 1 ID at base+9
        (1, 1),  // 1 ID  at base+10
        (5, 3),  // skip 4ms, 3 IDs at base+15
        (1, 1),  // 1 ID  at base+16
        (1, 2),  // 2 IDs at base+17
        (10, 1), // skip 9ms, 1 ID at base+27 (big gap)
        (1, 5),  // 5 IDs at base+28 (big burst)
        (1, 1),  // 1 ID  at base+29
    ];

    let pattern_duration_ms: u64 = pattern.iter().map(|(d, _)| *d).sum();

    let mut timestamps = Vec::with_capacity(count);
    let mut current_ms = base;
    'outer: loop {
        for &(delta, ids) in pattern {
            current_ms += delta;
            for _ in 0..ids {
                timestamps.push(current_ms);
                if timestamps.len() >= count {
                    break 'outer;
                }
            }
        }
        // Small gap between pattern repetitions
        current_ms += pattern_duration_ms;
    }
    timestamps
}

fn bench_webserver(c: &mut Criterion) {
    let mut group = c.benchmark_group("webserver_simulation");
    let base_timestamp: u64 = 1_700_000_000_000;
    let warmup_count = 10_000;
    let bench_count = 100_000;

    // Pre-build timestamp sequences (warmup + bench iterations share the pattern)
    let timestamps = build_webserver_timestamps(base_timestamp, warmup_count + bench_count);

    group.bench_function("realistic_traffic", |b| {
        b.iter_custom(|iterations| {
            let mut total = std::time::Duration::ZERO;
            for _ in 0..iterations {
                let mut gen = IdGenerator::new();
                // Warmup: prime the generator
                for &timestamp in &timestamps[..warmup_count] {
                    let _ = gen.next_id_at(timestamp);
                }

                let start = std::time::Instant::now();
                for &timestamp in &timestamps[warmup_count..] {
                    black_box(gen.next_id_at(black_box(timestamp)));
                }
                total += start.elapsed();
            }
            total
        });
    });

    group.finish();
}

fn bench_parse(c: &mut Criterion) {
    let mut gen = IdGenerator::new();
    let id = gen.next_id();
    let id_string = id.to_string();

    c.bench_function("sparkid::SparkId::from_str", |b| {
        b.iter(|| id_string.parse::<SparkId>().unwrap())
    });
}

fn bench_from_u128(c: &mut Criterion) {
    let mut gen = IdGenerator::new();
    let id = gen.next_id();
    let packed = id.as_u128();

    c.bench_function("sparkid::SparkId::from_u128", |b| {
        b.iter(|| SparkId::from_u128(black_box(packed)).unwrap())
    });
}

fn bench_from_bytes(c: &mut Criterion) {
    let mut gen = IdGenerator::new();
    let id = gen.next_id();
    let bytes = id.to_bytes();

    c.bench_function("sparkid::SparkId::from_bytes", |b| {
        b.iter(|| SparkId::from_bytes(black_box(bytes)).unwrap())
    });
}

criterion_group!(benches, bench_generator, bench_thread_local, bench_webserver, bench_parse, bench_from_u128, bench_from_bytes, bench_comparison);
criterion_main!(benches);
