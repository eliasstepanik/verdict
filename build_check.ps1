$env:RUSTFLAGS = "-D warnings"
cargo test --workspace
Write-Host "EXIT:$LASTEXITCODE"
