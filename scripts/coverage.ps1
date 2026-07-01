#!/usr/bin/env pwsh
#
# Axiom coverage gate. Runs the workspace test suite under llvm-cov and fails
# unless every region, line, and function is covered (100%). With a nightly
# toolchain it also enables true branch-arm coverage (`--branch`).
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

& cargo llvm-cov --version *> $null
if ($LASTEXITCODE -ne 0) {
    Write-Error 'cargo-llvm-cov is not installed. Run: cargo install cargo-llvm-cov'
    exit 2
}

# Without nightly, region coverage still pins every branch arm (each arm is its
# own region), so the 100% gate holds; you just lose the "Branches" column.
$toolchain = @()
$branch = @()
if ((rustup toolchain list 2>$null) -match 'nightly') {
    $toolchain = @('+nightly')
    $branch = @('--branch')
}

$report = @('--show-missing-lines')
if ($Open) { $report = @('--open') }
elseif ($Html) { $report = @('--html') }

# Must match exactly what coverage_scope.rs expects (CoverageIgnoreScriptDrift):
# apps/tools/xtask sit outside the gate, no layer or module file may be added here.
$exclude = @('--ignore-filename-regex', '[/\\](xtask|apps|axiom-zones|tools)[/\\]')

# llvm-cov has no --fail-under-branches; regions are the branch-level
# enforceable proxy (one region per branch arm).
$gate = @(
    '--fail-under-functions', '100',
    '--fail-under-lines', '100',
    '--fail-under-regions', '100'
)

& cargo @toolchain llvm-cov @branch --workspace @exclude @report @gate
exit $LASTEXITCODE
