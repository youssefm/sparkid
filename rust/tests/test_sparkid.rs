use sparkid::{IdGenerator, ParseSparkIdError, SparkId};

const MAX_TIMESTAMP: u64 = 128_063_081_718_015;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const ALPHABET: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE: usize = 58;

fn valid_chars() -> HashSet<char> {
    ALPHABET.chars().collect()
}

fn decode_timestamp(encoded: &str) -> u64 {
    let mut val: u64 = 0;
    for ch in encoded.bytes() {
        let idx = ALPHABET.bytes().position(|b| b == ch).unwrap();
        val = val * BASE as u64 + idx as u64;
    }
    val
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// ID format
// ---------------------------------------------------------------------------

#[test]
fn test_length() {
    let mut gen = IdGenerator::new();
    for _ in 0..1000 {
        assert_eq!(gen.next_id().as_str().len(), 21);
    }
}

#[test]
fn test_charset() {
    let valid = valid_chars();
    let mut gen = IdGenerator::new();
    for _ in 0..1000 {
        let id = gen.next_id();
        for ch in id.as_str().chars() {
            assert!(valid.contains(&ch), "invalid char: {ch}");
        }
    }
}

#[test]
fn test_no_ambiguous_chars() {
    let forbidden: HashSet<char> = "0OIl".chars().collect();
    let mut gen = IdGenerator::new();
    for _ in 0..1000 {
        let id = gen.next_id();
        for ch in id.as_str().chars() {
            assert!(!forbidden.contains(&ch), "ambiguous char: {ch}");
        }
    }
}

#[test]
fn test_all_alphanumeric_url_safe() {
    let mut gen = IdGenerator::new();
    for _ in 0..1000 {
        let id = gen.next_id();
        assert!(
            id.as_str().chars().all(|c| c.is_ascii_alphanumeric()),
            "non-alphanumeric: {id}"
        );
    }
}

#[test]
fn test_structure_parts() {
    let valid = valid_chars();
    let mut gen = IdGenerator::new();
    let id = gen.next_id();
    let s = id.as_str();
    let ts = &s[..8];
    let counter = &s[8..14];
    let random_tail = &s[14..];
    assert_eq!(ts.len(), 8);
    assert_eq!(counter.len(), 6);
    assert_eq!(random_tail.len(), 7);
    for ch in ts.chars().chain(counter.chars()).chain(random_tail.chars()) {
        assert!(valid.contains(&ch));
    }
}

// ---------------------------------------------------------------------------
// Timestamp encoding
// ---------------------------------------------------------------------------

#[test]
fn test_prefix_encodes_current_millisecond() {
    let before = current_time_ms();
    let id = SparkId::new();
    let after = current_time_ms();
    let decoded = decode_timestamp(&id.as_str()[..8]);
    assert!(
        before <= decoded && decoded <= after,
        "decoded={decoded} not in [{before}, {after}]"
    );
}

#[test]
fn test_prefix_round_trips_through_decode() {
    let id = SparkId::new();
    let decoded = decode_timestamp(&id.as_str()[..8]);
    let ts_now = current_time_ms();
    assert!(
        (decoded as i64 - ts_now as i64).unsigned_abs() < 100,
        "decoded={decoded} too far from now={ts_now}"
    );
}

#[test]
fn test_prefix_increases_across_milliseconds() {
    let mut gen = IdGenerator::new();
    let id_a = gen.next_id();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let id_b = gen.next_id();
    assert!(id_b.as_str()[..8] > id_a.as_str()[..8]);
}

#[test]
fn test_prefix_is_lexicographically_sortable() {
    let mut gen = IdGenerator::new();
    let mut prefixes = Vec::new();
    for _ in 0..20 {
        let id = gen.next_id();
        prefixes.push(id.as_str()[..8].to_string());
        // busy-wait to cross ms boundary
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < 1 {}
    }
    for i in 1..prefixes.len() {
        let ts_prev = decode_timestamp(&prefixes[i - 1]);
        let ts_curr = decode_timestamp(&prefixes[i]);
        if ts_curr > ts_prev {
            assert!(prefixes[i] > prefixes[i - 1]);
        }
    }
}

#[test]
fn test_encode_boundary_values() {
    let mut gen = IdGenerator::new();

    let id = gen.next_id_at(0);
    assert_eq!(id.timestamp_ms(), 0);

    let id = gen.next_id_at(1);
    assert_eq!(id.timestamp_ms(), 1);

    let id = gen.next_id_at(57);
    assert_eq!(id.timestamp_ms(), 57);

    let id = gen.next_id_at(58);
    assert_eq!(id.timestamp_ms(), 58);
}

#[test]
fn test_encode_monotonic_over_range() {
    let mut gen = IdGenerator::new();
    let ts = current_time_ms();
    let mut prev_ms = 0u64;
    for offset in 0..10_000u64 {
        let t = ts + offset;
        let id = gen.next_id_at(t);
        let decoded = id.timestamp_ms();
        assert_eq!(decoded, t, "Round-trip failed at offset {offset}");
        assert!(decoded >= prev_ms, "Not monotonic at offset {offset}");
        prev_ms = decoded;
    }
}

#[test]
fn test_encode_round_trip() {
    let mut gen = IdGenerator::new();
    let ts = current_time_ms();
    let id = gen.next_id_at(ts);
    assert_eq!(id.timestamp_ms(), ts);
}

#[test]
fn test_encode_digit_boundaries() {
    let mut gen = IdGenerator::new();
    for power in 1..8 {
        let boundary = 58u64.pow(power);

        let before = gen.next_id_at(boundary - 1);
        let at = gen.next_id_at(boundary);
        let after = gen.next_id_at(boundary + 1);

        assert_eq!(before.timestamp_ms(), boundary - 1);
        assert_eq!(at.timestamp_ms(), boundary);
        assert_eq!(after.timestamp_ms(), boundary + 1);
        assert!(before < at, "boundary {boundary}: before >= at");
        assert!(at < after, "boundary {boundary}: at >= after");
    }
}

#[test]
fn test_encode_preserves_counter_head() {
    let mut gen = IdGenerator::new();
    gen.set_counter_head(&[33, 34, 35, 36, 37]); // a b c d e
    gen.encode_timestamp_test(12345);
    assert_eq!(gen.counter_head_indices(), &[33, 34, 35, 36, 37]);
}

// ---------------------------------------------------------------------------
// Incremental timestamp encoding (delta ≤ 58 fast path)
// ---------------------------------------------------------------------------

#[test]
fn test_increment_various_deltas() {
    // Deltas 1 through 58 should all produce correct timestamps via the fast path.
    let base: u64 = 1_700_000_000_000;
    let mut gen = IdGenerator::new();
    let _ = gen.next_id_at(base); // prime

    for delta in 1..=58u64 {
        let ts = base + delta;
        let id = gen.next_id_at(ts);
        assert_eq!(id.timestamp_ms(), ts, "failed for delta {delta}");
    }
}

#[test]
fn test_increment_delta_at_headroom_boundary() {
    // Start at a timestamp where field 7 = 50 (headroom = 7).
    // delta=7 should fit without carry, delta=8 should carry.
    let mut gen = IdGenerator::new();
    let base = 50u64; // field 7 = 50, headroom = 57 - 50 = 7
    let _ = gen.next_id_at(base);

    // delta=7: no carry, field 7 becomes 57
    let id = gen.next_id_at(base + 7);
    assert_eq!(id.timestamp_ms(), 57);

    // delta=1 more: carry, field 7 wraps to 0, field 6 increments
    let id = gen.next_id_at(base + 8);
    assert_eq!(id.timestamp_ms(), 58);
}

#[test]
fn test_increment_large_delta_with_carry() {
    // delta=30 from a position near the wrap point triggers carry ~52% of the time.
    // Test a sweep of starting positions to cover both carry and no-carry.
    let mut gen = IdGenerator::new();
    for start_offset in 0..58u64 {
        let base = 1_000_000u64 + start_offset;
        let _ = gen.next_id_at(base);
        let ts = base + 30;
        let id = gen.next_id_at(ts);
        assert_eq!(id.timestamp_ms(), ts, "failed at start_offset {start_offset}");
    }
}

#[test]
fn test_increment_multi_level_carry() {
    // Position where fields 7 and 6 are both 57, so carry propagates two levels.
    // 58*57 + 57 = 3363 → fields: [0,0,0,0,0,0,57,57]
    let mut gen = IdGenerator::new();
    let base = 58u64 * 57 + 57; // 3363
    let _ = gen.next_id_at(base);

    let id = gen.next_id_at(base + 1); // 3364 = 58*58 = 58^2
    assert_eq!(id.timestamp_ms(), 3364);
    // Verify field structure: [0,0,0,0,0,1,0,0]
    assert_eq!(&id.as_str()[..8], "11111211");
}

#[test]
fn test_increment_deep_carry_chain() {
    // Position where fields 7, 6, 5, and 4 are all 57.
    // 58^4 - 1 = 11_316_495 → fields: [0,0,0,0,57,57,57,57]
    let mut gen = IdGenerator::new();
    let base = 58u64.pow(4) - 1;
    let _ = gen.next_id_at(base);

    let id = gen.next_id_at(base + 1); // = 58^4
    assert_eq!(id.timestamp_ms(), 58u64.pow(4));
    assert_eq!(&id.as_str()[..8], "11121111");
}

#[test]
fn test_increment_delta_58_with_carry() {
    // Maximum delta (58) starting from field 7 = 0 should carry.
    // 0 + 58 = 58 → remainder = 0, carry into field 6.
    let mut gen = IdGenerator::new();
    let base = 58u64 * 10; // field 7 = 0, field 6 = 10
    let _ = gen.next_id_at(base);

    let id = gen.next_id_at(base + 58);
    assert_eq!(id.timestamp_ms(), base + 58);
}

#[test]
fn test_increment_vs_full_encode_equivalence() {
    // Generate IDs with varying deltas and verify they match what a fresh
    // full-encode generator would produce (same timestamp_ms round-trip).
    let base: u64 = 1_700_000_000_000;
    let deltas = [1, 2, 5, 10, 20, 30, 45, 57, 58, 1, 3, 58, 58, 1];
    let mut gen = IdGenerator::new();
    let _ = gen.next_id_at(base);

    let mut ts = base;
    for &delta in &deltas {
        ts += delta;
        let id = gen.next_id_at(ts);
        assert_eq!(id.timestamp_ms(), ts, "mismatch at ts={ts}, delta={delta}");
    }
}

#[test]
fn test_encode_timestamp_zero() {
    let mut gen = IdGenerator::new();
    let id = gen.next_id_at(0);
    assert_eq!(id.timestamp_ms(), 0);
}

#[test]
fn test_encode_timestamp_max() {
    let mut gen = IdGenerator::new();
    let id = gen.next_id_at(MAX_TIMESTAMP);
    assert_eq!(id.timestamp_ms(), MAX_TIMESTAMP);
}

#[test]
#[should_panic(expected = "Timestamp out of range")]
fn test_encode_timestamp_above_max_panics() {
    let mut gen = IdGenerator::new();
    gen.encode_timestamp_test(MAX_TIMESTAMP + 1);
}

#[test]
#[should_panic(expected = "Timestamp out of range")]
fn test_counter_overflow_at_max_timestamp_panics() {
    let mut gen = IdGenerator::new();
    gen.set_timestamp_cache_ms(MAX_TIMESTAMP);
    gen.encode_timestamp_test(MAX_TIMESTAMP);
    gen.set_counter_head(&[57, 57, 57, 57, 57]); // zzzzz
    gen.set_counter_tail(57); // z

    gen.increment_carry_test();
}

// ---------------------------------------------------------------------------
// Counter monotonicity
// ---------------------------------------------------------------------------

#[test]
fn test_burst_strictly_increasing() {
    let mut gen = IdGenerator::new();
    let mut prev = gen.next_id();
    for i in 1..50_000 {
        let id = gen.next_id();
        assert!(id > prev, "Not monotonic at {i}");
        prev = id;
    }
}

#[test]
fn test_counter_increments_within_same_ms() {
    static FIXED_TS: AtomicU64 = AtomicU64::new(0);
    fn fixed_time() -> u64 { FIXED_TS.load(Ordering::SeqCst) }

    let mut gen = IdGenerator::new();
    // Pin time to a fixed millisecond to guarantee all IDs share the same timestamp.
    FIXED_TS.store(current_time_ms() + 100_000, Ordering::SeqCst);
    gen.set_time_function(fixed_time);

    let ids: Vec<SparkId> = (0..500).map(|_| gen.next_id()).collect();
    let ts0 = ids[0].as_str()[..8].to_string();
    assert!(ids.iter().all(|id| id.as_str()[..8] == ts0), "all IDs should share timestamp");
    for i in 1..ids.len() {
        assert!(ids[i].as_str()[8..14] > ids[i - 1].as_str()[8..14]);
    }
}

#[test]
fn test_sort_order_matches_generation_order() {
    let mut gen = IdGenerator::new();
    let ids: Vec<SparkId> = (0..10_000).map(|_| gen.next_id()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn test_counter_randomly_seeded_each_ms() {
    let mut gen = IdGenerator::new();
    let mut seeds = Vec::new();
    for _ in 0..10 {
        let id = gen.next_id();
        seeds.push(id.as_str()[8..14].to_string());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let unique: HashSet<&String> = seeds.iter().collect();
    assert!(unique.len() > 1, "counter seeds should vary across ms");
}

// ---------------------------------------------------------------------------
// Counter carry
// ---------------------------------------------------------------------------

#[test]
fn test_single_carry() {
    let mut gen = IdGenerator::new();
    gen.set_counter_head(&[9, 9, 9, 9, 9]); // AAAAA
    gen.set_counter_tail(57); // z

    gen.increment_carry_test();

    assert_eq!(gen.counter_tail(), 0); // '1' = index 0
    assert_eq!(gen.counter_head_indices(), &[9, 9, 9, 9, 10]); // AAAAB
}

#[test]
fn test_cascading_carry() {
    let mut gen = IdGenerator::new();
    gen.set_counter_head(&[9, 9, 57, 57, 57]); // AAzzz
    gen.set_counter_tail(57); // z

    gen.increment_carry_test();

    assert_eq!(gen.counter_tail(), 0); // '1' = index 0
    assert_eq!(gen.counter_head_indices(), &[9, 10, 0, 0, 0]); // AB111
}

#[test]
fn test_full_overflow_bumps_timestamp() {
    let mut gen = IdGenerator::new();
    let ts = current_time_ms();
    gen.set_timestamp_cache_ms(ts);
    gen.encode_timestamp_test(ts);
    gen.set_counter_head(&[57, 57, 57, 57, 57]); // zzzzz
    gen.set_counter_tail(57); // z

    gen.increment_carry_test();

    assert_eq!(gen.timestamp_cache_ms(), ts + 1);
}

#[test]
fn test_full_overflow_produces_valid_id() {
    let valid = valid_chars();
    let mut gen = IdGenerator::new();
    let ts = current_time_ms();
    gen.set_timestamp_cache_ms(ts);
    gen.encode_timestamp_test(ts);
    gen.seed_counter_test();

    // Force counter to max
    gen.set_counter_head(&[57, 57, 57, 57, 57]); // zzzzz
    gen.set_counter_tail(57); // z

    gen.increment_carry_test();

    let id = gen.next_id();
    assert_eq!(id.as_str().len(), 21);
    for ch in id.as_str().chars() {
        assert!(valid.contains(&ch));
    }
}

#[test]
fn test_carry_maintains_monotonicity() {
    let mut gen = IdGenerator::new();
    gen.set_counter_tail(56); // y = index 56, one before z
    let ids: Vec<SparkId> = (0..5).map(|_| gen.next_id()).collect();
    for i in 1..ids.len() {
        assert!(ids[i] > ids[i - 1]);
    }
}

#[test]
fn test_carry_observable_in_burst() {
    let mut gen = IdGenerator::new();
    let ids: Vec<SparkId> = (0..200).map(|_| gen.next_id()).collect();
    let ts = ids[0].as_str()[..8].to_string();
    let same_ts: Vec<&SparkId> = ids.iter().filter(|id| id.as_str()[..8] == ts).collect();
    for i in 1..same_ts.len() {
        assert!(same_ts[i].as_str()[8..14] > same_ts[i - 1].as_str()[8..14]);
    }
}

// ---------------------------------------------------------------------------
// Uniqueness
// ---------------------------------------------------------------------------

#[test]
fn test_no_duplicates_single_generator() {
    let mut gen = IdGenerator::new();
    let ids: HashSet<SparkId> = (0..100_000).map(|_| gen.next_id()).collect();
    assert_eq!(ids.len(), 100_000);
}

#[test]
fn test_no_duplicates_across_generators() {
    let mut gen1 = IdGenerator::new();
    let mut gen2 = IdGenerator::new();
    let ids1: HashSet<SparkId> = (0..10_000).map(|_| gen1.next_id()).collect();
    let ids2: HashSet<SparkId> = (0..10_000).map(|_| gen2.next_id()).collect();
    let intersection: HashSet<_> = ids1.intersection(&ids2).collect();
    assert!(intersection.is_empty());
}

#[test]
fn test_no_duplicates_sparkid_new() {
    let ids: HashSet<SparkId> = (0..50_000).map(|_| SparkId::new()).collect();
    assert_eq!(ids.len(), 50_000);
}

// ---------------------------------------------------------------------------
// Cross-millisecond sortability
// ---------------------------------------------------------------------------

#[test]
fn test_later_batch_sorts_after() {
    let mut gen = IdGenerator::new();
    let batch_a: Vec<SparkId> = (0..50).map(|_| gen.next_id()).collect();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let batch_b: Vec<SparkId> = (0..50).map(|_| gen.next_id()).collect();
    let max_a = batch_a.iter().max().unwrap();
    let min_b = batch_b.iter().min().unwrap();
    assert!(max_a < min_b);
}

#[test]
fn test_interleaved_generation_stays_sorted() {
    let mut gen = IdGenerator::new();
    let mut all_ids = Vec::new();
    for _ in 0..5 {
        for _ in 0..20 {
            all_ids.push(gen.next_id());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut sorted = all_ids.clone();
    sorted.sort();
    assert_eq!(all_ids, sorted);
}

// ---------------------------------------------------------------------------
// Clock regression (backward clock)
// ---------------------------------------------------------------------------

static MOCK_TS: AtomicU64 = AtomicU64::new(0);

fn mock_time() -> u64 {
    MOCK_TS.load(Ordering::SeqCst)
}

#[test]
fn test_monotonic_despite_backward_clock() {
    let mut gen = IdGenerator::new();
    let real_ts = current_time_ms();

    MOCK_TS.store(real_ts, Ordering::SeqCst);
    gen.set_time_function(mock_time);
    let id1 = gen.next_id();

    // Clock goes backward by 100ms
    MOCK_TS.store(real_ts - 100, Ordering::SeqCst);
    let id2 = gen.next_id();

    assert!(
        id2 > id1,
        "Monotonicity must be preserved when clock goes backward"
    );
    assert_eq!(&id2.as_str()[..8], &id1.as_str()[..8]); // Timestamp unchanged, counter incremented
}

#[test]
fn test_clock_catches_up_after_regression() {
    let mut gen = IdGenerator::new();
    let real_ts = current_time_ms() + 10000; // offset to avoid collision with other test

    MOCK_TS.store(real_ts, Ordering::SeqCst);
    gen.set_time_function(mock_time);
    let id1 = gen.next_id();

    // Clock goes backward
    MOCK_TS.store(real_ts - 50, Ordering::SeqCst);
    let id2 = gen.next_id();

    // Clock catches up past original
    MOCK_TS.store(real_ts + 50, Ordering::SeqCst);
    let id3 = gen.next_id();

    assert!(id1 < id2);
    assert!(id2 < id3);
    assert!(id3.as_str()[..8] > id1.as_str()[..8]); // New timestamp prefix
}

// ---------------------------------------------------------------------------
// Random tail properties
// ---------------------------------------------------------------------------

#[test]
fn test_uniform_char_distribution() {
    let mut gen = IdGenerator::new();
    let n = 100_000;
    let mut counts: HashMap<char, usize> = HashMap::new();
    for _ in 0..n {
        let id = gen.next_id();
        for ch in id.as_str()[14..].chars() {
            *counts.entry(ch).or_insert(0) += 1;
        }
    }
    let total: usize = counts.values().sum();
    let expected = total as f64 / 58.0;
    let chi_sq: f64 = counts
        .values()
        .map(|&c| {
            let diff = c as f64 - expected;
            diff * diff / expected
        })
        .sum();
    assert!(
        chi_sq < 120.0,
        "Random tail appears non-uniform (chi^2={chi_sq:.1})"
    );
}

#[test]
fn test_all_58_chars_appear() {
    let valid = valid_chars();
    let mut gen = IdGenerator::new();
    let mut seen: HashSet<char> = HashSet::new();
    for _ in 0..50_000 {
        let id = gen.next_id();
        for ch in id.as_str()[14..].chars() {
            seen.insert(ch);
        }
    }
    assert_eq!(seen, valid);
}

#[test]
fn test_random_tails_differ() {
    let mut gen = IdGenerator::new();
    let ids: Vec<SparkId> = (0..100).map(|_| gen.next_id()).collect();
    let tails: HashSet<String> = ids.iter().map(|id| id.as_str()[14..].to_string()).collect();
    assert_eq!(tails.len(), ids.len());
}

#[test]
fn test_no_modulo_bias() {
    let mut gen = IdGenerator::new();
    let mut counts: HashMap<char, usize> = HashMap::new();
    for _ in 0..200_000 {
        let id = gen.next_id();
        for ch in id.as_str()[14..].chars() {
            *counts.entry(ch).or_insert(0) += 1;
        }
    }
    let max_count = *counts.values().max().unwrap();
    let min_count = *counts.values().min().unwrap();
    let ratio = max_count as f64 / min_count as f64;
    assert!(
        ratio < 1.05,
        "max/min ratio {ratio:.4} suggests bias"
    );
}

// ---------------------------------------------------------------------------
// Rejection sampling internals
// ---------------------------------------------------------------------------

#[test]
fn test_refill_produces_valid_chars() {
    let mut gen = IdGenerator::new();
    gen.refill_random_test();
    for &index in gen.random_buffer_valid() {
        assert!(index < 58, "invalid index in random buf: {index}");
    }
}

#[test]
fn test_expected_yield() {
    let mut gen = IdGenerator::new();
    gen.refill_random_test();
    let ratio = gen.random_count() as f64 / 16384.0;
    assert!(
        (0.80..0.98).contains(&ratio),
        "unexpected yield ratio: {ratio}"
    );
}

// ---------------------------------------------------------------------------
// Thread safety
// ---------------------------------------------------------------------------

#[test]
fn test_sparkid_new_per_thread_isolation() {
    let mut handles = Vec::new();
    for _ in 0..4 {
        handles.push(std::thread::spawn(|| {
            let ids: Vec<SparkId> = (0..5000).map(|_| SparkId::new()).collect();
            ids
        }));
    }

    let mut all_ids: Vec<SparkId> = Vec::new();
    for handle in handles {
        let ids = handle.join().unwrap();
        // Per-thread monotonicity
        for i in 1..ids.len() {
            assert!(ids[i] > ids[i - 1], "Thread not monotonic at {i}");
        }
        all_ids.extend(ids);
    }

    // Global uniqueness
    let unique: HashSet<&SparkId> = all_ids.iter().collect();
    assert_eq!(unique.len(), all_ids.len());
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[test]
fn test_sparkid_new_returns_valid() {
    let valid = valid_chars();
    let id = SparkId::new();
    assert_eq!(id.as_str().len(), 21);
    for ch in id.as_str().chars() {
        assert!(valid.contains(&ch));
    }
}

#[test]
fn test_id_generator_next_id() {
    let mut gen = IdGenerator::new();
    let id = gen.next_id();
    assert_eq!(id.as_str().len(), 21);
}

#[test]
fn test_multiple_generators_independent() {
    let mut gen1 = IdGenerator::new();
    let mut gen2 = IdGenerator::new();
    let ids1: Vec<SparkId> = (0..100).map(|_| gen1.next_id()).collect();
    let ids2: Vec<SparkId> = (0..100).map(|_| gen2.next_id()).collect();

    let mut sorted1 = ids1.clone();
    sorted1.sort();
    assert_eq!(ids1, sorted1);

    let mut sorted2 = ids2.clone();
    sorted2.sort();
    assert_eq!(ids2, sorted2);

    let set1: HashSet<&SparkId> = ids1.iter().collect();
    let set2: HashSet<&SparkId> = ids2.iter().collect();
    let intersection: HashSet<_> = set1.intersection(&set2).collect();
    assert!(intersection.is_empty());
}

// ---------------------------------------------------------------------------
// Stress test
// ---------------------------------------------------------------------------

#[test]
fn test_stress_2m_ids() {
    let valid = valid_chars();
    let mut gen = IdGenerator::new();
    let mut prev = gen.next_id();
    let mut unique_set: HashSet<SparkId> = HashSet::with_capacity(500_000);
    unique_set.insert(prev);

    for i in 1..2_000_000 {
        let id = gen.next_id();
        assert_eq!(id.as_str().len(), 21);
        assert!(id > prev, "Not monotonic at {i}");
        for ch in id.as_str().chars() {
            assert!(valid.contains(&ch));
        }
        if i < 500_000 {
            assert!(unique_set.insert(id), "Duplicate at {i}");
        }
        prev = id;
    }
}

// ---------------------------------------------------------------------------
// SparkId type
// ---------------------------------------------------------------------------

#[test]
fn test_sparkid_new() {
    let id = SparkId::new();
    assert_eq!(id.as_str().len(), 21);
    assert!(id.as_str().chars().all(|c| c.is_ascii_alphanumeric()));
}

#[test]
fn test_sparkid_display_no_alloc() {
    let id = SparkId::new();
    let displayed = format!("{id}");
    assert_eq!(displayed, id.to_string());
    assert_eq!(displayed.len(), 21);
}

#[test]
fn test_sparkid_deref_to_str() {
    let id = SparkId::new();
    let s = id.as_str();
    let slice: &str = &s;
    assert_eq!(slice.len(), 21);
}

#[test]
fn test_sparkid_into_string() {
    let id = SparkId::new();
    let expected = id.to_string();
    let s: String = id.into();
    assert_eq!(s, expected);
}

#[test]
fn test_sparkid_copy() {
    let id = SparkId::new();
    let copy = id;
    assert_eq!(id, copy);
}

#[test]
fn test_sparkid_ord() {
    let mut gen = IdGenerator::new();
    let a = gen.next_id();
    let b = gen.next_id();
    assert!(a < b);
}

#[test]
fn test_sparkid_hash() {
    let mut gen = IdGenerator::new();
    let ids: HashSet<SparkId> = (0..10_000).map(|_| gen.next_id()).collect();
    assert_eq!(ids.len(), 10_000);
}

#[test]
fn test_sparkid_debug() {
    let id = SparkId::new();
    let debug = format!("{id:?}");
    assert!(debug.starts_with("SparkId("));
    assert!(debug.ends_with(')'));
}

#[test]
fn test_next_id_monotonic() {
    let mut gen = IdGenerator::new();
    let mut prev = gen.next_id();
    for i in 0..10_000 {
        let id = gen.next_id();
        assert!(id > prev, "Not monotonic at {i}");
        prev = id;
    }
}

#[test]
fn test_sparkid_new_monotonic_within_thread() {
    let mut prev = SparkId::new();
    for i in 0..1_000 {
        let id = SparkId::new();
        assert!(id > prev, "SparkId::new() not monotonic at {i}");
        prev = id;
    }
}

#[test]
fn test_iterator_yields_sparkid() {
    let mut gen = IdGenerator::new();
    let ids: Vec<SparkId> = gen.by_ref().take(100).collect();
    assert_eq!(ids.len(), 100);
    for i in 1..ids.len() {
        assert!(ids[i] > ids[i - 1]);
    }
}

// ---------------------------------------------------------------------------
// next_id_at (no_std-compatible API)
// ---------------------------------------------------------------------------

#[test]
fn test_next_id_at_valid_format() {
    let valid = valid_chars();
    let mut gen = IdGenerator::new();
    let ts = current_time_ms();
    for i in 0..1000 {
        let id = gen.next_id_at(ts + i);
        assert_eq!(id.as_str().len(), 21);
        for ch in id.as_str().chars() {
            assert!(valid.contains(&ch), "invalid char: {ch}");
        }
    }
}

#[test]
fn test_next_id_at_encodes_timestamp() {
    let mut gen = IdGenerator::new();
    let ts = 1_700_000_000_000u64;
    let id = gen.next_id_at(ts);
    let decoded = decode_timestamp(&id.as_str()[..8]);
    assert_eq!(decoded, ts);
}

#[test]
fn test_next_id_at_monotonic_same_ms() {
    let mut gen = IdGenerator::new();
    let ts = current_time_ms() + 200_000;
    let ids: Vec<SparkId> = (0..10_000).map(|_| gen.next_id_at(ts)).collect();
    for i in 1..ids.len() {
        assert!(ids[i] > ids[i - 1], "not monotonic at {i}");
    }
    // All share the same timestamp prefix
    let prefix = ids[0].as_str()[..8].to_string();
    assert!(ids.iter().all(|id| id.as_str()[..8] == prefix));
}

#[test]
fn test_next_id_at_monotonic_across_ms() {
    let mut gen = IdGenerator::new();
    let base = current_time_ms() + 300_000;
    let mut prev = gen.next_id_at(base);
    for i in 1..1000 {
        let id = gen.next_id_at(base + i);
        assert!(id > prev, "not monotonic at ms offset {i}");
        prev = id;
    }
}

#[test]
fn test_next_id_at_counter_reseeds_each_ms() {
    let mut gen = IdGenerator::new();
    let base = current_time_ms() + 400_000;
    let mut counters = Vec::new();
    for i in 0..20 {
        let id = gen.next_id_at(base + i);
        counters.push(id.as_str()[8..14].to_string());
    }
    let unique: HashSet<&String> = counters.iter().collect();
    assert!(unique.len() > 1, "counter should reseed across ms");
}

#[test]
fn test_next_id_at_handles_clock_regression() {
    let mut gen = IdGenerator::new();
    let ts = current_time_ms() + 500_000;
    let id1 = gen.next_id_at(ts);
    // Clock goes backward
    let id2 = gen.next_id_at(ts - 100);
    assert!(id2 > id1, "must stay monotonic despite backward clock");
    assert_eq!(&id2.as_str()[..8], &id1.as_str()[..8], "timestamp should not go backward");
}

#[test]
fn test_next_id_at_no_duplicates() {
    let mut gen = IdGenerator::new();
    let ts = current_time_ms() + 600_000;
    let ids: HashSet<SparkId> = (0..50_000).map(|i| gen.next_id_at(ts + i / 100)).collect();
    assert_eq!(ids.len(), 50_000);
}

#[test]
fn test_next_id_at_matches_next_id_behavior() {
    // next_id_at with real timestamps should produce IDs indistinguishable
    // from next_id: same length, charset, and timestamp accuracy.
    let valid = valid_chars();
    let mut gen = IdGenerator::new();
    let before = current_time_ms();
    let id = gen.next_id_at(before);
    let after = current_time_ms();
    assert_eq!(id.as_str().len(), 21);
    for ch in id.as_str().chars() {
        assert!(valid.contains(&ch));
    }
    let decoded = decode_timestamp(&id.as_str()[..8]);
    assert!(
        before <= decoded && decoded <= after,
        "decoded={decoded} not in [{before}, {after}]"
    );
}

// ---------------------------------------------------------------------------
// FromStr / TryFrom<&str>
// ---------------------------------------------------------------------------

#[test]
fn test_parse_round_trip() {
    let mut gen = IdGenerator::new();
    for _ in 0..1000 {
        let id = gen.next_id();
        let parsed: SparkId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
    }
}

#[test]
fn test_parse_preserves_value() {
    let id = SparkId::new();
    let s: String = id.into();
    let parsed: SparkId = s.parse().unwrap();
    assert_eq!(&*parsed.as_str(), s.as_str());
}

#[test]
fn test_try_from_str() {
    let id = SparkId::new();
    let s = id.as_str();
    let parsed = SparkId::try_from(&*s).unwrap();
    assert_eq!(id, parsed);
}

#[test]
fn test_parse_wrong_length_short() {
    let result: Result<SparkId, ParseSparkIdError> = "abc".parse();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("expected 21, got 3"), "{err}");
}

#[test]
fn test_parse_wrong_length_long() {
    let result = "1".repeat(23).parse::<SparkId>();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("expected 21, got 23"), "{err}");
}

#[test]
fn test_parse_empty_string() {
    let result = "".parse::<SparkId>();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("expected 21, got 0"), "{err}");
}

#[test]
fn test_parse_invalid_char_zero() {
    // '0' is excluded from Base58
    let input = "0".repeat(21);
    let result = input.parse::<SparkId>();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("'0'"), "{err}");
    assert!(err.to_string().contains("position 0"), "{err}");
}

#[test]
fn test_parse_invalid_char_ambiguous() {
    // 'O', 'I', 'l' are excluded from Base58
    for (ch, pos) in [('O', 5), ('I', 10), ('l', 20)] {
        let mut chars: Vec<char> = "1".repeat(21).chars().collect();
        chars[pos] = ch;
        let input: String = chars.into_iter().collect();
        let result = input.parse::<SparkId>();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains(&format!("'{ch}'"))
                && err.to_string().contains(&format!("position {pos}")),
            "expected error about '{ch}' at position {pos}, got: {err}"
        );
    }
}

#[test]
fn test_parse_invalid_char_special() {
    let mut input = "1".repeat(21);
    // Replace middle char with a non-alphanumeric
    input.replace_range(11..12, "-");
    let result = input.parse::<SparkId>();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("'-'"), "{err}");
    assert!(err.to_string().contains("position 11"), "{err}");
}

#[test]
fn test_parse_reports_first_invalid_char() {
    // Multiple bad chars — should report the first one
    let input = "0OIl".to_string() + &"1".repeat(17);
    let result = input.parse::<SparkId>();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("position 0"), "{err}");
}

#[test]
fn test_parse_all_valid_alphabet_chars() {
    // Build a 21-char string from the alphabet (wrapping)
    let alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let input: String = alphabet.chars().cycle().take(21).collect();
    let result = input.parse::<SparkId>();
    assert!(result.is_ok());
    assert_eq!(&*result.unwrap().as_str(), input.as_str());
}

#[test]
fn test_parsed_id_has_correct_ordering() {
    let mut gen = IdGenerator::new();
    let a = gen.next_id();
    let b = gen.next_id();
    let parsed_a: SparkId = a.to_string().parse().unwrap();
    let parsed_b: SparkId = b.to_string().parse().unwrap();
    assert!(parsed_a < parsed_b);
    assert_eq!(a, parsed_a);
    assert_eq!(b, parsed_b);
}

#[test]
fn test_parse_error_is_debug_and_display() {
    let err = "bad".parse::<SparkId>().unwrap_err();
    // Debug
    let debug = format!("{err:?}");
    assert!(!debug.is_empty());
    // Display
    let display = format!("{err}");
    assert!(display.contains("21"));
}

#[test]
fn test_parse_error_eq() {
    let err1 = "bad".parse::<SparkId>().unwrap_err();
    let err2 = "bad".parse::<SparkId>().unwrap_err();
    assert_eq!(err1, err2);

    let err3 = "also_bad".parse::<SparkId>().unwrap_err();
    // Both wrong length, but different lengths
    assert_ne!(err1, err3);
}

// ---------------------------------------------------------------------------
// Timestamp extraction
// ---------------------------------------------------------------------------

#[test]
fn test_timestamp_ms_round_trip() {
    let known_ms: u64 = 1_700_000_000_000;
    let mut gen = IdGenerator::new();
    let id = gen.next_id_at(known_ms);
    assert_eq!(id.timestamp_ms(), known_ms);
}

#[test]
fn test_timestamp_returns_system_time() {
    let before = SystemTime::now();
    let id = SparkId::new();
    let after = SystemTime::now();
    let ts = id.timestamp();
    assert!(ts >= before - std::time::Duration::from_millis(50));
    assert!(ts <= after + std::time::Duration::from_millis(50));
}

#[test]
fn test_timestamp_ms_known_value() {
    // Manually construct an ID with a known timestamp of 0
    // Timestamp 0 encodes as "11111111" (all first Base58 chars)
    let id_str = "111111111111111111111";
    let id: SparkId = id_str.parse().unwrap();
    assert_eq!(id.timestamp_ms(), 0);

    // Construct ID with timestamp = 58 (should be "11111121" + 13 pad chars)
    let id_str2 = "111111211111111111111";
    let id2: SparkId = id_str2.parse().unwrap();
    assert_eq!(id2.timestamp_ms(), 58);
}

// ---------------------------------------------------------------------------
// Binary representation (u128 / bytes) round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_as_u128_round_trip() {
    let mut gen = IdGenerator::new();
    for _ in 0..100 {
        let id = gen.next_id();
        let value = id.as_u128();
        let restored = SparkId::from_u128(value).unwrap();
        assert_eq!(id, restored);
    }
}

#[test]
fn test_to_bytes_round_trip() {
    let mut gen = IdGenerator::new();
    for _ in 0..100 {
        let id = gen.next_id();
        let bytes = id.to_bytes();
        let restored = SparkId::from_bytes(bytes).unwrap();
        assert_eq!(id, restored);
    }
}

#[test]
fn test_u128_and_bytes_are_consistent() {
    let mut gen = IdGenerator::new();
    let id = gen.next_id();
    let value = id.as_u128();
    let bytes = id.to_bytes();
    assert_eq!(bytes, value.to_be_bytes());
}

#[test]
fn test_from_u128_preserves_string() {
    let mut gen = IdGenerator::new();
    let id = gen.next_id();
    let s = id.as_str().to_string();
    let restored = SparkId::from_u128(id.as_u128()).unwrap();
    assert_eq!(&*restored.as_str(), &s);
}

#[test]
fn test_from_bytes_preserves_string() {
    let mut gen = IdGenerator::new();
    let id = gen.next_id();
    let s = id.as_str().to_string();
    let restored = SparkId::from_bytes(id.to_bytes()).unwrap();
    assert_eq!(&*restored.as_str(), &s);
}

#[test]
fn test_from_u128_rejects_invalid_index() {
    // Set a 6-bit field to 63 (> 57), which is an invalid Base58 index.
    // Bits 127..122 hold the first index; set it to 63 (0x3F).
    let bad_value: u128 = 0x3F << 122;
    let result = SparkId::from_u128(bad_value);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("invalid 6-bit index"));
}

#[test]
fn test_from_bytes_rejects_invalid_padding() {
    let mut gen = IdGenerator::new();
    let id = gen.next_id();
    let mut bytes = id.to_bytes();
    // Set lowest 2 bits (padding) to non-zero
    bytes[15] |= 0x01;
    let result = SparkId::from_bytes(bytes);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("padding"));
}

#[test]
fn test_u128_preserves_ordering() {
    let mut gen = IdGenerator::new();
    let ids: Vec<SparkId> = (0..100).map(|_| gen.next_id()).collect();
    for i in 1..ids.len() {
        assert!(ids[i].as_u128() > ids[i - 1].as_u128());
    }
}

#[test]
fn test_from_u128_known_value() {
    // All-ones ID: "111111111111111111111" → all indices are 0
    let all_zeros: u128 = 0;
    let id = SparkId::from_u128(all_zeros).unwrap();
    assert_eq!(&*id.as_str(), "111111111111111111111");
}

#[test]
fn test_from_u128_boundary_value_57_valid_all_fields() {
    // 57 is the maximum valid index. Set every field to 57.
    let mut value: u128 = 0;
    for i in 0..21 {
        value |= 57u128 << (122 - i * 6);
    }
    assert!(SparkId::from_u128(value).is_ok());
}

#[test]
fn test_from_u128_boundary_value_58_invalid_each_field() {
    // 58 is the minimum invalid index. Test it at every field position.
    for field in 0..21 {
        let shift = 122 - field * 6;
        // Start with all-zeros (valid), set one field to 58
        let value: u128 = 58u128 << shift;
        let result = SparkId::from_u128(value);
        assert!(
            result.is_err(),
            "field {field} with value 58 should be rejected"
        );
    }
}

#[test]
fn test_from_u128_all_invalid_values_58_through_63() {
    for invalid_value in 58..=63u128 {
        // Place the invalid value in the middle field (field 10)
        let shift = 122 - 10 * 6;
        let value = invalid_value << shift;
        let result = SparkId::from_u128(value);
        assert!(
            result.is_err(),
            "value {invalid_value} at field 10 should be rejected"
        );
    }
}

#[test]
fn test_from_u128_multiple_invalid_fields() {
    // Set fields 0, 10, and 20 to invalid values simultaneously
    let mut value: u128 = 0;
    value |= 63u128 << 122; // field 0
    value |= 60u128 << (122 - 10 * 6); // field 10
    value |= 58u128 << (122 - 20 * 6); // field 20
    assert!(SparkId::from_u128(value).is_err());
}

#[test]
fn test_from_u128_max_valid_id() {
    // Every field set to 57 = the largest valid SparkId
    let mut value: u128 = 0;
    for i in 0..21 {
        value |= 57u128 << (122 - i * 6);
    }
    let id = SparkId::from_u128(value).unwrap();
    // 57 maps to 'z' (last char of Base58 alphabet)
    assert_eq!(&*id.as_str(), "zzzzzzzzzzzzzzzzzzzzz");
}

#[test]
fn test_from_bytes_boundary_value_58_invalid() {
    // Same boundary test via from_bytes
    let shift = 122 - 5 * 6; // field 5
    let value: u128 = 58u128 << shift;
    let bytes = value.to_be_bytes();
    assert!(SparkId::from_bytes(bytes).is_err());
}

// ---------------------------------------------------------------------------
// Binary ↔ String alignment
// ---------------------------------------------------------------------------

/// Build a 21-char string where position `pos` has alphabet char at `index`
/// and every other position has '1' (index 0).
fn string_with_single_index(position: usize, index: u8) -> String {
    let alphabet = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut chars = [b'1'; 21];
    chars[position] = alphabet[index as usize];
    String::from_utf8(chars.to_vec()).unwrap()
}

#[test]
fn test_each_position_maps_to_correct_bit_field() {
    // For each of the 21 character positions, place a single non-zero index
    // and verify it lands in exactly the right 6-bit field of the u128.
    for position in 0..21 {
        let index: u8 = 1; // minimal non-zero
        let s = string_with_single_index(position, index);
        let id: SparkId = s.parse().unwrap();

        let expected = if position < 20 {
            // Positions 0..19: index is at bits (122 - pos*6)..(127 - pos*6)
            (index as u128) << (122 - position * 6)
        } else {
            // Position 20: shifted left by 2 for padding
            (index as u128) << 2
        };
        assert_eq!(
            id.as_u128(),
            expected,
            "position {position}: expected {expected:#034x}, got {:#034x}",
            id.as_u128()
        );
    }
}

#[test]
fn test_max_index_at_each_position() {
    // Same as above but with max index (57 → 'z') to test full 6-bit range.
    for position in 0..21 {
        let index: u8 = 57;
        let s = string_with_single_index(position, index);
        let id: SparkId = s.parse().unwrap();

        // All other positions are '1' = index 0, so only the target field
        // contributes to the u128 value.
        let expected = if position < 20 {
            (index as u128) << (122 - position * 6)
        } else {
            (index as u128) << 2
        };
        assert_eq!(
            id.as_u128(),
            expected,
            "position {position} with max index"
        );
    }
}

#[test]
fn test_crafted_strings_match_expected_binary() {
    // Each case: (string, description, expected u128 computed by hand / formula)
    struct Case {
        string: &'static str,
        description: &'static str,
    }

    let cases = [
        Case {
            string: "111111111111111111111",
            description: "all index-0",
        },
        Case {
            string: "zzzzzzzzzzzzzzzzzzzzz",
            description: "all index-57",
        },
        Case {
            string: "211111111111111111111",
            description: "index-1 at position 0 only",
        },
        Case {
            string: "111111111111111111112",
            description: "index-1 at position 20 only",
        },
        Case {
            string: "zzzzzzzz1111111111111",
            description: "max timestamp, min counter+random",
        },
        Case {
            string: "11111111zzzzzz1111111",
            description: "min timestamp, max counter, min random",
        },
        Case {
            string: "11111111111111zzzzzzz",
            description: "min timestamp+counter, max random",
        },
        Case {
            string: "9AHJNPZakm11111111111",
            description: "alphabet gap boundary chars in timestamp",
        },
    ];

    // Independent oracle: compute u128 from string without reusing library code
    let alphabet = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    for case in &cases {
        let bytes = case.string.as_bytes();
        assert_eq!(bytes.len(), 21, "bad test case: {}", case.description);

        // Manually pack: positions 0..19 shift left by 6 each, position 20 shifts left by 8
        let mut expected: u128 = 0;
        for i in 0..20 {
            let idx = alphabet.iter().position(|&c| c == bytes[i]).unwrap() as u128;
            expected = (expected << 6) | idx;
        }
        let last_idx = alphabet.iter().position(|&c| c == bytes[20]).unwrap() as u128;
        expected = (expected << 8) | (last_idx << 2);

        let id: SparkId = case.string.parse().unwrap();

        // Binary matches independent encoding
        assert_eq!(
            id.as_u128(),
            expected,
            "binary mismatch for case: {}",
            case.description
        );

        // String round-trips through binary
        assert_eq!(
            &*id.as_str(),
            case.string,
            "string mismatch for case: {}",
            case.description
        );

        // Bytes are big-endian of u128
        assert_eq!(
            id.to_bytes(),
            expected.to_be_bytes(),
            "bytes mismatch for case: {}",
            case.description
        );
    }
}

#[test]
fn test_sort_order_agrees_across_representations() {
    // Pairs that differ at alphabet discontinuities (gaps in Base58 where
    // the ASCII distance between adjacent-index chars is > 1).
    // Each pair: (lesser, greater, description)
    let pairs: &[(&str, &str, &str)] = &[
        // Differ at position 0: '9' (index 8) vs 'A' (index 9)
        (
            "911111111111111111111",
            "A11111111111111111111",
            "gap: 9 < A at pos 0",
        ),
        // Differ at position 0: 'H' (index 16) vs 'J' (index 17) — skips 'I'
        (
            "H11111111111111111111",
            "J11111111111111111111",
            "gap: H < J at pos 0",
        ),
        // Differ at position 0: 'N' (index 21) vs 'P' (index 22) — skips 'O'
        (
            "N11111111111111111111",
            "P11111111111111111111",
            "gap: N < P at pos 0",
        ),
        // Differ at position 5: 'Z' (index 32) vs 'a' (index 33)
        (
            "11111Z111111111111111",
            "11111a111111111111111",
            "gap: Z < a at pos 5",
        ),
        // Differ at position 10: 'k' (index 43) vs 'm' (index 44) — skips 'l'
        (
            "1111111111k1111111111",
            "1111111111m1111111111",
            "gap: k < m at pos 10",
        ),
        // Differ at last position (20): boundary padding correctness
        (
            "111111111111111111111",
            "111111111111111111112",
            "differ at pos 20 only",
        ),
        // Differ at segment boundary: pos 7 (timestamp end) > pos 8 (counter start)
        (
            "11111111211111111111z",
            "11111112111111111111z",
            "pos 8 '2' < pos 7 '2' (higher position dominates)",
        ),
        // Differ at segment boundary: pos 13 (counter end) > pos 14 (random start)
        (
            "11111111111111211111z",
            "11111111111112111111z",
            "pos 14 '2' < pos 13 '2' (higher position dominates)",
        ),
        // Full extremes
        (
            "111111111111111111111",
            "zzzzzzzzzzzzzzzzzzzzz",
            "all-min vs all-max",
        ),
        // Differ across a wide index gap at position 15
        (
            "111111111111111111111",
            "111111111111111z11111",
            "index 0 vs 57 at pos 15",
        ),
    ];

    for &(lesser_str, greater_str, description) in pairs {
        let lesser: SparkId = lesser_str.parse().unwrap();
        let greater: SparkId = greater_str.parse().unwrap();

        // String order
        assert!(
            lesser_str < greater_str,
            "string order failed: {description}"
        );

        // u128 order
        assert!(
            lesser.as_u128() < greater.as_u128(),
            "u128 order failed: {description}"
        );

        // Byte order
        assert!(
            lesser.to_bytes() < greater.to_bytes(),
            "byte order failed: {description}"
        );

        // SparkId Ord order
        assert!(lesser < greater, "SparkId Ord failed: {description}");
    }
}

#[test]
fn test_byte_group_boundaries() {
    // Verify the 3-byte grouping from the spec:
    // bytes 0-2 = chars 0-3, bytes 3-5 = chars 4-7, etc.
    // Place a known index at the first and last char of each group
    // and check exactly which bytes are affected.
    let group_boundaries: &[(usize, usize)] = &[
        (0, 3),   // group 0: bytes 0-2
        (4, 7),   // group 1: bytes 3-5
        (8, 11),  // group 2: bytes 6-8
        (12, 15), // group 3: bytes 9-11
        (16, 19), // group 4: bytes 12-14
    ];

    for &(first_char, last_char) in group_boundaries {
        let group_index = first_char / 4;
        let byte_start = group_index * 3;
        let byte_end = byte_start + 3; // exclusive

        // Place max index at first_char of group
        let s = string_with_single_index(first_char, 57);
        let id: SparkId = s.parse().unwrap();
        let bytes = id.to_bytes();

        // Non-zero bytes should only be within this group's range
        for (i, &byte) in bytes.iter().enumerate() {
            if i >= byte_start && i < byte_end {
                // may or may not be non-zero depending on bit alignment
                continue;
            }
            if i == 15 {
                // byte 15 holds char 20 + padding; skip in this check
                continue;
            }
            assert_eq!(
                byte, 0,
                "char {first_char} (group {group_index}) leaked into byte {i}"
            );
        }

        // Same for last_char of group
        let s = string_with_single_index(last_char, 57);
        let id: SparkId = s.parse().unwrap();
        let bytes = id.to_bytes();
        for (i, &byte) in bytes.iter().enumerate() {
            if i >= byte_start && i < byte_end {
                continue;
            }
            if i == 15 {
                continue;
            }
            assert_eq!(
                byte, 0,
                "char {last_char} (group {group_index}) leaked into byte {i}"
            );
        }
    }
}

#[test]
fn test_position_20_padding_is_always_zero() {
    // For every valid index at position 20, the bottom 2 bits must be zero.
    for index in 0..58u8 {
        let s = string_with_single_index(20, index);
        let id: SparkId = s.parse().unwrap();
        assert_eq!(
            id.as_u128() & 0x03,
            0,
            "padding non-zero for index {index} at position 20"
        );
    }
}

#[test]
fn test_timestamp_ms_agrees_with_string_decode() {
    // Verify timestamp_ms() returns the same value as manually decoding
    // the first 8 characters of the string representation.
    let mut gen = IdGenerator::new();
    for _ in 0..50 {
        let id = gen.next_id();
        let s = id.as_str();
        let string_timestamp = decode_timestamp(&s[..8]);
        assert_eq!(
            id.timestamp_ms(),
            string_timestamp,
            "timestamp_ms() disagrees with string decode for {s}"
        );
    }
}

/// Build a u128 with a single 6-bit field set to `index` at position `pos`,
/// all other fields zero. This is the binary→string counterpart of
/// `string_with_single_index`.
fn u128_with_single_index(position: usize, index: u8) -> u128 {
    if position < 20 {
        (index as u128) << (122 - position * 6)
    } else {
        (index as u128) << 2
    }
}

#[test]
fn test_binary_to_string_each_position() {
    // Construct u128 values with a known index at each position,
    // convert to string via as_str(), and verify the character independently.
    let alphabet = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    for position in 0..21 {
        for &index in &[0u8, 1, 28, 57] {
            let value = u128_with_single_index(position, index);
            let id = SparkId::from_u128(value).unwrap();
            let s = id.as_str();
            let chars: Vec<u8> = s.bytes().collect();

            // The target position should have the expected alphabet character
            assert_eq!(
                chars[position], alphabet[index as usize],
                "position {position}, index {index}: expected '{}', got '{}'",
                alphabet[index as usize] as char, chars[position] as char
            );

            // All other positions should be '1' (index 0)
            for (i, &ch) in chars.iter().enumerate() {
                if i != position {
                    assert_eq!(
                        ch, b'1',
                        "position {i} should be '1' when only position {position} is set"
                    );
                }
            }
        }
    }
}

#[test]
fn test_binary_to_string_crafted_u128_values() {
    // Construct u128 from explicit indices, verify as_str() produces the
    // expected string. This is a pure binary→string test using an independent
    // oracle (manual index→char mapping).
    let alphabet = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    struct Case {
        indices: [u8; 21],
        description: &'static str,
    }

    let cases = [
        Case {
            indices: [0; 21],
            description: "all zeros → all '1's",
        },
        Case {
            indices: [57; 21],
            description: "all 57s → all 'z's",
        },
        Case {
            // Ascending indices: 0,1,2,...,20
            indices: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
            description: "ascending indices",
        },
        Case {
            // Descending indices: 57,56,...,37
            indices: [
                57, 56, 55, 54, 53, 52, 51, 50, 49, 48, 47, 46, 45, 44, 43, 42, 41, 40, 39, 38,
                37,
            ],
            description: "descending from max",
        },
        Case {
            // Alphabet gap boundary indices: 8(='9'), 9(='A'), 16(='H'), 17(='J'),
            // 21(='N'), 22(='P'), 32(='Z'), 33(='a'), 43(='k'), 44(='m')
            indices: [8, 9, 16, 17, 21, 22, 32, 33, 43, 44, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            description: "alphabet gap boundary indices",
        },
        Case {
            // Max timestamp (57*8), min counter (0*6), max random (57*7)
            indices: [
                57, 57, 57, 57, 57, 57, 57, 57, 0, 0, 0, 0, 0, 0, 57, 57, 57, 57, 57, 57, 57,
            ],
            description: "max timestamp + min counter + max random",
        },
    ];

    for case in &cases {
        // Build expected string from indices
        let expected: String = case
            .indices
            .iter()
            .map(|&idx| alphabet[idx as usize] as char)
            .collect();

        // Build u128 from indices (same packing as the spec)
        let mut value: u128 = 0;
        for i in 0..20 {
            value = (value << 6) | case.indices[i] as u128;
        }
        value = (value << 8) | (case.indices[20] as u128) << 2;

        let id = SparkId::from_u128(value).unwrap();
        let actual = id.as_str();

        assert_eq!(
            &*actual, &expected,
            "binary→string mismatch for case: {}",
            case.description
        );
    }
}

#[test]
fn test_from_bytes_to_string_field_extraction() {
    // Construct 16-byte arrays by hand, convert via from_bytes,
    // and verify the string output matches expected characters.
    let alphabet = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    // All-zero bytes → all index-0 → "111111111111111111111"
    let bytes = [0u8; 16];
    let id = SparkId::from_bytes(bytes).unwrap();
    assert_eq!(&*id.as_str(), "111111111111111111111");

    // Bytes with known pattern: set byte 0 to pack indices for chars 0-1.
    // Byte 0 holds bits 127-120: top 6 bits = char 0, next 2 bits = top 2 of char 1.
    // Place index 9 (='A') at char 0: bits 127-122 = 001001
    // Char 1 stays 0: bits 121-116 = 000000
    // So byte 0 = 0b00100100 = 0x24
    let mut bytes = [0u8; 16];
    bytes[0] = 0x24;
    let id = SparkId::from_bytes(bytes).unwrap();
    let s = id.as_str();
    assert_eq!(
        s.as_bytes()[0],
        alphabet[9],
        "byte 0 = 0x24 should give 'A' at position 0, got '{}'",
        s.as_bytes()[0] as char
    );

    // Place index 57 (='z') at char 20 (last): byte 15 = 57 << 2 = 0xE4
    let mut bytes = [0u8; 16];
    bytes[15] = 57 << 2; // 0xE4
    let id = SparkId::from_bytes(bytes).unwrap();
    let s = id.as_str();
    assert_eq!(
        s.as_bytes()[20],
        alphabet[57],
        "byte 15 = 0xE4 should give 'z' at position 20, got '{}'",
        s.as_bytes()[20] as char
    );
}

// ---------------------------------------------------------------------------
// serde
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
mod serde_tests {
    use sparkid::{IdGenerator, SparkId, SparkIdStr};

    #[test]
    fn test_sparkid_json_roundtrip() {
        let mut generator = IdGenerator::new();
        let id = generator.next_id();
        let json = serde_json::to_string(&id).unwrap();

        // JSON output is a quoted 21-char Base58 string
        let expected_string: String = id.into();
        assert_eq!(json, format!("\"{}\"", expected_string));

        let deserialized: SparkId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn test_sparkid_json_roundtrip_equality() {
        let mut generator = IdGenerator::new();
        for _ in 0..100 {
            let id = generator.next_id();
            let json = serde_json::to_string(&id).unwrap();
            let restored: SparkId = serde_json::from_str(&json).unwrap();
            assert_eq!(id, restored);
        }
    }

    #[test]
    fn test_sparkid_binary_roundtrip() {
        let mut generator = IdGenerator::new();
        for _ in 0..100 {
            let id = generator.next_id();
            let bytes = postcard::to_allocvec(&id).unwrap();
            assert_eq!(bytes.len(), 16, "binary encoding should be exactly 16 bytes");
            assert_eq!(bytes.as_slice(), &id.to_bytes(), "wire bytes should match to_bytes()");
            let restored: SparkId = postcard::from_bytes(&bytes).unwrap();
            assert_eq!(id, restored);
        }
    }

    #[test]
    fn test_sparkid_binary_preserves_sort_order() {
        let mut generator = IdGenerator::new();
        let id_a = generator.next_id();
        let id_b = generator.next_id();
        assert!(id_a < id_b, "IDs should be monotonically increasing");

        let bytes_a = postcard::to_allocvec(&id_a).unwrap();
        let bytes_b = postcard::to_allocvec(&id_b).unwrap();
        assert!(
            bytes_a < bytes_b,
            "serialized bytes should preserve sort order"
        );
    }

    #[test]
    fn test_sparkid_str_json_roundtrip() {
        let mut generator = IdGenerator::new();
        for _ in 0..100 {
            let id = generator.next_id();
            let id_str = id.as_str();
            let json = serde_json::to_string(&id_str).unwrap();
            let restored: SparkIdStr = serde_json::from_str(&json).unwrap();
            assert_eq!(id_str, restored);
        }
    }

    #[test]
    fn test_deserialize_invalid_json_wrong_length() {
        let result = serde_json::from_str::<SparkId>("\"abc\"");
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(
            error_message.contains("length"),
            "error should mention length: {error_message}"
        );
    }

    #[test]
    fn test_deserialize_invalid_json_bad_characters() {
        // 'O' and '0' are not in the Base58 alphabet
        let result = serde_json::from_str::<SparkId>("\"OOOOOOOOOOOOOOOOOOOOO\"");
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(
            error_message.contains("invalid character"),
            "error should mention invalid character: {error_message}"
        );
    }

    #[test]
    fn test_deserialize_invalid_binary_wrong_byte_count() {
        // Fixed-size array encoding: fewer than 16 bytes should fail
        let short_bytes: &[u8] = &[0xDE, 0xAD, 0xBE, 0xEF];
        let result = postcard::from_bytes::<SparkId>(short_bytes);
        assert!(result.is_err());
    }
}
