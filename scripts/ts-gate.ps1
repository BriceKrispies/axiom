<#
.SYNOPSIS
  Axiom TypeScript SDK gate (PowerShell). The TS-native counterpart of
  scripts/coverage.ps1 — holds packages/axiom-client (the @axiom/client SDK) to
  TS-native versions of the Static-Analysis, Branchless, and Coverage laws. See
  packages/axiom-client/STATIC_ANALYSIS.md and docs/ts-sdk-hardening.md.

.DESCRIPTION
  Runs, in order, failing on the first red:
    1. typecheck  — tsgo (TypeScript 7.0 native) under the strict tsconfig
    2. lint       — Oxlint, every category an error, plus the branch ban
    3. coverage   — node:test built-in coverage at 100% lines/branches/functions

  Requires npm + Node >= 24. Deps live in packages/axiom-client/devDependencies;
  run `npm --prefix packages/axiom-client install` once first.

  NOTE (Phase 1): the SDK is not yet green — the branchless rewrite and the drive
  to 100% coverage are tracked in docs/ts-sdk-hardening.md. Until that lands this
  gate reports red and is intentionally NOT wired into the blocking pre-commit
  hook or CI (exactly how the Rust spine ran its unbranching loop before the
  engine_no_branching gate went hard).
#>
$ErrorActionPreference = 'Stop'
$pkg = 'packages/axiom-client'

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) { Write-Error 'npm is not installed.'; exit 2 }
if (-not (Test-Path "$pkg/node_modules")) { Write-Error "deps missing - run: npm --prefix $pkg install"; exit 2 }

function Step($n, $label, $script) {
  Write-Host "ts-gate [$n/3] $label ..."
  npm --prefix $pkg run --silent $script
  if ($LASTEXITCODE -ne 0) { Write-Error "ts-gate: $script FAILED - gate aborted."; exit 1 }
}

Step 1 'typecheck (tsgo / TypeScript 7.0)' 'typecheck'
Step 2 'lint (Oxlint - all categories error + branch ban)' 'lint'
Step 3 'coverage (100% gate)' 'coverage'

Write-Host 'ts-gate: all TypeScript gates green.'
