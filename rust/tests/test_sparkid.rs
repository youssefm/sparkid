use sparkid::{IdGenerator, ParseSparkIdError, SparkId};
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

    gen.encode_timestamp_test(0);
    assert_eq!(&gen.prefix_plus_counter_head()[..8], &[0, 0, 0, 0, 0, 0, 0, 0]);

    gen.encode_timestamp_test(1);
    assert_eq!(&gen.prefix_plus_counter_head()[..8], &[0, 0, 0, 0, 0, 0, 0, 1]);

    gen.encode_timestamp_test(57);
    assert_eq!(&gen.prefix_plus_counter_head()[..8], &[0, 0, 0, 0, 0, 0, 0, 57]);

    gen.encode_timestamp_test(58);
    assert_eq!(&gen.prefix_plus_counter_head()[..8], &[0, 0, 0, 0, 0, 0, 1, 0]);
}

#[test]
fn test_encode_monotonic_over_range() {
    let mut gen = IdGenerator::new();
    let ts = current_time_ms();
    let mut prev: Vec<u8> = Vec::new();
    for offset in 0..10_000u64 {
        gen.encode_timestamp_test(ts + offset);
        let encoded: Vec<u8> = gen.prefix_plus_counter_head()[..8].to_vec();
        assert!(encoded > prev, "Not monotonic at offset {offset}");
        prev = encoded;
    }
}

#[test]
fn test_encode_round_trip() {
    let mut gen = IdGenerator::new();
    let ts = current_time_ms();
    gen.encode_timestamp_test(ts);
    let prefix_indices = &gen.prefix_plus_counter_head()[..8];
    let mut val: u64 = 0;
    for &idx in prefix_indices {
        val = val * 58 + idx as u64;
    }
    assert_eq!(val, ts);
}

#[test]
fn test_encode_digit_boundaries() {
    let mut gen = IdGenerator::new();
    for power in 1..8 {
        let boundary = 58u64.pow(power);

        gen.encode_timestamp_test(boundary - 1);
        let before: Vec<u8> = gen.prefix_plus_counter_head()[..8].to_vec();
        gen.encode_timestamp_test(boundary);
        let at: Vec<u8> = gen.prefix_plus_counter_head()[..8].to_vec();
        gen.encode_timestamp_test(boundary + 1);
        let after: Vec<u8> = gen.prefix_plus_counter_head()[..8].to_vec();

        assert!(before < at, "boundary {boundary}: before >= at");
        assert!(at < after, "boundary {boundary}: at >= after");
    }
}

#[test]
fn test_encode_preserves_counter_head() {
    let mut gen = IdGenerator::new();
    gen.set_counter_head(&[33, 34, 35, 36, 37]); // a b c d e
    gen.encode_timestamp_test(12345);
    assert_eq!(&gen.prefix_plus_counter_head()[8..], &[33, 34, 35, 36, 37]);
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
    assert_eq!(&gen.prefix_plus_counter_head()[8..], &[9, 9, 9, 9, 10]); // AAAAB
}

#[test]
fn test_cascading_carry() {
    let mut gen = IdGenerator::new();
    gen.set_counter_head(&[9, 9, 57, 57, 57]); // AAzzz
    gen.set_counter_tail(57); // z

    gen.increment_carry_test();

    assert_eq!(gen.counter_tail(), 0); // '1' = index 0
    assert_eq!(&gen.prefix_plus_counter_head()[8..], &[9, 10, 0, 0, 0]); // AB111
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
