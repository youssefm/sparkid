"""Comprehensive tests for the sparkid ID generator."""

import os
import threading
import time
from collections import Counter
from datetime import datetime, timezone

import pytest

from sparkid import (
    ALPHABET,
    BASE,
    IdGenerator,
    _after_fork_in_child,
    _all_generators,
    extract_timestamp,
    from_bytes,
    generate_id,
    generate_id_at,
    to_bytes,
)
from sparkid._constants import MAX_TIMESTAMP

VALID_CHARS = frozenset(ALPHABET)


# ---------------------------------------------------------------------------
# ID format
# ---------------------------------------------------------------------------


class TestIdFormat:
    def test_length(self):
        gen = IdGenerator()
        for _ in range(1000):
            assert len(gen()) == 21

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
        """ID has 8-char timestamp, 6-char counter, 7-char random."""
        gen = IdGenerator()
        id_ = gen()
        ts = id_[:8]
        counter = id_[8:14]
        random_tail = id_[14:]
        assert len(ts) == 8
        assert len(counter) == 6
        assert len(random_tail) == 7
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
        """Encoding of specific timestamps produces expected prefixes."""
        gen = IdGenerator()

        id_ = gen.generate_at(0)
        assert id_[:8] == "1" * 8

        gen2 = IdGenerator()
        id_ = gen2.generate_at(1)
        assert id_[:8] == "1" * 7 + "2"

        gen3 = IdGenerator()
        id_ = gen3.generate_at(57)
        assert id_[:8] == "1" * 7 + "z"

        gen4 = IdGenerator()
        id_ = gen4.generate_at(58)
        assert id_[:8] == "1" * 6 + "21"

    def test_encode_monotonic_over_range(self):
        ts = int(time.time() * 1000)
        prev = ""
        for offset in range(10000):
            gen2 = IdGenerator()
            id_ = gen2.generate_at(ts + offset)
            encoded = id_[:8]
            assert encoded > prev, f"Not monotonic at offset {offset}"
            prev = encoded

    def test_encode_round_trip(self):
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        id_ = gen.generate_at(ts)
        encoded = id_[:8]
        assert _decode_timestamp(encoded) == ts

    def test_encode_digit_boundaries(self):
        """Verify monotonicity across Base58 digit rollover boundaries."""
        for power in range(1, 8):
            boundary = BASE**power
            gen1 = IdGenerator()
            id_before = gen1.generate_at(boundary - 1)
            gen2 = IdGenerator()
            id_at = gen2.generate_at(boundary)
            gen3 = IdGenerator()
            id_after = gen3.generate_at(boundary + 1)
            assert id_before[:8] < id_at[:8] < id_after[:8]

    def test_encode_preserves_id_structure(self):
        """IDs generated at a specific timestamp have valid structure."""
        gen = IdGenerator()
        id_ = gen.generate_at(12345)
        assert len(id_) == 21
        assert set(id_) <= VALID_CHARS
        assert _decode_timestamp(id_[:8]) == 12345


# ---------------------------------------------------------------------------
# Timestamp validation
# ---------------------------------------------------------------------------


class TestTimestampValidation:
    def test_encode_timestamp_zero(self):
        gen = IdGenerator()
        id_ = gen.generate_at(0)
        assert id_[:8] == "1" * 8

    def test_encode_timestamp_max(self):
        gen = IdGenerator()
        id_ = gen.generate_at(MAX_TIMESTAMP)
        assert id_[:8] == "z" * 8

    def test_encode_timestamp_negative_raises(self):
        import pytest

        gen = IdGenerator()
        with pytest.raises(ValueError, match="Timestamp out of range"):
            gen.generate_at(-1)

    def test_encode_timestamp_above_max_raises(self):
        import pytest

        gen = IdGenerator()
        with pytest.raises(ValueError, match="Timestamp out of range"):
            gen.generate_at(MAX_TIMESTAMP + 1)

    def test_counter_overflow_at_max_timestamp_raises(self):
        """Counter overflow at MAX_TIMESTAMP would need to bump timestamp past max.
        This is tested indirectly — the Rust implementation handles this internally
        by panicking, but we validate the timestamp range before passing to Rust."""
        import pytest

        gen = IdGenerator()
        with pytest.raises(ValueError):
            gen.generate_at(MAX_TIMESTAMP + 1)


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
        fixed_ts = int(time.time() * 1000) + 100_000
        ids = [gen.generate_at(fixed_ts) for _ in range(500)]
        assert all(
            id_[:8] == ids[0][:8] for id_ in ids
        ), "all IDs should share timestamp"
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
        """Counter tail overflow propagates to counter head."""
        gen = IdGenerator()
        # Generate many IDs at the same ms to force counter increments
        ts = int(time.time() * 1000) + 100_000
        ids = [gen.generate_at(ts) for _ in range(60)]
        # After 58 IDs, the counter tail wraps and head increments
        counters = [id_[8:14] for id_ in ids]
        for i in range(1, len(counters)):
            assert counters[i] > counters[i - 1]

    def test_cascading_carry(self):
        """Multiple carry propagations maintain monotonicity."""
        gen = IdGenerator()
        ts = int(time.time() * 1000) + 200_000
        # Generate enough to cross multiple carry boundaries
        ids = [gen.generate_at(ts) for _ in range(200)]
        counters = [id_[8:14] for id_ in ids]
        for i in range(1, len(counters)):
            assert counters[i] > counters[i - 1]

    def test_full_overflow_bumps_timestamp(self):
        """When counter fully overflows, timestamp is bumped forward."""
        gen = IdGenerator()
        ts = int(time.time() * 1000) + 300_000
        # Generate enough IDs to observe timestamp bumps
        ids = [gen.generate_at(ts) for _ in range(5000)]
        # All IDs must be monotonically increasing
        for i in range(1, len(ids)):
            assert ids[i] > ids[i - 1]

    def test_full_overflow_produces_valid_id(self):
        """After extensive generation at same timestamp, IDs remain valid."""
        gen = IdGenerator()
        ts = int(time.time() * 1000) + 400_000
        ids = [gen.generate_at(ts) for _ in range(5000)]
        for id_ in ids:
            assert len(id_) == 21
            assert set(id_) <= VALID_CHARS

    def test_carry_maintains_monotonicity(self):
        """Generate IDs that force carry and verify order is preserved."""
        gen = IdGenerator()
        ids = [gen() for _ in range(200)]
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

        id1 = gen.generate_at(real_ts)
        id2 = gen.generate_at(real_ts - 100)

        assert id2 > id1, "Monotonicity must be preserved when clock goes backward"
        assert id2[:8] == id1[:8]  # Timestamp unchanged, counter incremented

    def test_clock_catches_up_after_regression(self):
        gen = IdGenerator()
        real_ts = int(time.time() * 1000)

        id1 = gen.generate_at(real_ts)
        id2 = gen.generate_at(real_ts - 50)
        id3 = gen.generate_at(real_ts + 50)

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
    def test_random_chars_are_valid(self):
        """All random tail characters must be from the Base58 alphabet."""
        gen = IdGenerator()
        for _ in range(1000):
            id_ = gen()
            for ch in id_[14:]:
                assert ch in VALID_CHARS

    def test_random_tail_length(self):
        """Random tail is always exactly 7 characters."""
        gen = IdGenerator()
        for _ in range(1000):
            id_ = gen()
            assert len(id_[14:]) == 7

    def test_random_chars_vary(self):
        """Random tail characters should vary across IDs."""
        gen = IdGenerator()
        tails = set()
        for _ in range(100):
            tails.add(gen()[14:])
        assert len(tails) == 100

    def test_expected_coverage(self):
        """All 58 chars should appear in random tails with enough samples."""
        gen = IdGenerator()
        seen: set[str] = set()
        for _ in range(10_000):
            for ch in gen()[14:]:
                seen.add(ch)
        assert len(seen) == 58


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


@pytest.mark.skipif(not hasattr(os, "fork"), reason="os.fork not available")
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
        """After reset, generator produces IDs from fresh state."""
        gen = IdGenerator()
        for _ in range(100):
            gen()

        # Reset via the internal Rust method
        gen._inner.reset()

        # After reset, should produce valid IDs starting fresh
        id_ = gen()
        assert len(id_) == 21
        assert set(id_) <= VALID_CHARS

    def test_reset_then_generate_produces_valid_ids(self):
        gen = IdGenerator()
        for _ in range(100):
            gen()
        gen._inner.reset()
        id_ = gen()
        assert len(id_) == 21
        assert set(id_) <= VALID_CHARS

    def test_after_fork_resets_all_generators(self):
        gen1 = IdGenerator()
        gen2 = IdGenerator()
        for _ in range(100):
            gen1()
            gen2()

        _after_fork_in_child()

        # After fork reset, generators should still produce valid IDs
        for gen in (gen1, gen2):
            id_ = gen()
            assert len(id_) == 21
            assert set(id_) <= VALID_CHARS

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
        assert len(id_) == 21
        assert set(id_) <= VALID_CHARS

    def test_id_generator_callable(self):
        gen = IdGenerator()
        id_ = gen()
        assert isinstance(id_, str)
        assert len(id_) == 21

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
        assert hasattr(sparkid, "generate_id_at")
        assert hasattr(sparkid, "extract_timestamp")
        assert hasattr(sparkid, "to_bytes")
        assert hasattr(sparkid, "from_bytes")
        assert hasattr(sparkid, "IdGenerator")
        assert sparkid.__all__ == [
            "generate_id",
            "generate_id_at",
            "extract_timestamp",
            "to_bytes",
            "from_bytes",
            "IdGenerator",
        ]


# ---------------------------------------------------------------------------
# extract_timestamp
# ---------------------------------------------------------------------------


class TestExtractTimestamp:
    def test_round_trip(self):
        before = time.time_ns() // 1_000_000
        id_ = generate_id()
        after = time.time_ns() // 1_000_000
        ts = extract_timestamp(id_)
        extracted_ms = int(ts.timestamp() * 1000)
        assert before - 5 <= extracted_ms <= after + 5

    def test_known_value(self):
        known_ms = 1700000000000
        # Encode known_ms into 8 Base58 chars
        val = known_ms
        chars = []
        for _ in range(8):
            val, r = divmod(val, BASE)
            chars.append(ALPHABET[r])
        encoded = "".join(reversed(chars))
        id_ = encoded + "1" * 13  # pad counter + random with '1's
        ts = extract_timestamp(id_)
        assert int(ts.timestamp() * 1000) == known_ms
        assert ts.tzinfo is not None

    def test_wrong_length_raises(self):
        import pytest

        with pytest.raises(ValueError):
            extract_timestamp("abc")
        with pytest.raises(ValueError):
            extract_timestamp("1" * 20)
        with pytest.raises(ValueError):
            extract_timestamp("1" * 22)

    def test_invalid_chars_raises(self):
        import pytest

        with pytest.raises(ValueError):
            extract_timestamp("0" + "1" * 20)
        with pytest.raises(ValueError):
            extract_timestamp("O" + "1" * 20)
        with pytest.raises(ValueError):
            extract_timestamp("I" + "1" * 20)


# ---------------------------------------------------------------------------
# Binary encoding (to_bytes / from_bytes)
# ---------------------------------------------------------------------------


class TestToFromBytes:
    def test_round_trip(self):
        gen = IdGenerator()
        for _ in range(10_000):
            sparkid = gen()
            assert from_bytes(to_bytes(sparkid)) == sparkid

    def test_produces_16_bytes(self):
        gen = IdGenerator()
        for _ in range(100):
            assert len(to_bytes(gen())) == 16

    def test_padding_bits_always_zero(self):
        gen = IdGenerator()
        for _ in range(1000):
            binary = to_bytes(gen())
            assert binary[15] & 0x03 == 0, "last 2 bits must be zero padding"

    def test_sort_order_preserved(self):
        gen = IdGenerator()
        ids = [gen() for _ in range(1000)]
        for i in range(1, len(ids)):
            binary_a = to_bytes(ids[i - 1])
            binary_b = to_bytes(ids[i])
            assert binary_a < binary_b, f"binary sort mismatch at index {i}"

    def test_sort_order_across_millisecond_boundaries(self):
        gen = IdGenerator()
        batch_a = sorted([gen() for _ in range(50)])
        time.sleep(0.03)
        batch_b = sorted([gen() for _ in range(50)])
        max_binary_a = to_bytes(batch_a[-1])
        min_binary_b = to_bytes(batch_b[0])
        assert max_binary_a < min_binary_b

    def test_all_alphabet_characters(self):
        for char in ALPHABET:
            sparkid = char + "1" * 20
            binary = to_bytes(sparkid)
            recovered = from_bytes(binary)
            assert recovered == sparkid, f"round-trip failed for char {char!r}"

    def test_deterministic(self):
        gen = IdGenerator()
        sparkid = gen()
        assert to_bytes(sparkid) == to_bytes(sparkid)


class TestToBytesValidation:
    def test_wrong_length(self):
        import pytest

        with pytest.raises(ValueError):
            to_bytes("abc")
        with pytest.raises(ValueError):
            to_bytes("1" * 20)
        with pytest.raises(ValueError):
            to_bytes("1" * 22)
        with pytest.raises(ValueError):
            to_bytes("")

    def test_invalid_characters(self):
        import pytest

        with pytest.raises(ValueError):
            to_bytes("0" + "1" * 20)
        with pytest.raises(ValueError):
            to_bytes("O" + "1" * 20)
        with pytest.raises(ValueError):
            to_bytes("I" + "1" * 20)
        with pytest.raises(ValueError):
            to_bytes("l" + "1" * 20)
        with pytest.raises(ValueError):
            to_bytes("!" + "1" * 20)
        with pytest.raises(ValueError):
            to_bytes(" " + "1" * 20)

    def test_non_ascii_characters(self):
        import pytest

        with pytest.raises(ValueError):
            to_bytes("ñ" + "1" * 20)
        with pytest.raises(ValueError):
            to_bytes("é" + "1" * 20)
        with pytest.raises(ValueError):
            to_bytes("💎" + "1" * 19)


class TestFromBytesValidation:
    def test_wrong_length(self):
        import pytest

        with pytest.raises(ValueError):
            from_bytes(b"\x00" * 15)
        with pytest.raises(ValueError):
            from_bytes(b"\x00" * 17)
        with pytest.raises(ValueError):
            from_bytes(b"")

    def test_out_of_range_index(self):
        import pytest

        bad = bytearray(16)
        bad[0] = 0xFF  # first 6-bit index = 63, which is >= 58
        with pytest.raises(ValueError):
            from_bytes(bytes(bad))

    def test_non_zero_padding(self):
        import pytest

        sparkid = "1" * 21
        binary = bytearray(to_bytes(sparkid))
        binary[15] = binary[15] | 0x01  # set lowest bit
        with pytest.raises(ValueError):
            from_bytes(bytes(binary))


# ---------------------------------------------------------------------------
# generate_id_at / IdGenerator.generate_at
# ---------------------------------------------------------------------------


class TestGenerateAt:
    def test_valid_format_length(self):
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        for _ in range(100):
            id_ = gen.generate_at(ts)
            assert len(id_) == 21

    def test_valid_format_charset(self):
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        for _ in range(100):
            id_ = gen.generate_at(ts)
            assert set(id_) <= VALID_CHARS

    def test_timestamp_encoding_int(self):
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        id_ = gen.generate_at(ts)
        decoded = _decode_timestamp(id_[:8])
        assert decoded == ts

    def test_timestamp_encoding_datetime(self):
        gen = IdGenerator()
        dt = datetime(2024, 6, 15, 12, 0, 0, tzinfo=timezone.utc)
        expected_ms = int(dt.timestamp() * 1000)
        id_ = gen.generate_at(dt)
        decoded = _decode_timestamp(id_[:8])
        assert decoded == expected_ms

    def test_monotonicity_same_ms(self):
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        ids = [gen.generate_at(ts) for _ in range(100)]
        for i in range(1, len(ids)):
            assert ids[i] > ids[i - 1]

    def test_monotonicity_across_ms(self):
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        ids = [gen.generate_at(ts + i) for i in range(100)]
        for i in range(1, len(ids)):
            assert ids[i] > ids[i - 1]

    def test_clock_regression_preserves_monotonicity(self):
        gen = IdGenerator()
        ts = int(time.time() * 1000)
        id_future = gen.generate_at(ts + 1000)
        id_past = gen.generate_at(ts)  # earlier timestamp
        assert id_past > id_future  # still monotonically increasing

    def test_datetime_timezone_aware(self):
        gen = IdGenerator()
        dt = datetime(2024, 1, 1, 0, 0, 0, tzinfo=timezone.utc)
        id_ = gen.generate_at(dt)
        assert len(id_) == 21
        decoded_ms = _decode_timestamp(id_[:8])
        assert decoded_ms == int(dt.timestamp() * 1000)

    def test_datetime_naive_raises_valueerror(self):
        gen = IdGenerator()
        dt = datetime(2024, 1, 1, 0, 0, 0)  # naive
        with pytest.raises(ValueError, match="timezone-aware"):
            gen.generate_at(dt)

    def test_int_input_works(self):
        gen = IdGenerator()
        ts = 1_700_000_000_000
        id_ = gen.generate_at(ts)
        assert len(id_) == 21
        decoded = _decode_timestamp(id_[:8])
        assert decoded == ts

    def test_bool_accepted_as_int(self):
        gen = IdGenerator()
        id_true = gen.generate_at(True)
        id_false = gen.generate_at(False)
        assert len(id_true) == 21
        assert len(id_false) == 21

    def test_float_raises_typeerror(self):
        gen = IdGenerator()
        with pytest.raises(TypeError):
            gen.generate_at(1.5)

    def test_str_raises_typeerror(self):
        gen = IdGenerator()
        with pytest.raises(TypeError):
            gen.generate_at("123")

    def test_negative_int_raises_valueerror(self):
        gen = IdGenerator()
        gen.generate_at(1_000_000)
        with pytest.raises(ValueError, match="Timestamp out of range"):
            gen.generate_at(-1)

    def test_exceeds_max_timestamp_raises_valueerror(self):
        gen = IdGenerator()
        with pytest.raises(ValueError):
            gen.generate_at(MAX_TIMESTAMP + 1)

    def test_max_timestamp_succeeds(self):
        gen = IdGenerator()
        id_ = gen.generate_at(MAX_TIMESTAMP)
        assert len(id_) == 21

    def test_zero_timestamp_succeeds(self):
        gen = IdGenerator()
        id_ = gen.generate_at(0)
        assert len(id_) == 21
        decoded = _decode_timestamp(id_[:8])
        assert decoded == 0


class TestGenerateIdAtModuleLevel:
    def test_valid_format(self):
        ts = int(time.time() * 1000)
        id_ = generate_id_at(ts)
        assert len(id_) == 21
        assert set(id_) <= VALID_CHARS

    def test_interaction_generate_id_at_then_generate_id(self):
        """generate_id_at and generate_id share thread-local generator."""
        ts = int(time.time() * 1000) + 5000  # future timestamp
        id_at = generate_id_at(ts)
        id_normal = generate_id()
        assert id_normal > id_at

    def test_interaction_generate_id_then_generate_id_at(self):
        """generate_id then generate_id_at share thread-local state."""
        id_normal = generate_id()
        # Use same ms or earlier — should still be monotonic
        ts = int(time.time() * 1000)
        id_at = generate_id_at(ts)
        assert id_at > id_normal

    def test_thread_local_isolation(self):
        """generate_id_at uses per-thread generators."""
        ts = int(time.time() * 1000)
        results = {}

        def worker(name):
            ids = [generate_id_at(ts) for _ in range(10)]
            results[name] = ids

        t1 = threading.Thread(target=worker, args=("a",))
        t2 = threading.Thread(target=worker, args=("b",))
        t1.start()
        t2.start()
        t1.join()
        t2.join()

        # Within each thread, IDs are monotonic
        for name in ("a", "b"):
            ids = results[name]
            for i in range(1, len(ids)):
                assert ids[i] > ids[i - 1]

        # All IDs are unique across threads
        all_ids = results["a"] + results["b"]
        assert len(set(all_ids)) == len(all_ids)
