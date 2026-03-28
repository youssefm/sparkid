"""Comprehensive tests for the sparkid ID generator."""

import os
import threading
import time
from collections import Counter
from unittest.mock import patch

from sparkid import IdGenerator, generate_id
from sparkid._generator import (
    _FIRST_CHAR,
    ALPHABET,
    BASE,
    _after_fork_in_child,
    _all_generators,
)

VALID_CHARS = frozenset(ALPHABET)


# ---------------------------------------------------------------------------
# ID format
# ---------------------------------------------------------------------------


class TestIdFormat:
    def test_length(self):
        gen = IdGenerator()
        for _ in range(1000):
            assert len(gen()) == 22

    def test_charset(self):
        gen = IdGenerator()
        for _ in range(1000):
            assert set(gen()) <= VALID_CHARS

    def test_no_ambiguous_chars(self):
        gen = IdGenerator()
        forbidden = set("0OIl")
        for _ in range(1000):
            assert not set(gen()) & forbidden

    def test_all_alphanumeric_url_safe(self):
        gen = IdGenerator()
        for _ in range(1000):
            assert gen().isalnum()

    def test_structure_parts(self):
        """ID has 8-char timestamp, 6-char counter, 8-char random."""
        gen = IdGenerator()
        id_ = gen()
        ts = id_[:8]
        counter = id_[8:14]
        random_tail = id_[14:]
        assert len(ts) == 8
        assert len(counter) == 6
        assert len(random_tail) == 8
        assert set(ts) <= VALID_CHARS
        assert set(counter) <= VALID_CHARS
        assert set(random_tail) <= VALID_CHARS


# ---------------------------------------------------------------------------
# Timestamp encoding (verified through generated IDs)
# ---------------------------------------------------------------------------


def _decode_timestamp(encoded: str) -> int:
    val = 0
    for ch in encoded:
        val = val * BASE + ALPHABET.index(ch)
    return val


class TestTimestampEncoding:
    def test_prefix_encodes_current_millisecond(self):
        before = int(time.time() * 1000)
        id_ = generate_id()
        after = int(time.time() * 1000)
        decoded = _decode_timestamp(id_[:8])
        assert before <= decoded <= after

    def test_prefix_round_trips_through_decode(self):
        id_ = generate_id()
        decoded = _decode_timestamp(id_[:8])
        ts_now = int(time.time() * 1000)
        assert abs(decoded - ts_now) < 100

    def test_prefix_increases_across_milliseconds(self):
        gen = IdGenerator()
        id_a = gen()
        time.sleep(0.02)
        id_b = gen()
        assert id_b[:8] > id_a[:8]

    def test_prefix_is_lexicographically_sortable(self):
        """Lexicographic order of prefix must match numeric order of timestamps."""
        gen = IdGenerator()
        prefixes = []
        for _ in range(20):
            id_ = gen()
            prefixes.append(id_[:8])
            start = time.time()
            while time.time() - start < 0.001:
                pass  # busy-wait to cross ms boundary

        for i in range(1, len(prefixes)):
            ts_prev = _decode_timestamp(prefixes[i - 1])
            ts_curr = _decode_timestamp(prefixes[i])
            if ts_curr > ts_prev:
                assert prefixes[i] > prefixes[i - 1]

    def test_encode_boundary_values(self):
        """Encoding of specific timestamps via internal API."""
        gen = IdGenerator()

        gen._encode_timestamp(0)
        assert gen._prefix_plus_counter_head[:8] == "1" * 8

        gen._encode_timestamp(1)
        assert gen._prefix_plus_counter_head[:8] == "1" * 7 + "2"

        gen._encode_timestamp(57)
        assert gen._prefix_plus_counter_head[:8] == "1" * 7 + "z"

        gen._encode_timestamp(58)
        assert gen._prefix_plus_counter_head[:8] == "1" * 6 + "21"

    def test_encode_monotonic_over_range(self):
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        prev = ""
        for offset in range(10000):
            gen._encode_timestamp(ts + offset)
            encoded = gen._prefix_plus_counter_head[:8]
            assert encoded > prev, f"Not monotonic at offset {offset}"
            prev = encoded

    def test_encode_round_trip(self):
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        gen._encode_timestamp(ts)
        encoded = gen._prefix_plus_counter_head[:8]
        assert _decode_timestamp(encoded) == ts

    def test_encode_digit_boundaries(self):
        """Verify monotonicity across Base58 digit rollover boundaries."""
        gen = IdGenerator()
        for power in range(1, 8):
            boundary = BASE**power
            gen._encode_timestamp(boundary - 1)
            before = gen._prefix_plus_counter_head[:8]
            gen._encode_timestamp(boundary)
            at = gen._prefix_plus_counter_head[:8]
            gen._encode_timestamp(boundary + 1)
            after = gen._prefix_plus_counter_head[:8]
            assert before < at < after

    def test_encode_preserves_counter_head(self):
        gen = IdGenerator()
        gen._prefix_plus_counter_head = "XXXXXXXX" + "abcde"
        gen._encode_timestamp(12345)
        assert gen._prefix_plus_counter_head[8:] == "abcde"


# ---------------------------------------------------------------------------
# Counter monotonicity
# ---------------------------------------------------------------------------


class TestCounterMonotonicity:
    def test_burst_strictly_increasing(self):
        gen = IdGenerator()
        ids = [gen() for _ in range(50_000)]
        for i in range(1, len(ids)):
            assert ids[i] > ids[i - 1], f"Not monotonic at {i}"

    def test_counter_increments_within_same_ms(self):
        gen = IdGenerator()
        # Pin time to a fixed millisecond to guarantee all IDs share the same timestamp.
        fixed_ts = int(time.time() * 1000) + 100_000
        with patch("sparkid._generator._time_ns", return_value=fixed_ts * 1_000_000):
            ids = [gen() for _ in range(500)]
        assert all(id_[:8] == ids[0][:8] for id_ in ids), (
            "all IDs should share timestamp"
        )
        for i in range(1, len(ids)):
            assert ids[i][8:14] > ids[i - 1][8:14]

    def test_sort_order_matches_generation_order(self):
        gen = IdGenerator()
        ids = [gen() for _ in range(10_000)]
        assert ids == sorted(ids)

    def test_counter_randomly_seeded_each_ms(self):
        """Counter starting value should differ across milliseconds."""
        gen = IdGenerator()
        seeds = []
        for _ in range(10):
            id_ = gen()
            seeds.append(id_[8:14])
            time.sleep(0.005)
        assert len(set(seeds)) > 1, "counter seeds should vary across ms"


class TestCounterCarry:
    def test_single_carry(self):
        gen = IdGenerator()
        gen._prefix_plus_counter_head = "XXXXXXXX" + "AAAAA"
        gen._counter_head_buf = bytearray(
            [ord("A"), ord("A"), ord("A"), ord("A"), ord("A")]
        )
        gen._counter_tail = "z"

        gen._increment_counter_carry()

        assert gen._counter_tail == _FIRST_CHAR
        buf_str = gen._counter_head_buf.decode("ascii")
        assert buf_str == "AAAAB"

    def test_cascading_carry(self):
        gen = IdGenerator()
        gen._prefix_plus_counter_head = "XXXXXXXX" + "AAzzz"
        gen._counter_head_buf = bytearray(
            [ord("A"), ord("A"), ord("z"), ord("z"), ord("z")]
        )
        gen._counter_tail = "z"

        gen._increment_counter_carry()

        assert gen._counter_tail == _FIRST_CHAR
        buf_str = gen._counter_head_buf.decode("ascii")
        assert buf_str == "AB111"

    def test_full_overflow_bumps_timestamp(self):
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        gen._timestamp_cache_ms = ts
        gen._encode_timestamp(ts)
        gen._prefix_plus_counter_head = gen._prefix_plus_counter_head[:8] + "zzzzz"
        gen._counter_head_buf = bytearray([ord("z")] * 5)
        gen._counter_tail = "z"

        gen._increment_counter_carry()

        assert gen._timestamp_cache_ms == ts + 1

    def test_full_overflow_produces_valid_id(self):
        """After full counter overflow, the next ID should still be valid."""
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        gen._timestamp_cache_ms = ts
        gen._encode_timestamp(ts)
        gen._seed_counter()

        # Force counter to max
        gen._prefix_plus_counter_head = gen._prefix_plus_counter_head[:8] + "zzzzz"
        gen._counter_head_buf = bytearray([ord("z")] * 5)
        gen._counter_tail = "z"

        gen._increment_counter_carry()

        id_ = gen()
        assert len(id_) == 22
        assert set(id_) <= VALID_CHARS

    def test_carry_maintains_monotonicity(self):
        """Generate IDs that force carry and verify order is preserved."""
        gen = IdGenerator()
        gen._counter_tail = "y"  # one before 'z'
        ids = [gen() for _ in range(5)]
        for i in range(1, len(ids)):
            assert ids[i] > ids[i - 1]

    def test_carry_observable_in_burst(self):
        """Within one ms, 200 IDs cross multiple carry events (58 values per tail)."""
        gen = IdGenerator()
        ids = [gen() for _ in range(200)]
        ts = ids[0][:8]
        same_ts = [id_ for id_ in ids if id_[:8] == ts]
        for i in range(1, len(same_ts)):
            assert same_ts[i][8:14] > same_ts[i - 1][8:14]


# ---------------------------------------------------------------------------
# Uniqueness
# ---------------------------------------------------------------------------


class TestUniqueness:
    def test_no_duplicates_single_generator(self):
        gen = IdGenerator()
        ids = [gen() for _ in range(100_000)]
        assert len(set(ids)) == len(ids)

    def test_no_duplicates_across_generators(self):
        gen1 = IdGenerator()
        gen2 = IdGenerator()
        ids1 = {gen1() for _ in range(10_000)}
        ids2 = {gen2() for _ in range(10_000)}
        assert not ids1 & ids2

    def test_no_duplicates_generate_id(self):
        ids = [generate_id() for _ in range(50_000)]
        assert len(set(ids)) == len(ids)


# ---------------------------------------------------------------------------
# Cross-millisecond sortability
# ---------------------------------------------------------------------------


class TestCrossMillisecondSortability:
    def test_later_batch_sorts_after(self):
        gen = IdGenerator()
        batch_a = [gen() for _ in range(50)]
        time.sleep(0.02)
        batch_b = [gen() for _ in range(50)]
        assert max(batch_a) < min(batch_b)

    def test_interleaved_generation_stays_sorted(self):
        gen = IdGenerator()
        all_ids: list[str] = []
        for _ in range(5):
            all_ids.extend(gen() for _ in range(20))
            time.sleep(0.01)
        assert all_ids == sorted(all_ids)


# ---------------------------------------------------------------------------
# Clock regression (backward clock)
# ---------------------------------------------------------------------------


class TestClockRegression:
    def test_monotonic_despite_backward_clock(self):
        gen = IdGenerator()
        real_ts = int(time.time() * 1000)

        with patch("sparkid._generator._time_ns", return_value=real_ts * 1_000_000):
            id1 = gen()

        backward_ts = (real_ts - 100) * 1_000_000
        with patch("sparkid._generator._time_ns", return_value=backward_ts):
            id2 = gen()

        assert id2 > id1, "Monotonicity must be preserved when clock goes backward"
        assert id2[:8] == id1[:8]  # Timestamp unchanged, counter incremented

    def test_clock_catches_up_after_regression(self):
        gen = IdGenerator()
        real_ts = int(time.time() * 1000)

        with patch("sparkid._generator._time_ns", return_value=real_ts * 1_000_000):
            id1 = gen()

        with patch(
            "sparkid._generator._time_ns", return_value=(real_ts - 50) * 1_000_000
        ):
            id2 = gen()

        with patch(
            "sparkid._generator._time_ns", return_value=(real_ts + 50) * 1_000_000
        ):
            id3 = gen()

        assert id1 < id2 < id3
        assert id3[:8] > id1[:8]  # New timestamp prefix after catch-up


# ---------------------------------------------------------------------------
# Random tail properties
# ---------------------------------------------------------------------------


class TestRandomTail:
    def test_uniform_char_distribution(self):
        """Chi-squared test on random tail characters."""
        gen = IdGenerator()
        n = 100_000
        counts: Counter[str] = Counter()
        for _ in range(n):
            id_ = gen()
            for ch in id_[14:]:
                counts[ch] += 1

        total = sum(counts.values())
        expected = total / 58
        chi_sq = sum((c - expected) ** 2 / expected for c in counts.values())
        assert chi_sq < 120, f"Random tail appears non-uniform (chi^2={chi_sq:.1f})"

    def test_all_58_chars_appear(self):
        gen = IdGenerator()
        seen: set[str] = set()
        for _ in range(50_000):
            seen.update(gen()[14:])
        assert seen == VALID_CHARS

    def test_random_tails_differ(self):
        """IDs generated in the same ms should have different random tails."""
        gen = IdGenerator()
        ids = [gen() for _ in range(100)]
        tails = [id_[14:] for id_ in ids]
        assert len(set(tails)) == len(tails)

    def test_no_modulo_bias(self):
        """Max/min char frequency ratio should be close to 1."""
        gen = IdGenerator()
        counts: Counter[str] = Counter()
        for _ in range(200_000):
            for ch in gen()[14:]:
                counts[ch] += 1
        ratio = max(counts.values()) / min(counts.values())
        assert ratio < 1.05, f"max/min ratio {ratio:.4f} suggests bias"


# ---------------------------------------------------------------------------
# Rejection sampling internals
# ---------------------------------------------------------------------------


class TestRejectionSampling:
    def test_refill_produces_valid_chars(self):
        gen = IdGenerator()
        gen._refill_random()
        for ch in gen._random_char_buffer:
            assert ch in VALID_CHARS

    def test_refill_byte_string_consistency(self):
        gen = IdGenerator()
        gen._refill_random()
        assert gen._random_char_buffer == gen._random_byte_buffer.decode("ascii")

    def test_refill_length_tracking(self):
        gen = IdGenerator()
        gen._refill_random()
        assert gen._random_byte_len == len(gen._random_byte_buffer)
        assert gen._random_byte_len == len(gen._random_char_buffer)
        assert gen._random_byte_position == 0

    def test_expected_yield(self):
        """Rejection sampling should yield ~90.6% of input bytes."""
        gen = IdGenerator()
        gen._refill_random()
        ratio = gen._random_byte_len / 256  # _RANDOM_BATCH_SIZE
        assert 0.80 < ratio < 0.98


# ---------------------------------------------------------------------------
# Thread safety
# ---------------------------------------------------------------------------


class TestThreadSafety:
    def test_generate_id_per_thread_isolation(self):
        """Each thread gets its own IdGenerator; IDs are monotonic per-thread and
        unique across threads."""
        results: dict[str, list[str]] = {}
        errors: list[Exception] = []

        def worker(name: str, count: int) -> None:
            try:
                ids = [generate_id() for _ in range(count)]
                results[name] = ids
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=worker, args=(f"t{i}", 5000)) for i in range(4)
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert not errors

        for name, ids in results.items():
            for i in range(1, len(ids)):
                assert ids[i] > ids[i - 1], f"Thread {name} not monotonic at {i}"

        all_ids = [id_ for ids in results.values() for id_ in ids]
        assert len(set(all_ids)) == len(all_ids)


# ---------------------------------------------------------------------------
# Fork safety
# ---------------------------------------------------------------------------


class TestForkSafety:
    def test_generators_tracked_in_weakset(self):
        gen = IdGenerator()
        assert gen in _all_generators

    def test_gc_removes_from_weakset(self):
        import gc

        gen = IdGenerator()
        ref_id = id(gen)
        assert any(id(g) == ref_id for g in _all_generators)
        del gen
        gc.collect()
        assert not any(id(g) == ref_id for g in _all_generators)

    def test_reset_state_clears_everything(self):
        gen = IdGenerator()
        for _ in range(100):
            gen()
        assert gen._timestamp_cache_ms > 0
        assert gen._random_byte_len > 0

        gen._reset_state()

        assert gen._timestamp_cache_ms == 0
        assert gen._prefix_plus_counter_head == _FIRST_CHAR * 13
        assert gen._counter_tail == _FIRST_CHAR
        assert gen._counter_head_buf == bytearray(b"1" * 5)
        assert gen._random_char_buffer == ""
        assert gen._random_byte_buffer == b""
        assert gen._random_byte_position == 0
        assert gen._random_byte_len == 0

    def test_reset_then_generate_produces_valid_ids(self):
        gen = IdGenerator()
        for _ in range(100):
            gen()
        gen._reset_state()
        id_ = gen()
        assert len(id_) == 22
        assert set(id_) <= VALID_CHARS

    def test_after_fork_resets_all_generators(self):
        gen1 = IdGenerator()
        gen2 = IdGenerator()
        for _ in range(100):
            gen1()
            gen2()

        _after_fork_in_child()

        for gen in (gen1, gen2):
            assert gen._timestamp_cache_ms == 0
            assert gen._random_byte_len == 0

    def test_fork_produces_no_duplicates(self):
        """After os.fork(), parent and child should produce distinct IDs."""
        gen = IdGenerator()
        for _ in range(10):
            gen()

        r, w = os.pipe()

        pid = os.fork()
        if pid == 0:
            os.close(r)
            child_ids = [gen() for _ in range(50)]
            os.write(w, "\n".join(child_ids).encode())
            os.close(w)
            os._exit(0)
        else:
            os.close(w)
            parent_ids = [gen() for _ in range(50)]

            data = b""
            while True:
                chunk = os.read(r, 4096)
                if not chunk:
                    break
                data += chunk
            os.close(r)
            os.waitpid(pid, 0)

            child_ids = data.decode().split("\n")
            collisions = set(parent_ids) & set(child_ids)
            assert not collisions, f"Fork produced {len(collisions)} duplicate IDs"


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


class TestPublicAPI:
    def test_generate_id_returns_string(self):
        assert isinstance(generate_id(), str)

    def test_generate_id_format(self):
        id_ = generate_id()
        assert len(id_) == 22
        assert set(id_) <= VALID_CHARS

    def test_id_generator_callable(self):
        gen = IdGenerator()
        id_ = gen()
        assert isinstance(id_, str)
        assert len(id_) == 22

    def test_multiple_generators_independent(self):
        gen1 = IdGenerator()
        gen2 = IdGenerator()
        ids1 = [gen1() for _ in range(100)]
        ids2 = [gen2() for _ in range(100)]
        assert ids1 == sorted(ids1)
        assert ids2 == sorted(ids2)
        assert not set(ids1) & set(ids2)

    def test_exports(self):
        import sparkid

        assert hasattr(sparkid, "generate_id")
        assert hasattr(sparkid, "IdGenerator")
        assert sparkid.__all__ == ["generate_id", "IdGenerator"]
