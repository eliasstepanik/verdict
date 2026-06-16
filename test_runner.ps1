# PowerShell script to run tests and capture output
$testOutput = & cargo test 2>&1
$testOutput | Out-File -FilePath "test_output.txt" -Encoding UTF8
Write-Host "Test output saved to test_output.txt"
Write-Host $testOutput
