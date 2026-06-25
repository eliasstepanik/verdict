# Build release binary
Set-Location "C:\Users\Elias Stepanik\OpenCloud\Persönlich\Dev\Projecte\verdict"
cargo build -p verdict-app --release
$exitCode = $LASTEXITCODE
Write-Host "Build exit code: $exitCode"
exit $exitCode
