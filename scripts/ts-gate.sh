#!/usr/bin/env bash
#
# Axiom TypeScript SDK gate: holds each TS package to the TS gate stack (tsgo,
# Oxlint, node:test coverage). See packages/axiom-client/STATIC_ANALYSIS.md and
# docs/ts-sdk-hardening.md.
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
set -euo pipefail

# The netcode client SDK and the Phaser-style game-authoring SDK; both run the
# same tsgo + Oxlint(branch-ban) + 100%-coverage stack.
pkgs=("packages/axiom-client" "packages/axiom-game")

command -v npm >/dev/null 2>&1 || { echo 'npm is not installed.' >&2; exit 2; }

for pkg in "${pkgs[@]}"; do
  test -d "$pkg/node_modules" || { echo "deps missing — run: npm --prefix $pkg install" >&2; exit 2; }

  echo "ts-gate [$pkg 1/4] typecheck (tsgo / TypeScript 7.0) ..."
  npm --prefix "$pkg" run --silent typecheck

  echo "ts-gate [$pkg 2/4] lint (Oxlint — all categories error + branch ban) ..."
  npm --prefix "$pkg" run --silent lint

  echo "ts-gate [$pkg 3/4] co-location (every src file has a sibling *.test.ts) ..."
  npm --prefix "$pkg" run --silent colocation

  echo "ts-gate [$pkg 4/4] coverage (100% gate) ..."
  npm --prefix "$pkg" run --silent coverage
done

echo "ts-gate: all TypeScript gates green."
