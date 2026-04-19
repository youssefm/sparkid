---
name: optimize-perf
description: Optimize performance of a specific function or code path. Use when the user asks to optimize, speed up, or make something faster. Triggers on 'optimize', 'speed up', 'make faster', 'optimize perf', 'improve performance'.
---

# Optimize Perf Skill

Systematically optimize a specific function or code path using benchmark-driven iteration.

## Workflow

### 1. Identify the target

Ask the user what function/code path to optimize and in which language (JS, Python, Rust), if not already clear from context.

### 2. Ensure a benchmark exists

Check if a benchmark already covers the target. If not, write a focused micro-benchmark for it using the project's existing benchmark infrastructure (Criterion for Rust, node:test-style timing loops for JS, timeit-style for Python). The benchmark must isolate the target — don't measure unrelated work.

### 3. Take a baseline

Run the benchmark once and record the result. This is the number to beat.

### 4. Brainstorm optimizations

Read the target code carefully. Come up with the **5 most promising optimization ideas**, ranked by expected impact. List them for the user with a one-line explanation each. Don't implement yet.

### 5. Try each optimization

For each idea, in order:

1. Implement the change on a clean copy of the code (revert the previous attempt first if it didn't help).
2. Run the tests to make sure nothing is broken.
3. Run the benchmark.
4. Compare to baseline. Record whether it helped, hurt, or was neutral.
5. If it helped, keep it and update the baseline. If not, revert it.

### 6. Report results

Summarize what worked and what didn't in a short table:

| # | Optimization | Result | Δ vs baseline |
|---|---|---|---|

Keep cumulative winners applied. Discard everything else.

## Rules

- **One change at a time.** Never stack untested optimizations.
- **Tests must pass** after every change. If tests fail, revert immediately.
- **Don't break invariants.** Read the repo's CLAUDE.md for invariants that must be preserved.
- **Revert cleanly.** Use `git checkout -- <file>` or `git stash` to revert failed attempts. Don't leave dead code.
- **Be honest about results.** If nothing helps, say so. Don't force marginal wins.
