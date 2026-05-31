#!/usr/bin/env bash
#
# Axiom coverage gate. See CLAUDE.md -> "The Axiom Coverage Law".
#
# Runs the FULL workspace test suite under llvm-cov instrumentation and FAILS
# unless every region, line, and function is covered (100%). When a nightly
# toolchain is present it also enables true branch-arm coverage (--branch), so
# the report's "Branches / Missed Branches" columns show the exact branches unit
# tests never exercised.
#
#   scripts/coverage.sh            # gate the workspace; print every uncovered line
#   scripts/coverage.sh --html     # also write an annotated HTML report
#   scripts/coverage.sh --open     # write the HTML report and open it
#
# Requires: cargo-llvm-cov   ->  cargo install cargo-llvm-cov
#           rustup nightly   ->  optional; enables --branch (true branch coverage)
set -euo pipefail

# The gate is never silently skipped: a missing tool is a hard failure.
if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo 'cargo-llvm-cov is not installed. Run: cargo install cargo-llvm-cov' >&2
  exit 2
fi

# Prefer nightly for true branch-arm coverage. Without it, region coverage still
# pins every branch arm, so the 100% gate holds; you just lose the "Branches"
# column.
toolchain=()
branch=()
if rustup toolchain list 2>/dev/null | grep -q nightly; then
  toolchain=(+nightly)
  branch=(--branch)
fi

# Report shape: a precise miss list by default; an annotated report on request.
report=(--show-missing-lines)
case "${1:-}" in
  --html) report=(--html) ;;
  --open) report=(--open) ;;
esac

# The Coverage Law governs the reusable engine spine: layers + modules. Apps are
# composition leaves (nothing depends on them) and xtask is repo tooling (outside
# the engine graph) — both sit outside the gate. This is a scope boundary, NOT a
# loophole: no layer or module file may ever be added here to dodge the gate.
exclude=(--ignore-filename-regex '[/\\](xtask|apps)[/\\]')

# The 100% gate. llvm-cov has no --fail-under-branches; regions are the
# branch-level enforceable proxy (one region per branch arm).
exec cargo "${toolchain[@]}" llvm-cov "${branch[@]}" --workspace "${exclude[@]}" "${report[@]}" \
  --fail-under-functions 100 \
  --fail-under-lines 100 \
  --fail-under-regions 100
