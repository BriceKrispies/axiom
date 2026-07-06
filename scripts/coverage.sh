#!/usr/bin/env bash
#
# Axiom coverage gate. Runs the workspace test suite under llvm-cov and fails
# unless every region, line, and function is covered (100%). With a nightly
# toolchain it also enables true branch-arm coverage (--branch).
#
#   scripts/coverage.sh            # gate the workspace; print every uncovered line
#   scripts/coverage.sh --html     # also write an annotated HTML report
#   scripts/coverage.sh --open     # write the HTML report and open it
#
# Requires: cargo-llvm-cov   ->  cargo install cargo-llvm-cov
#           rustup nightly   ->  optional; enables --branch (true branch coverage)
set -euo pipefail

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo 'cargo-llvm-cov is not installed. Run: cargo install cargo-llvm-cov' >&2
  exit 2
fi

# Without nightly, region coverage still pins every branch arm, so the 100%
# gate holds; you just lose the "Branches" column.
toolchain=()
branch=()
if rustup toolchain list 2>/dev/null | grep -q nightly; then
  toolchain=(+nightly)
  branch=(--branch)
fi

report=(--show-missing-lines)
case "${1:-}" in
  --html) report=(--html) ;;
  --open) report=(--open) ;;
esac

# Must match exactly what coverage_scope.rs expects (CoverageIgnoreScriptDrift):
# apps/tools/xtask sit outside the gate, no layer or module file may be added here.
exclude=(--ignore-filename-regex '[/\\](xtask|apps|axiom-zones|tools)[/\\]')

# llvm-cov has no --fail-under-branches; regions are the branch-level
# enforceable proxy (one region per branch arm).
exec cargo "${toolchain[@]}" llvm-cov "${branch[@]}" --workspace "${exclude[@]}" "${report[@]}" \
  --fail-under-functions 100 \
  --fail-under-lines 100 \
  --fail-under-regions 100
