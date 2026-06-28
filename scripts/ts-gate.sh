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
# The SDK is green (branchless, maximally linted, 100% covered) and this gate is
# wired into the pre-commit hook and CI as a hard gate. The remediation history is
# in docs/ts-sdk-hardening.md; the tool<->law mapping and documented exceptions are
# in packages/axiom-client/STATIC_ANALYSIS.md.
set -euo pipefail

# The two TypeScript packages held to the spine's TS-native laws: the netcode
# client SDK and the Phaser-style game-authoring SDK. Both run the same
# tsgo + Oxlint(branch-ban) + 100%-coverage stack.
pkgs=("packages/axiom-client" "packages/axiom-game")

# The gate is never silently skipped: a missing toolchain is a hard failure.
command -v npm >/dev/null 2>&1 || { echo 'npm is not installed.' >&2; exit 2; }

for pkg in "${pkgs[@]}"; do
  test -d "$pkg/node_modules" || { echo "deps missing — run: npm --prefix $pkg install" >&2; exit 2; }

  echo "ts-gate [$pkg 1/3] typecheck (tsgo / TypeScript 7.0) ..."
  npm --prefix "$pkg" run --silent typecheck

  echo "ts-gate [$pkg 2/3] lint (Oxlint — all categories error + branch ban) ..."
  npm --prefix "$pkg" run --silent lint

  echo "ts-gate [$pkg 3/3] coverage (100% gate) ..."
  npm --prefix "$pkg" run --silent coverage
done

echo "ts-gate: all TypeScript gates green."
