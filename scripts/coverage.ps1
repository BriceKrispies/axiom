#!/usr/bin/env pwsh
#
# Axiom coverage gate. See CLAUDE.md -> "The Axiom Coverage Law".
#
# Runs the FULL workspace test suite under llvm-cov instrumentation and FAILS
# unless every region, line, and function is covered (100%). When a nightly
# toolchain is present it also enables true branch-arm coverage (`--branch`),
# so the report's "Branches / Missed Branches" columns show the exact branches
# unit tests never exercised.
#
#   scripts/coverage.ps1            # gate the workspace; print every uncovered line
#   scripts/coverage.ps1 -Html     # also write an annotated HTML report under target/llvm-cov
#   scripts/coverage.ps1 -Open     # write the HTML report and open it in a browser
#
# Requires: cargo-llvm-cov   ->  cargo install cargo-llvm-cov
#           rustup nightly   ->  optional; enables --branch (true branch coverage)
[CmdletBinding()]
param(
    [switch]$Html,
    [switch]$Open
)

$ErrorActionPreference = 'Stop'

# The gate is never silently skipped: a missing tool is a hard failure.
& cargo llvm-cov --version *> $null
if ($LASTEXITCODE -ne 0) {
    Write-Error 'cargo-llvm-cov is not installed. Run: cargo install cargo-llvm-cov'
    exit 2
}

# Prefer nightly for true branch-arm coverage. Without it, region coverage still
# pins every branch arm (each arm is its own region), so the 100% gate holds;
# you just lose the dedicated "Branches" column.
$toolchain = @()
$branch = @()
if ((rustup toolchain list 2>$null) -match 'nightly') {
    $toolchain = @('+nightly')
    $branch = @('--branch')
}

# Report shape: a precise miss list by default; an annotated report on request.
$report = @('--show-missing-lines')
if ($Open) { $report = @('--open') }
elseif ($Html) { $report = @('--html') }

# The Coverage Law governs the reusable engine spine: layers + modules. Apps are
# composition leaves (nothing depends on them) and repo tooling (the xtask crate
# and anything under tools/) sits outside the engine graph — both sit outside the
# gate. This is a scope boundary, NOT a loophole: no layer or module file may ever
# be added here to dodge the gate.
$exclude = @('--ignore-filename-regex', '[/\\](xtask|apps|axiom-zones|tools)[/\\]')

# The 100% gate. llvm-cov has no --fail-under-branches; regions are the
# branch-level enforceable proxy (one region per branch arm).
$gate = @(
    '--fail-under-functions', '100',
    '--fail-under-lines', '100',
    '--fail-under-regions', '100'
)

& cargo @toolchain llvm-cov @branch --workspace @exclude @report @gate
exit $LASTEXITCODE
