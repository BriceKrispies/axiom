# Thin launcher for `ax` (tools/axiom-atlas) — the repo's query-and-change
# gateway. Execs the prebuilt release binary so an agent never pays `cargo run`
# overhead; builds it once if it is missing.
$root = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $root 'target\release\ax.exe'
if (-not (Test-Path $bin)) {
    Write-Host 'ax: building tools/axiom-atlas once...' -ForegroundColor DarkGray
    cargo build --release --manifest-path (Join-Path $root 'Cargo.toml') -p axiom-atlas
}
& $bin @args
exit $LASTEXITCODE
