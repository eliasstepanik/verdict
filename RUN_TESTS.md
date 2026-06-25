# Stress Test Suite for Verdict Framework

## Prerequisites
Ensure you have:
- Rust toolchain installed (`cargo` available)
- `verdict-app` release binary built (see below)
- Current working directory: `C:\Users\Elias Stepanik\OpenCloud\Persönlich\Dev\Projecte\verdict`

## Step 1: Build Release Binary

```powershell
cargo build -p verdict-app --release
```

Expected output: Binary available at `.\target\release\verdict-app.exe`

## Step 2: Run Stress Tests

Execute all tests in sequence. The test format is:
```powershell
echo 'YOUR QUESTION' | .\target\release\verdict-app.exe 2>$null
```

### Test T1 — Basic file listing
```powershell
echo 'list all files in the current folder' | .\target\release\verdict-app.exe 2>$null
```
**Expected**: Lists real files (Cargo.toml, src/, tests/, verdict-app/, etc.), no error

### Test T2 — File reading
```powershell
echo 'read the contents of Cargo.toml' | .\target\release\verdict-app.exe 2>$null
```
**Expected**: Shows [package] section with name/version, no error

### Test T3 — Search/grep
```powershell
echo 'find all .rs files in the src folder' | .\target\release\verdict-app.exe 2>$null
```
**Expected**: Lists .rs files, no error

### Test T4 — Multi-step reasoning (no tools needed)
```powershell
echo 'explain what the Verdict framework is in 3 sentences' | .\target\release\verdict-app.exe 2>$null
```
**Expected**: Coherent explanation, no error (should NOT call tools)

### Test T5 — Code search
```powershell
echo 'search for the word PipelineRunner in any rust file' | .\target\release\verdict-app.exe 2>$null
```
**Expected**: Finds src files mentioning PipelineRunner

### Test T6 — Shell command
```powershell
echo 'run cargo check and tell me if the project compiles' | .\target\release\verdict-app.exe 2>$null
```
**Expected**: Runs cargo check, reports result

### Test T7 — Multi-file exploration
```powershell
echo 'look at src/lib.rs and tell me what modules are exported' | .\target\release\verdict-app.exe 2>$null
```
**Expected**: Reads the file, lists pub mod declarations

### Test T8 — Directory navigation
```powershell
echo 'explore the verdict-app directory structure' | .\target\release\verdict-app.exe 2>$null
```
**Expected**: Lists verdict-app/ contents, possibly reads some files

## Failure Criteria

Any response containing `[error:` or that is completely empty indicates a failure.

### Common failures and fixes:

1. **`[error: guard failed at step 'act' (Out): guard 'NonEmptyOutput' failed]`**
   - The synthesis call is not reaching the user. Check execution.rs synthesis path.
   - Rebuild and re-test.

2. **`[error: LLM client not configured]`**
   - The runner doesn't have an LLM client configured.
   - Ensure OPENAI_API_KEY environment variable is set.
   - Set it and re-test.

3. **Hallucinated responses (fake files, wrong paths)**
   - The tool schemas may not be reaching Claude.
   - Check tool registration in the runner.

## Verification

After all tests pass:
```powershell
cargo test --all
```

Expected: All tests pass with exit code 0.

## Debug Notes

All debug `eprintln!` statements have been removed from:
- `src/llm/provider.rs` — removed `[llm-req]` and `[llm-resp]` eprintlns
- `src/runner/execution.rs` — removed `[debug] round=`, `[debug] post-loop:`, `[debug] entering synthesis`, `[debug] synthesis response` eprintlns

The `UserInput` eprintln at line 874 in execution.rs was intentionally kept as it prompts the user.
