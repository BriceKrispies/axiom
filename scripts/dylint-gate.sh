#!/usr/bin/env bash
#
# Axiom dylint rulebook gate. Runs the engine dylint rulebook over the whole
# workspace and fails if any engine lint rises above its ratchet baseline in
# tools/lints/dylint-baseline.txt.
#
#   scripts/dylint-gate.sh
#
# Lints are Warn-level, so `cargo dylint` exits 0 even on a finding. This gate
# therefore parses the findings itself: a driver/compile error is a hard failure,
# and a lint whose finding count exceeds its baseline is a regression. The gate
# passes only if every engine lint is at or below baseline — "add no new
# violations" without forcing the pre-existing, documented backlog to be fixed in
# the same change. This is the check the old .git/hooks/pre-commit ran; it now
# lives here and runs in CI (.github/workflows/ci.yml).
#
# Requires: cargo-dylint + dylint-link  ->  cargo install cargo-dylint dylint-link
#           the pinned nightly toolchain in tools/lints/rust-toolchain
#           (rustc-dev + llvm-tools-preview) that the lint drivers build against.
set -uo pipefail

# Run from the repo root so the workspace + baseline paths resolve regardless of
# the caller's cwd.
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

base_file="tools/lints/dylint-baseline.txt"

if ! cargo dylint --version >/dev/null 2>&1; then
  echo 'cargo-dylint is not installed. Run: cargo install cargo-dylint dylint-link' >&2
  exit 2
fi

echo "dylint-gate: running the engine rulebook over the workspace ..."
# A driver/compile error (non-zero exit) is a hard failure — the rulebook could
# not run, so we cannot certify the change.
if ! dyout="$(cargo dylint --all -- --all-targets 2>&1)"; then
  echo "$dyout" >&2
  echo "dylint-gate: FAILED — dylint driver error (the rulebook could not run)." >&2
  exit 1
fi

# Findings-per-lint, ratcheted against the baseline. A commit fails only if it
# RAISES any engine lint above its recorded baseline (pre-existing rustc warnings
# like dead_code are ignored — only the engine rulebook lints are counted).
cur="$(echo "$dyout" \
  | grep -oE '#\[warn\((engine_[a-z_]+|no_unwrap_in_engine|test_without_assertion)' \
  | sed -E 's/.*\(//' | sort | uniq -c | awk '{print $2"="$1}')"

regressed=0
for entry in $cur; do
  lint="${entry%%=*}"; n="${entry##*=}"
  allowed="$(grep -E "^${lint}=" "$base_file" 2>/dev/null | head -1 | cut -d= -f2)"
  allowed="${allowed:-0}"
  if [ "$n" -gt "$allowed" ]; then
    echo "  dylint: ${lint} = ${n} findings; baseline allows ${allowed}" >&2
    echo "$dyout" | grep -nE "warn\(${lint}\)" >&2
    regressed=1
  fi
done

if [ "$regressed" -ne 0 ]; then
  echo "dylint-gate: FAILED — engine lint findings above baseline (see tools/lints/dylint-baseline.txt)." >&2
  exit 1
fi

echo "dylint-gate: green — every engine lint is at or below baseline."
