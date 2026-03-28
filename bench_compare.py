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
    names = _order_names(all_names)  # used only to assign stable colours

    # Languages sorted fastest → slowest (determines group order top → bottom)
    langs_ordered = ["Rust", "JavaScript", "Python"]
    datasets_ordered = [rust, js, py]

    # For each language, sort its generators fastest → slowest so the longest
    # bar is at the top of each sub-group.
    per_lang: list[list[str]] = [
        sorted(data.keys(), key=lambda n: -data.get(n, 0.0))
        for data in datasets_ordered
    ]

    # One stable colour per generator
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

    # ── layout: one horizontal sub-group per language ─────────────────────────
    BAR_H = 0.55      # height of each bar
    GROUP_GAP = 1.0   # extra vertical space between language groups

    # Build Y positions from top to bottom: Rust group, then JS, then Python.
    # Within each group, generators are ordered fastest (top) to slowest (bottom).
    y_ticks_lang: list[float] = []   # centre Y of each language label
    bar_data: list[tuple[float, float, str, str, str]] = []  # (y, val, gen, lang, color)

    y = 0.0
    for li, (lang, data, gen_names) in enumerate(
        zip(langs_ordered, datasets_ordered, per_lang)
    ):
        group_start = y
        for gen in gen_names:
            val = data.get(gen, 0.0)
            bar_data.append((y, val, gen, lang, gen_colors.get(gen, "#94A3B8")))
            y -= BAR_H
        group_end = y + BAR_H  # last bar's top edge
        y_ticks_lang.append((group_start + group_end) / 2)
        if li < len(langs_ordered) - 1:
            y -= GROUP_GAP   # gap between groups

    all_y = [d[0] for d in bar_data]

    # ── figure setup ─────────────────────────────────────────────────────────
    FIG_BG = "#0d1117"
    AX_BG = "#161b22"
    GRID_COLOR = "#30363d"
    SPINE_COLOR = "#30363d"
    TEXT_COLOR = "#e6edf3"
    MUTED = "#8b949e"

    total_bars = len(bar_data)
    fig_h = max(9, total_bars * 0.55 + len(langs_ordered) * 0.5 + 2.5)
    fig, ax = plt.subplots(figsize=(16, fig_h))
    fig.patch.set_facecolor(FIG_BG)
    ax.set_facecolor(AX_BG)

    # ── draw horizontal bars ──────────────────────────────────────────────────
    plotted_gens: set[str] = set()
    for y_pos, val, gen, _lang, bar_c in bar_data:
        lw = 1.2 if gen == "sparkid" else 0.6
        alpha = 0.92 if gen == "sparkid" else 0.78
        bar = ax.barh(
            y_pos,
            val,
            BAR_H * 0.85,
            color=bar_c,
            edgecolor=bar_c,
            linewidth=lw,
            alpha=alpha,
            zorder=3,
            label=gen if gen not in plotted_gens else "_nolegend_",
        )
        plotted_gens.add(gen)

        if val > 0:
            if val >= 1e9:
                label = f"{val / 1e9:.1f}B"
            elif val >= 1e6:
                label = f"{val / 1e6:.1f}M"
            elif val >= 1e3:
                label = f"{val / 1e3:.0f}K"
            else:
                label = str(int(val))
            ax.text(
                val * 1.012,
                y_pos,
                label,
                ha="left",
                va="center",
                fontsize=8,
                fontweight="bold" if gen == "sparkid" else "normal",
                color=bar_c,
                zorder=5,
            )

    # ── axes & grid ───────────────────────────────────────────────────────────
    ax.xaxis.set_major_formatter(
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

    # Y-axis: language group labels at the vertical centre of each group
    ax.set_yticks(y_ticks_lang)
    ax.set_yticklabels(langs_ordered, fontsize=14, fontweight="bold", color=TEXT_COLOR)
    ax.tick_params(axis="y", length=0, pad=10)
    ax.tick_params(axis="x", colors=MUTED, labelsize=10)
    ax.set_xlim(left=0)
    ax.set_ylim(min(all_y) - BAR_H, max(all_y) + BAR_H)

    ax.set_xlabel("IDs / second  (higher is better →)", fontsize=12, color=MUTED, labelpad=12)

    ax.grid(axis="x", color=GRID_COLOR, linewidth=0.7, zorder=0)
    ax.set_axisbelow(True)

    # Draw a subtle horizontal separator between language groups
    for i in range(len(langs_ordered) - 1):
        mid = (y_ticks_lang[i] + y_ticks_lang[i + 1]) / 2
        ax.axhline(mid, color=SPINE_COLOR, linewidth=1.0, zorder=1)

    for spine in ax.spines.values():
        spine.set_edgecolor(SPINE_COLOR)

    # ── title ─────────────────────────────────────────────────────────────────
    fig.text(
        0.5, 0.98,
        "SparkID — ID Generator Benchmark",
        ha="center", va="top",
        fontsize=20, fontweight="bold", color=TEXT_COLOR,
    )
    fig.text(
        0.5, 0.945,
        "Median throughput per language  ·  sparkid vs. alternatives  |  Higher is better",
        ha="center", va="top",
        fontsize=11, color=MUTED,
    )

    # ── legend ────────────────────────────────────────────────────────────────
    ax.legend(
        fontsize=11,
        loc="lower right",
        framealpha=0.4,
        edgecolor=SPINE_COLOR,
        facecolor=AX_BG,
        labelcolor=TEXT_COLOR,
        handlelength=1.4,
        handleheight=1.2,
        ncol=1,
    )

    plt.tight_layout(rect=[0, 0.01, 1, 0.93])
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
