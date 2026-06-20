#!/usr/bin/env bash
#
# Axiom TypeScript SDK gate. The TS-native counterpart of scripts/coverage.sh —
# it holds packages/axiom-client (the @axiom/client SDK) to TS-native versions of
# the Static-Analysis, Branchless, and Coverage laws. See
# packages/axiom-client/STATIC_ANALYSIS.md and docs/ts-sdk-hardening.md.
#
# It runs, in order, failing on the first red:
#   1. typecheck  — tsgo (TypeScript 7.0 native) under the strict tsconfig
#   2. lint       — Oxlint, every category an error, plus the branch ban
#   3. coverage   — node:test built-in coverage at 100% lines/branches/functions
#
#   scripts/ts-gate.sh            # run the full gate
#
# Requires: npm + Node >= 24 (built-in TS + coverage thresholds). The deps live
# in packages/axiom-client/devDependencies; run `npm --prefix packages/axiom-client
# install` once first.
#
# NOTE (Phase 1): the SDK is not yet green — the branchless rewrite and the drive
# to 100% coverage are tracked in docs/ts-sdk-hardening.md. Until that lands this
# gate reports red; it is intentionally NOT wired into the blocking pre-commit
# hook or CI yet (exactly how the Rust spine ran its unbranching loop before the
# engine_no_branching gate went hard).
set -euo pipefail

pkg="packages/axiom-client"

# The gate is never silently skipped: a missing toolchain is a hard failure.
command -v npm >/dev/null 2>&1 || { echo 'npm is not installed.' >&2; exit 2; }
test -d "$pkg/node_modules" || { echo "deps missing — run: npm --prefix $pkg install" >&2; exit 2; }

echo "ts-gate [1/3] typecheck (tsgo / TypeScript 7.0) ..."
npm --prefix "$pkg" run --silent typecheck

echo "ts-gate [2/3] lint (Oxlint — all categories error + branch ban) ..."
npm --prefix "$pkg" run --silent lint

echo "ts-gate [3/3] coverage (100% gate) ..."
npm --prefix "$pkg" run --silent coverage

echo "ts-gate: all TypeScript gates green."
