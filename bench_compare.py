#!/usr/bin/env python3
"""
Run all three sparkid comparison benchmarks (JS, Python, Rust) and generate
a single visually-stunning PNG chart of the results.

Usage:
    python3 bench_compare.py
    python3 bench_compare.py --out results.png
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).parent
JS_DIR = REPO_ROOT / "js"
PYTHON_DIR = REPO_ROOT / "python"
RUST_DIR = REPO_ROOT / "rust"

DEFAULT_OUT = str(REPO_ROOT / "benchmark_comparison.png")

_MAX_ERROR_OUTPUT_CHARS = 400

# Extra directories to include when searching for tools (e.g. npm, uv)
_EXTRA_PATH_DIRS = [
    "/home/runner/.local/bin",
    "/home/runner/.cargo/bin",
    "/usr/local/bin",
    os.path.expanduser("~/.local/bin"),
]


def _which(cmd: str) -> str:
    """Locate *cmd* on PATH (plus common extra dirs), raising if not found."""
    extra = os.pathsep.join(d for d in _EXTRA_PATH_DIRS if d not in os.environ.get("PATH", ""))
    augmented_path = os.environ.get("PATH", "") + (os.pathsep + extra if extra else "")
    found = shutil.which(cmd, path=augmented_path)
    if not found:
        raise FileNotFoundError(
            f"'{cmd}' not found. Make sure it is installed and on your PATH."
        )
    return found

# ── name normalisation ────────────────────────────────────────────────────────

_NAME_MAP: dict[str, str] = {
    "sparkid": "sparkid",
    "uuid v4": "UUID v4",
    "uuid4": "UUID v4",
    "uuid_v4": "UUID v4",
    "uuid v7": "UUID v7",
    "uuid7": "UUID v7",
    "uuid_v7": "UUID v7",
    "nanoid": "nanoid",
    "ulid": "ulid",
    "cuid2": "cuid2",
}


def _norm(name: str) -> str:
    return _NAME_MAP.get(name.lower().strip(), name.strip())


# ── benchmark runners ─────────────────────────────────────────────────────────


def run_js() -> str:
    print("▶  Running JS comparison benchmark …", flush=True)
    try:
        npm = _which("npm")
    except FileNotFoundError as exc:
        print(f"   ⚠ {exc}", file=sys.stderr)
        return ""
    # Ensure dev dependencies (tsx, uuid, nanoid, ulid) are installed
    if not (JS_DIR / "node_modules" / ".bin" / "tsx").exists():
        print("   installing JS dependencies …", flush=True)
        subprocess.run([npm, "install"], cwd=JS_DIR, check=True, capture_output=True)
    result = subprocess.run(
        [npm, "run", "bench:compare"],
        cwd=JS_DIR,
        capture_output=True,
        text=True,
    )
    output = result.stdout + result.stderr
    if result.returncode != 0:
        print(f"   ⚠ JS benchmark exited with code {result.returncode}", file=sys.stderr)
        print(result.stderr[:_MAX_ERROR_OUTPUT_CHARS], file=sys.stderr)
    return output


def run_python() -> str:
    print("▶  Running Python comparison benchmark …", flush=True)
    try:
        uv = _which("uv")
    except FileNotFoundError as exc:
        print(f"   ⚠ {exc}", file=sys.stderr)
        return ""
    # Ensure all benchmark dependencies are installed in the uv environment
    sync = subprocess.run(
        [uv, "sync", "--all-groups"],
        cwd=PYTHON_DIR,
        capture_output=True,
    )
    if sync.returncode != 0:
        print("   ⚠ uv sync failed; benchmark may be missing optional dependencies", file=sys.stderr)
    result = subprocess.run(
        [uv, "run", "python", "bench/benchmark.py", "--compare"],
        cwd=PYTHON_DIR,
        capture_output=True,
        text=True,
    )
    output = result.stdout + result.stderr
    if result.returncode != 0:
        print(f"   ⚠ Python benchmark exited with code {result.returncode}", file=sys.stderr)
        print(result.stderr[:_MAX_ERROR_OUTPUT_CHARS], file=sys.stderr)
    return output


def run_rust() -> str:
    print("▶  Running Rust comparison benchmark …", flush=True)
    try:
        cargo = _which("cargo")
    except FileNotFoundError as exc:
        print(f"   ⚠ {exc}", file=sys.stderr)
        return ""
    result = subprocess.run(
        [cargo, "bench", "--bench", "benchmark"],
        cwd=RUST_DIR,
        capture_output=True,
        text=True,
    )
    # Criterion writes results to stderr; stdout may contain compile output
    output = result.stdout + result.stderr
    if result.returncode != 0:
        print(f"   ⚠ Rust benchmark exited with code {result.returncode}", file=sys.stderr)
    return output


# ── parsers ───────────────────────────────────────────────────────────────────


def parse_js_python(output: str) -> dict[str, float]:
    """
    Parse the tabular output from the JS / Python comparison benchmarks.

    Looks for lines after the long ``---`` separator in the table and extracts
    the generator name (first field) and throughput in ids/sec (third field).

    Returns ``{normalised_name: ids_per_sec}``.
    """
    results: dict[str, float] = {}
    in_table = False
    for line in output.splitlines():
        stripped = line.strip()
        if re.match(r"^-{20,}", stripped):
            in_table = True
            continue
        if in_table:
            if not stripped:
                break
            # Fields are separated by two or more spaces
            parts = re.split(r"\s{2,}", stripped)
            if len(parts) >= 3:
                name = parts[0].strip()
                tp_raw = parts[2].replace(",", "").strip()
                try:
                    tp = float(tp_raw)
                    results[_norm(name)] = tp
                except ValueError:
                    pass
    return results


def parse_rust(output: str) -> dict[str, float]:
    """
    Parse Criterion output for the ``id_generators`` benchmark group.

    Extracts the median (central) time estimate and converts it to ids/sec.

    Returns ``{normalised_name: ids_per_sec}``.
    """
    _UNIT_TO_SECS = {
        "ps": 1e-12,
        "ns": 1e-9,
        "µs": 1e-6,
        "us": 1e-6,
        "ms": 1e-3,
        "s": 1.0,
    }

    results: dict[str, float] = {}
    lines = output.splitlines()
    for i, line in enumerate(lines):
        m = re.match(r"\s*id_generators/(\S+)", line)
        if not m:
            continue
        raw_name = m.group(1)
        # Time may be on the same line or the next non-blank line
        search_lines = [line] + (
            [lines[i + 1]] if i + 1 < len(lines) else []
        )
        for tline in search_lines:
            tm = re.search(
                r"time:\s+\[\s*[\d.]+\s*\S+\s+([\d.]+)\s*(\S+)\s+[\d.]+\s*\S+\s*\]",
                tline,
            )
            if tm:
                value = float(tm.group(1))
                unit = tm.group(2).strip()
                secs = value * _UNIT_TO_SECS.get(unit, 1.0)
                if secs > 0:
                    results[_norm(raw_name)] = 1.0 / secs
                break
    return results


# ── chart ─────────────────────────────────────────────────────────────────────

# Generator display order (sparkid first, then others alphabetically)
_PREFERRED_ORDER = ["sparkid", "UUID v4", "UUID v7", "nanoid", "ulid", "cuid2"]


def _order_names(all_names: set[str]) -> list[str]:
    seen: list[str] = []
    for n in _PREFERRED_ORDER:
        if n in all_names:
            seen.append(n)
    for n in sorted(all_names - set(seen)):
        seen.append(n)
    return seen


def make_chart(
    js: dict[str, float],
    py: dict[str, float],
    rust: dict[str, float],
    out: str,
) -> None:
    try:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
        import matplotlib.ticker as mticker
        import numpy as np
    except ImportError as exc:  # pragma: no cover
        sys.exit(f"Missing dependency: {exc}. Run: pip install matplotlib numpy")

    all_names = set(js) | set(py) | set(rust)
    names = _order_names(all_names)  # generator names, sparkid first
    n = len(names)

    langs = ["JavaScript", "Python", "Rust"]
    datasets = [js, py, rust]
    n_langs = len(langs)

    # One colour per generator - sparkid gets a distinct gold, others muted palette
    GEN_PALETTE = [
        "#F59E0B",  # sparkid - amber gold
        "#60A5FA",  # UUID v4 - blue
        "#34D399",  # UUID v7 - emerald
        "#F472B6",  # nanoid  - pink
        "#A78BFA",  # ulid    - violet
        "#FB923C",  # cuid2   - orange
        "#94A3B8",  # extra   - slate
    ]
    gen_colors = {name: GEN_PALETTE[i % len(GEN_PALETTE)] for i, name in enumerate(names)}

    BAR_W = 0.14
    LANG_SPACING = n * BAR_W + 0.35  # distance between language group centres
    lang_xs = np.arange(n_langs) * LANG_SPACING

    # ── figure setup ─────────────────────────────────────────────────────────
    FIG_BG = "#0d1117"
    AX_BG = "#161b22"
    GRID_COLOR = "#30363d"
    SPINE_COLOR = "#30363d"
    TEXT_COLOR = "#e6edf3"
    MUTED = "#8b949e"

    fig_w = max(18, n_langs * (n * BAR_W + 1.4))
    fig, ax = plt.subplots(figsize=(fig_w, 9))
    fig.patch.set_facecolor(FIG_BG)
    ax.set_facecolor(AX_BG)

    # ── draw bars (grouped by language, coloured by generator) ───────────────
    for gi, name in enumerate(names):
        offset = (gi - (n - 1) / 2) * BAR_W
        vals = np.array([datasets[li].get(name, 0.0) for li in range(n_langs)], dtype=float)
        bar_c = gen_colors[name]
        # sparkid bars get a brighter, slightly wider stroke
        lw = 1.4 if name == "sparkid" else 0.7

        bars = ax.bar(
            lang_xs + offset,
            vals,
            BAR_W,
            label=name,
            color=bar_c,
            edgecolor=bar_c,
            linewidth=lw,
            zorder=3,
            alpha=0.90 if name == "sparkid" else 0.75,
        )

        # Value labels above each bar
        for bar, val in zip(bars, vals):
            if val <= 0:
                continue
            if val >= 1e9:
                label = f"{val / 1e9:.1f}B"
            elif val >= 1e6:
                label = f"{val / 1e6:.1f}M"
            elif val >= 1e3:
                label = f"{val / 1e3:.0f}K"
            else:
                label = str(int(val))
            ax.text(
                bar.get_x() + bar.get_width() / 2,
                bar.get_height() * 1.015,
                label,
                ha="center",
                va="bottom",
                fontsize=7,
                fontweight="bold" if name == "sparkid" else "normal",
                color=bar_c,
                zorder=5,
            )

    # ── axes & grid ───────────────────────────────────────────────────────────
    ax.yaxis.set_major_formatter(
        mticker.FuncFormatter(
            lambda v, _: (
                f"{v / 1e9:.0f}B"
                if v >= 1e9
                else f"{v / 1e6:.0f}M"
                if v >= 1e6
                else f"{v / 1e3:.0f}K"
                if v >= 1e3
                else str(int(v))
            )
        )
    )

    ax.set_xticks(lang_xs)
    ax.set_xticklabels(langs, fontsize=14, fontweight="bold", color=TEXT_COLOR)
    ax.tick_params(axis="x", length=0, pad=10)
    ax.tick_params(axis="y", colors=MUTED, labelsize=10)
    ax.set_ylim(bottom=0)

    ax.set_ylabel("IDs / second  (higher is better ↑)", fontsize=12, color=MUTED, labelpad=12)

    ax.grid(axis="y", color=GRID_COLOR, linewidth=0.7, zorder=0)
    ax.set_axisbelow(True)
    half_group = (n - 1) / 2 * BAR_W
    ax.set_xlim(lang_xs[0] - half_group - 0.3, lang_xs[-1] + half_group + 0.3)

    for spine in ax.spines.values():
        spine.set_edgecolor(SPINE_COLOR)

    # ── title ─────────────────────────────────────────────────────────────────
    fig.text(
        0.5, 0.97,
        "SparkID — ID Generator Benchmark",
        ha="center", va="top",
        fontsize=20, fontweight="bold", color=TEXT_COLOR,
    )
    fig.text(
        0.5, 0.925,
        "Median throughput per language  ·  sparkid vs. alternatives  |  Higher is better",
        ha="center", va="top",
        fontsize=11, color=MUTED,
    )

    # ── legend ────────────────────────────────────────────────────────────────
    ax.legend(
        fontsize=11,
        loc="upper right",
        framealpha=0.4,
        edgecolor=SPINE_COLOR,
        facecolor=AX_BG,
        labelcolor=TEXT_COLOR,
        handlelength=1.4,
        handleheight=1.2,
        ncol=1,
    )

    plt.tight_layout(rect=[0, 0.01, 1, 0.92])
    plt.savefig(out, dpi=150, bbox_inches="tight", facecolor=fig.get_facecolor())
    plt.close(fig)
    print(f"\n✅  Chart saved → {out}")


# ── main ──────────────────────────────────────────────────────────────────────


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Run all three sparkid comparison benchmarks and chart the results."
    )
    parser.add_argument("--out", default=DEFAULT_OUT, help="Output PNG path")
    parser.add_argument("--skip-js", action="store_true", help="Skip JS benchmark")
    parser.add_argument("--skip-python", action="store_true", help="Skip Python benchmark")
    parser.add_argument("--skip-rust", action="store_true", help="Skip Rust benchmark")
    args = parser.parse_args()

    js_raw = run_js() if not args.skip_js else ""
    py_raw = run_python() if not args.skip_python else ""
    rust_raw = run_rust() if not args.skip_rust else ""

    js = parse_js_python(js_raw) if js_raw else {}
    py = parse_js_python(py_raw) if py_raw else {}
    rust = parse_rust(rust_raw) if rust_raw else {}

    print(f"\n  JS results     : {js}")
    print(f"  Python results : {py}")
    print(f"  Rust results   : {rust}")

    if not any([js, py, rust]):
        sys.exit("No benchmark data collected — check the output above.")

    make_chart(js, py, rust, args.out)


if __name__ == "__main__":
    main()
