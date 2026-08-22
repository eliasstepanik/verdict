use super::*;

#[test]
fn test_get_cl100k_bpe_returns_result() {
    // Test that get_cl100k_bpe now returns a Result instead of panicking
    let result = get_cl100k_bpe();
    assert!(
        result.is_ok(),
        "get_cl100k_bpe should return Ok on valid initialization: {:?}",
        result
    );
}

#[test]
fn test_get_cl100k_bpe_is_cached() {
    // Verify that OnceLock caches the result across calls
    let result1 = get_cl100k_bpe();
    let result2 = get_cl100k_bpe();

    // Both should succeed
    assert!(result1.is_ok());
    assert!(result2.is_ok());

    // Both should reference the exact same static instance
    if let (Ok(bpe1), Ok(bpe2)) = (result1, result2) {
        let ptr1 = bpe1 as *const tiktoken_rs::CoreBPE;
        let ptr2 = bpe2 as *const tiktoken_rs::CoreBPE;
        assert_eq!(ptr1, ptr2, "both calls should return the same cached instance");
    }
}

#[test]
fn test_get_cl100k_bpe_error_is_guard_error() {
    // Verify that the error type is GuardError::Failed (not a panic)
    let result = get_cl100k_bpe();
    if let Err(err) = result {
        // Check that we get a GuardError (not a panic or other error)
        match err {
            GuardError::Failed { guard, .. } => {
                assert_eq!(guard, "CL100K_BPE");
                // In success path, this is not reached, but the type system ensures
                // we're returning GuardError, not panicking.
            }
            _ => panic!("unexpected GuardError variant"),
        }
    }
    // On success, we just verify the type is correct
}
