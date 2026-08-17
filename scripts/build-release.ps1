# Build release binary
Set-Location (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
cargo build -p verdict-app --release
$exitCode = $LASTEXITCODE
Write-Host "Build exit code: $exitCode"
exit $exitCode
