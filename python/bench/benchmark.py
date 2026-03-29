"""Benchmark for sparkid."""

import argparse
import importlib
import sys
import time
import uuid
from collections.abc import Callable

if sys.version_info < (3, 14):
    sys.exit("benchmark requires Python 3.14+ (for stdlib uuid.uuid7)")

from sparkid import IdGenerator

generate_id = IdGenerator()

ITERATIONS = 1_000_000
WARMUP = 10_000
TRIALS = 5


def verify() -> None:
    """Run correctness checks."""
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    valid = set(alphabet)
    ids: set[str] = set()

    for _ in range(50_000):
        id_ = generate_id()

        if len(id_) != 21:
            raise AssertionError(f"wrong length: {id_} ({len(id_)})")
        if not set(id_) <= valid:
            raise AssertionError(f"invalid chars in: {id_}")
        if id_ in ids:
            raise AssertionError(f"duplicate: {id_}")
        ids.add(id_)

    print("correctness: 50,000 IDs valid, unique, correct charset")

    # Monotonicity (burst within same millisecond)
    burst = [generate_id() for _ in range(10_000)]
    for i in range(1, len(burst)):
        if burst[i] <= burst[i - 1]:
            raise AssertionError(f"not monotonic at {i}: {burst[i - 1]} >= {burst[i]}")
    print("correctness: monotonicity OK (10,000 burst IDs)")

    # Sortability (across milliseconds)
    batch_a = [generate_id() for _ in range(10)]
    time.sleep(0.02)
    batch_b = [generate_id() for _ in range(10)]
    if max(batch_a) >= min(batch_b):
        raise AssertionError(f"sort order broken: {max(batch_a)} >= {min(batch_b)}")
    print("correctness: sortability OK")


def bench_candidate(
    generate: Callable[[], str], *, verbose: bool = False
) -> tuple[float, int]:
    """Benchmark a single generator, return (median_us, median_throughput)."""
    for _ in range(WARMUP):
        generate()

    results: list[float] = []

    for trial in range(TRIALS):
        start = time.perf_counter_ns()
        for _ in range(ITERATIONS):
            generate()
        elapsed_ns = time.perf_counter_ns() - start

        per_call_us = elapsed_ns / ITERATIONS / 1_000
        results.append(per_call_us)

        if verbose:
            throughput = round(ITERATIONS / (elapsed_ns / 1_000_000_000))
            print(
                f"  trial {trial + 1}: {per_call_us:.3f} µs/call"
                f"  {throughput:,} ids/sec"
            )

    results.sort()
    median_us = results[len(results) // 2]
    median_throughput = round(1_000_000 / median_us)
    return median_us, median_throughput


def benchmark() -> None:
    """Run performance benchmark."""
    print()
    print(f"warmup:      {WARMUP:,} calls")
    print(f"iterations:  {ITERATIONS:,} per trial")
    print(f"trials:      {TRIALS}")
    print()

    median_us, median_throughput = bench_candidate(generate_id, verbose=True)

    print()
    print(f"  median: {median_us:.3f} µs/call  {median_throughput:,} ids/sec")
    print()

    # Sample output
    print("sample IDs:")
    for _ in range(5):
        print(f"  {generate_id()}")


def run_comparison() -> None:
    """Run comparison benchmark across multiple ID generators."""
    print()
    print("=== ID Generator Comparison ===")
    print()
    print(f"warmup:      {WARMUP:,} calls")
    print(f"iterations:  {ITERATIONS:,} per trial")
    print(f"trials:      {TRIALS}")
    print()

    candidates: list[dict[str, object]] = [
        {
            "name": "sparkid",
            "generate": generate_id,
            "length": "21",
            "sortable": "yes",
            "format": "Base58",
        },
    ]

    # uuid4 (stdlib)
    candidates.append(
        {
            "name": "uuid4",
            "generate": lambda: str(uuid.uuid4()),
            "length": "36",
            "sortable": "no",
            "format": "hex+dashes",
        }
    )

    # uuid7 (Python 3.14+ stdlib)
    candidates.append(
        {
            "name": "uuid7",
            "generate": lambda: str(uuid.uuid7()),
            "length": "36",
            "sortable": "yes",
            "format": "hex+dashes",
        }
    )

    # nanoid
    try:
        nanoid_mod = importlib.import_module("nanoid")
        candidates.append(
            {
                "name": "nanoid",
                "generate": nanoid_mod.generate,
                "length": "21",
                "sortable": "no",
                "format": "URL-safe",
            }
        )
    except ImportError:
        print("  ⚠ nanoid not installed, skipping (pip install nanoid)")

    # ulid
    try:
        ulid_mod = importlib.import_module("ulid")
        ulid_cls = ulid_mod.ULID
        candidates.append(
            {
                "name": "ulid",
                "generate": lambda: str(ulid_cls()),
                "length": "26",
                "sortable": "yes",
                "format": "Crockford Base32",
            }
        )
    except ImportError:
        print("  ⚠ python-ulid not installed, skipping (pip install python-ulid)")

    # cuid2
    try:
        cuid2_mod = importlib.import_module("cuid2")
        cuid_gen = cuid2_mod.Cuid()
        candidates.append(
            {
                "name": "cuid2",
                "generate": cuid_gen.generate,
                "length": "24-25",
                "sortable": "no",
                "format": "alphanumeric",
            }
        )
    except ImportError:
        pass  # cuid2 is optional — skip silently

    # Bench each candidate
    rows: list[dict[str, object]] = []

    for c in candidates:
        name = str(c["name"])
        generate = c["generate"]
        sys.stdout.write(f"  benchmarking {name}...")
        sys.stdout.flush()
        median_us, median_throughput = bench_candidate(generate)  # type: ignore[arg-type]
        print(f" {median_throughput:,} ids/sec")
        rows.append(
            {
                "name": name,
                "median_us": median_us,
                "median_throughput": median_throughput,
                "sample": generate(),  # type: ignore[operator]
                "length": c["length"],
                "sortable": c["sortable"],
                "format": c["format"],
            }
        )

    # Sort by throughput descending
    rows.sort(key=lambda r: r["median_throughput"], reverse=True)  # type: ignore[arg-type]

    # Print results table
    name_w = max(10, *(len(str(r["name"])) for r in rows))
    us_w = 10
    tp_w = 14
    len_w = 6
    sort_w = 8
    fmt_w = max(6, *(len(str(r["format"])) for r in rows))

    header = "  ".join(
        [
            "Generator".ljust(name_w),
            "µs/call".rjust(us_w),
            "ids/sec".rjust(tp_w),
            "Len".rjust(len_w),
            "Sortable".rjust(sort_w),
            "Format".ljust(fmt_w),
            "Sample",
        ]
    )
    separator = "-" * (len(header) + 30)

    print()
    print(header)
    print(separator)

    for r in rows:
        print(
            "  ".join(
                [
                    str(r["name"]).ljust(name_w),
                    f"{r['median_us']:.3f}".rjust(us_w),
                    f"{r['median_throughput']:,}".rjust(tp_w),
                    str(r["length"]).rjust(len_w),
                    str(r["sortable"]).rjust(sort_w),
                    str(r["format"]).ljust(fmt_w),
                    str(r["sample"]),
                ]
            )
        )

    print()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Benchmark sparkid")
    parser.add_argument(
        "--compare",
        action="store_true",
        help="Compare sparkid against other ID generators",
    )
    args = parser.parse_args()

    if args.compare:
        ITERATIONS = 100_000
        WARMUP = 1_000

    verify()
    if args.compare:
        run_comparison()
    else:
        benchmark()
