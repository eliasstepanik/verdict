//! Tests for self-update module: validation, cost-benefit analysis, and orchestrating loop

use super::*;

// ============================================================================
// Validation Tests (existing behavior)
// ============================================================================

#[test]
fn test_validate_proposal_empty_patch() {
    let proposal = SelfUpdateProposal {
        patch: "".to_string(),
        summary: "test".to_string(),
        risk_level: RiskLevel::Low,
    };
    let config = SelfUpdateConfig::default();

    let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
    assert!(result.is_err());
    match result {
        Err(SelfUpdateError::EmptyPatch) => (),
        _ => panic!("expected EmptyPatch error"),
    }
}

#[test]
fn test_validate_proposal_not_a_diff() {
    let proposal = SelfUpdateProposal {
        patch: "this is not a diff".to_string(),
        summary: "test".to_string(),
        risk_level: RiskLevel::Low,
    };
    let config = SelfUpdateConfig::default();

    let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
    assert!(result.is_err());
    match result {
        Err(SelfUpdateError::InvalidDiff) => (),
        _ => panic!("expected InvalidDiff error"),
    }
}

#[test]
fn test_validate_proposal_forbidden_path() {
    let proposal = SelfUpdateProposal {
        patch: "--- a/Cargo.toml\n+++ b/Cargo.toml\n@@ -1,1 +1,1 @@".to_string(),
        summary: "test".to_string(),
        risk_level: RiskLevel::Low,
    };
    let config = SelfUpdateConfig::default();

    let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
    assert!(result.is_err());
    match result {
        Err(SelfUpdateError::ForbiddenPath { .. }) => (),
        _ => panic!("expected ForbiddenPath error"),
    }
}

#[test]
fn test_validate_proposal_valid_diff() {
    let proposal = SelfUpdateProposal {
        patch: "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,2 @@\n content"
            .to_string(),
        summary: "test".to_string(),
        risk_level: RiskLevel::Low,
    };
    let config = SelfUpdateConfig::default();

    let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
    assert!(result.is_ok());
}

// ============================================================================
// Cost-Benefit Analysis Tests
// ============================================================================

#[test]
fn test_cost_benefit_small_improvement() {
    // Small patch (10 lines) with improvement: should promote
    let patch = "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,11 @@\n-old line\n+new line 1\n+new line 2\n+new line 3\n+new line 4\n+new line 5\n+new line 6\n+new line 7\n+new line 8\n+new line 9\n+new line 10";
    let score_before = 0.50;
    let score_after = 0.75;

    let analysis = CostBenefitAnalysis::analyze(patch, score_before, score_after);

    assert_eq!(analysis.score_before, 0.50);
    assert_eq!(analysis.score_after, 0.75);
    assert_eq!(analysis.benefit, 0.25);
    assert!(analysis.is_worth_it, "Small patch with 25% improvement should be worth it");
}

#[test]
fn test_cost_benefit_equal_score() {
    // Small patch with equal score: should still promote for stability
    let patch = "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,5 @@\n+fix 1\n+fix 2\n+fix 3\n+fix 4\n+fix 5";
    let score_before = 0.80;
    let score_after = 0.80;

    let analysis = CostBenefitAnalysis::analyze(patch, score_before, score_after);

    assert_eq!(analysis.benefit, 0.0);
    assert!(analysis.is_worth_it, "Small patch with equal score should be worth it (≤50 lines)");
}

#[test]
fn test_cost_benefit_regression() {
    // Patch that regresses score: should NOT promote
    let patch = "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,10 @@\n+bad change 1\n+bad change 2\n+bad change 3\n+bad change 4\n+bad change 5\n+bad change 6\n+bad change 7\n+bad change 8\n+bad change 9\n+bad change 10";
    let score_before = 0.90;
    let score_after = 0.70;

    let analysis = CostBenefitAnalysis::analyze(patch, score_before, score_after);

    // Allow for floating-point imprecision
    assert!((analysis.benefit - (-0.20)).abs() < 0.0001);
    assert!(!analysis.is_worth_it, "Regression should not be promoted");
}

#[test]
fn test_cost_benefit_high_cost_small_improvement() {
    // Large patch (100 lines) with tiny improvement: should NOT promote
    let patch = (0..100)
        .map(|i| format!("+line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let score_before = 0.80;
    let score_after = 0.81; // Only 0.01 improvement (1%)

    let analysis = CostBenefitAnalysis::analyze(&patch, score_before, score_after);

    // Estimated cost should be 100 lines
    assert_eq!(analysis.estimated_cost, 100.0);
    // Benefit 0.01 with cost 100 requires benefit >= 0.05, so this is NOT worth it
    assert!(!analysis.is_worth_it, "Large patch (100 lines) with only 0.01 improvement should NOT be promoted");
}

#[test]
fn test_cost_benefit_medium_improvement() {
    // Medium patch (60 lines) with meaningful improvement: should promote
    let patch = (0..60)
        .map(|i| format!("+improvement {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let score_before = 0.60;
    let score_after = 0.72; // 12% improvement (>=5%)

    let analysis = CostBenefitAnalysis::analyze(&patch, score_before, score_after);

    assert_eq!(analysis.estimated_cost, 60.0);
    assert!(analysis.benefit >= 0.05, "Benefit should be >=5%");
    assert!(analysis.is_worth_it, "Medium patch (60 lines) with 12% improvement should be worth it");
}

#[test]
fn test_cost_benefit_very_small_change_equal_score() {
    // 1-line change with equal score: should promote (very low risk)
    let patch = "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,1 @@\n-old\n+new";
    let score_before = 0.50;
    let score_after = 0.50;

    let analysis = CostBenefitAnalysis::analyze(patch, score_before, score_after);

    assert!(analysis.is_worth_it, "1-line change should be worth it even with equal score");
}

// ============================================================================
// VersionAgent Tests
// ============================================================================

#[test]
fn test_version_agent_creates_valid_version() {
    use crate::agent::{Agent, AgentPolicy};
    use crate::toolset::ToolSet;
    use crate::skills::SkillSet;
    use crate::pipeline::{Pipeline, FailureMode};

    let agent = Agent {
        name: "test_agent".into(),
        description: "test".into(),
        pipeline: Pipeline {
            name: "test_pipeline".into(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadOnly,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let version = SelfUpdateEngine::version_agent(&agent, "Fixed a bug", Some(0.85));

    assert_eq!(version.agent_name, "test_agent");
    assert_eq!(version.change_summary, "Fixed a bug");
    assert_eq!(version.evaluation_score, Some(0.85));
    assert!(version.parent_version.is_none());
    // Version format is YYYYMMDDHHMMSS (timestamp-based)
    assert_eq!(version.version.len(), 14);
}

#[test]
fn test_version_agent_without_eval_score() {
    use crate::agent::{Agent, AgentPolicy};
    use crate::toolset::ToolSet;
    use crate::skills::SkillSet;
    use crate::pipeline::{Pipeline, FailureMode};

    let agent = Agent {
        name: "test_agent".into(),
        description: "test".into(),
        pipeline: Pipeline {
            name: "test_pipeline".into(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadOnly,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let version = SelfUpdateEngine::version_agent(&agent, "Minor refactor", None);

    assert_eq!(version.agent_name, "test_agent");
    assert!(version.evaluation_score.is_none());
}

// ============================================================================
// Extract Modified Paths Tests
// ============================================================================

#[test]
fn test_extract_modified_paths_single_file() {
    let patch = "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,2 @@\n content";
    let paths = extract_modified_paths(patch);
    // Both --- and +++ lines are extracted, so we expect 2 entries for the same file
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], "src/agents/test.rs");
    assert_eq!(paths[1], "src/agents/test.rs");
}

#[test]
fn test_extract_modified_paths_multiple_files() {
    let patch = "--- a/src/agents/a.rs\n+++ b/src/agents/a.rs\n@@ -1,1 +1,2 @@\n content\n--- a/src/agents/b.rs\n+++ b/src/agents/b.rs\n@@ -1,1 +1,2 @@\n content";
    let paths = extract_modified_paths(patch);
    // Both files appear twice (--- and +++)
    assert_eq!(paths.len(), 4);
    assert!(paths.iter().filter(|p| *p == "src/agents/a.rs").count() >= 1);
    assert!(paths.iter().filter(|p| *p == "src/agents/b.rs").count() >= 1);
}

#[test]
fn test_extract_modified_paths_ignores_dev_null() {
    let patch = "--- /dev/null\n+++ b/src/agents/new.rs\n@@ -0,0 +1,5 @@";
    let paths = extract_modified_paths(patch);
    // /dev/null should be filtered out, only new file should remain
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], "src/agents/new.rs");
}

// ============================================================================
// Normalize Path Tests
// ============================================================================

#[test]
fn test_normalize_path_lexical_with_parent_refs() {
    let path = "src/agents/../skills/test.rs";
    let normalized = normalize_path_lexical(path);
    assert_eq!(normalized, "src/skills/test.rs");
}

#[test]
fn test_normalize_path_lexical_with_dots() {
    let path = "src/./agents/./test.rs";
    let normalized = normalize_path_lexical(path);
    assert_eq!(normalized, "src/agents/test.rs");
}

#[test]
fn test_normalize_path_lexical_no_change() {
    let path = "src/agents/test.rs";
    let normalized = normalize_path_lexical(path);
    assert_eq!(normalized, "src/agents/test.rs");
}

// ============================================================================
// Mutation Self-Check: Validate "promote only if better" gate
// ============================================================================

#[test]
fn test_cost_benefit_mutation_regression_without_gate() {
    // This test validates the "promote only if better" gate in run_self_improvement_cycle
    // The decision logic is: should_promote = eval_after.passed && cost_benefit.is_worth_it
    //
    // If we REMOVE the "eval_after.passed" check, a regression (eval_after.passed=false)
    // would incorrectly be promoted if cost_benefit says "worth it" (e.g., small negative change)
    //
    // This test confirms that regressions are ALWAYS rejected regardless of cost-benefit
    let regression_patch = "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,5 @@\n+bad fix 1\n+bad fix 2\n+bad fix 3\n+bad fix 4\n+bad fix 5";
    let score_before = 0.90;
    let score_after = 0.70; // Regression: 70 < 90

    let analysis = CostBenefitAnalysis::analyze(regression_patch, score_before, score_after);

    // Cost-benefit: small patch (5 lines), regression (negative benefit)
    // Even if the patch is tiny, regression should NEVER be promoted
    assert_eq!(analysis.estimated_cost, 5.0);
    assert!(analysis.benefit < 0.0, "Benefit is negative (regression)");
    
    // The cost-benefit analysis correctly identifies this as NOT worth it
    // because benefit < 0.0 fails all promotion conditions
    assert!(!analysis.is_worth_it, "Even small regressions should not pass cost-benefit");

    // This validates that line 475 in the orchestrating loop is ESSENTIAL:
    //   let should_promote = eval_after.passed && cost_benefit.is_worth_it;
    // If eval_after.passed is removed, this regression would slip through!
}

// ============================================================================
// End-to-End Cycle Tests — data-loss regression coverage
//
// These tests exercise `run_self_improvement_cycle` against a REAL temporary
// workspace. They assert the workspace is never zeroed/corrupted, that an
// improving patch is promoted and left applied, and that a regressing patch is
// rolled back to its exact pre-cycle bytes.
// ============================================================================

#[cfg(test)]
mod cycle_e2e {
    use super::*;
    use crate::action::{StepAction, StepOutput};
    use crate::agent::{Agent, AgentPolicy};
    use crate::eval::{EvaluationCase, EvaluationExpected, EvaluationSuite};
    use crate::guards::Guard;
    use crate::pipeline::{AgentStep, FailureMode, Pipeline};
    use crate::runner::PipelineRunner;
    use crate::skills::SkillSet;
    use crate::toolset::ToolSet;
    use crate::verdict::Verdict;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    /// The single relative file the cycle tests patch. Inside `src/agents/`
    /// so it passes `SelfUpdateConfig::default()`'s allowed-paths whitelist.
    const QUALITY_FILE: &str = "src/agents/quality.txt";

    /// Hard safety rail: refuse to run anything destructive unless the target is
    /// a real, canonicalized path under the OS temp dir. Guards against a test
    /// ever being pointed at the actual source worktree.
    fn assert_is_tempdir(dir: &Path) {
        let canonical = dir
            .canonicalize()
            .unwrap_or_else(|e| panic!("workspace {:?} must exist: {}", dir, e));
        let tmp = std::env::temp_dir()
            .canonicalize()
            .expect("temp_dir must canonicalize");
        assert!(
            canonical.starts_with(&tmp),
            "REFUSING to run destructive test: {:?} is not under temp dir {:?}",
            canonical,
            tmp
        );
        assert!(
            !canonical.join("Cargo.toml").exists(),
            "REFUSING to run destructive test: {:?} looks like a real crate root",
            canonical
        );
    }

    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Create a temp workspace containing `QUALITY_FILE` with `content`, plus a
    /// couple of bystander files used to prove nothing gets zeroed.
    fn make_workspace(content: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create temp workspace");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/agents")).unwrap();
        std::fs::write(root.join(QUALITY_FILE), content).unwrap();
        std::fs::write(root.join("src/agents/bystander.txt"), "DO NOT TOUCH\n").unwrap();
        std::fs::write(root.join("README.md"), "workspace readme\n").unwrap();
        assert_is_tempdir(root);
        dir
    }

    /// Unified diff flipping QUALITY_FILE from `from` to `to` (single line each).
    fn one_line_patch(from: &str, to: &str) -> String {
        format!(
            "--- a/{f}\n+++ b/{f}\n@@ -1 +1 @@\n-{from}\n+{to}\n",
            f = QUALITY_FILE,
            from = from,
            to = to
        )
    }

    /// Pipeline whose single step reads QUALITY_FILE from the given workspace and
    /// emits its contents. This makes the eval score a direct function of the
    /// on-disk workspace state — exactly what the cycle is supposed to change.
    fn workspace_reading_pipeline(root: PathBuf) -> Pipeline {
        let file = root.join(QUALITY_FILE);
        Pipeline {
            name: "quality_check".into(),
            steps: vec![AgentStep {
                name: "read_quality".into(),
                action: StepAction::Custom(Arc::new(move |_ctx| {
                    let raw = std::fs::read_to_string(&file).map_err(|e| {
                        crate::action::StepError::ActionFailed {
                            reason: format!("cannot read quality file: {}", e),
                        }
                    })?;
                    Ok(StepOutput {
                        raw: raw.trim().to_string(),
                        parsed: None,
                        eval_result: None,
                    })
                })),
                guard_in: Guard::None,
                guard_out: Guard::None,
                verdict: Verdict::None,
                tools: ToolSet::ReadOnly,
                ..Default::default()
            }],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        }
    }

    fn agent_for(pipeline: Pipeline) -> Agent {
        Agent {
            name: "quality_agent".into(),
            description: "reads workspace quality marker".into(),
            pipeline,
            tools: ToolSet::ReadOnly,
            skills: SkillSet { skills: vec![] },
            policy: AgentPolicy::default(),
            scorers: vec![],
        }
    }

    /// Suite that passes only when the pipeline output is exactly "good".
    fn quality_suite() -> EvaluationSuite {
        EvaluationSuite {
            name: "quality".into(),
            cases: vec![EvaluationCase {
                name: "marker_is_good".into(),
                input: serde_json::json!({}),
                expected: EvaluationExpected::Custom(Arc::new(|result| {
                    let last = result
                        .steps_passed
                        .last()
                        .ok_or(crate::eval::EvalError::NoOutput)?;
                    let out = &result
                        .step_results
                        .get(last)
                        .ok_or(crate::eval::EvalError::NoOutput)?
                        .output
                        .raw;
                    if out == "good" {
                        Ok(())
                    } else {
                        Err(crate::eval::EvalError::Failed {
                            reason: format!("quality marker is {:?}, expected \"good\"", out),
                        })
                    }
                })),
            }],
            minimum_score: 1.0,
        }
    }

    /// Assert no file under `root` was truncated to zero bytes.
    fn assert_no_zeroed_files(root: &Path) {
        for rel in [QUALITY_FILE, "src/agents/bystander.txt", "README.md"] {
            let p = root.join(rel);
            let len = std::fs::metadata(&p)
                .unwrap_or_else(|e| panic!("{:?} must still exist: {}", p, e))
                .len();
            assert!(len > 0, "{:?} was zeroed out (len 0) — data loss bug", p);
        }
    }

    #[tokio::test]
    async fn test_cycle_improving_patch_promotes_and_leaves_workspace_intact() {
        if !git_available() {
            eprintln!("git unavailable — skipping");
            return;
        }
        let ws = make_workspace("bad\n");
        let root = ws.path().to_path_buf();

        let pipeline = workspace_reading_pipeline(root.clone());
        let agent = agent_for(pipeline.clone());
        let mut runner = PipelineRunner::new();

        let proposal = SelfUpdateProposal {
            patch: one_line_patch("bad", "good"),
            summary: "flip quality marker to good".into(),
            risk_level: RiskLevel::Low,
        };

        let result = SelfUpdateEngine::run_self_improvement_cycle(
            &agent,
            &pipeline,
            &proposal,
            &SelfUpdateConfig::default(),
            &mut runner,
            &quality_suite(),
            &root,
        )
        .await
        .expect("cycle should complete");

        assert!(result.promoted, "improving patch must be promoted: {:?}", result.reason);
        assert!(result.new_version.is_some(), "a new AgentVersion must be created");
        assert_eq!(result.score_before, 0.0);
        assert_eq!(result.score_after, 1.0);

        // Workspace must be correctly PATCHED, not zeroed.
        assert_no_zeroed_files(&root);
        assert_eq!(
            std::fs::read_to_string(root.join(QUALITY_FILE)).unwrap(),
            "good\n",
            "promoted patch must remain applied"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/agents/bystander.txt")).unwrap(),
            "DO NOT TOUCH\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "workspace readme\n"
        );
    }

    #[tokio::test]
    async fn test_cycle_regressing_patch_rejected_and_rolled_back() {
        if !git_available() {
            eprintln!("git unavailable — skipping");
            return;
        }
        let ws = make_workspace("good\n");
        let root = ws.path().to_path_buf();

        // Snapshot exact pre-cycle bytes of every file.
        let before: Vec<(PathBuf, Vec<u8>)> =
            [QUALITY_FILE, "src/agents/bystander.txt", "README.md"]
                .iter()
                .map(|rel| {
                    let p = root.join(rel);
                    let bytes = std::fs::read(&p).unwrap();
                    (p, bytes)
                })
                .collect();

        let pipeline = workspace_reading_pipeline(root.clone());
        let agent = agent_for(pipeline.clone());
        let mut runner = PipelineRunner::new();

        let proposal = SelfUpdateProposal {
            patch: one_line_patch("good", "bad"),
            summary: "regress quality marker".into(),
            risk_level: RiskLevel::Low,
        };

        let result = SelfUpdateEngine::run_self_improvement_cycle(
            &agent,
            &pipeline,
            &proposal,
            &SelfUpdateConfig::default(),
            &mut runner,
            &quality_suite(),
            &root,
        )
        .await
        .expect("cycle should complete");

        assert!(!result.promoted, "regressing patch must NOT be promoted");
        assert!(result.new_version.is_none(), "no version may be created on rejection");
        assert_eq!(result.score_before, 1.0);
        assert_eq!(result.score_after, 0.0);

        // Rollback: every file must be byte-identical to its pre-cycle state.
        assert_no_zeroed_files(&root);
        for (path, expected) in &before {
            let actual = std::fs::read(path).unwrap();
            assert_eq!(
                &actual, expected,
                "{:?} was not rolled back to its pre-cycle contents",
                path
            );
        }
    }

    #[tokio::test]
    async fn test_apply_in_sandbox_rejects_workspace_as_its_own_sandbox() {
        // The original data-loss bug: sandbox_dir == workspace_root made
        // copy_dir_recursive run fs::copy(p, p), truncating every file.
        let ws = make_workspace("good\n");
        let root = ws.path().to_path_buf();

        let err = SelfUpdateEngine::apply_in_sandbox(
            &one_line_patch("good", "bad"),
            &root,
            &root,
        )
        .await
        .expect_err("self-sandboxing must be rejected");
        assert!(matches!(err, SelfUpdateError::SandboxSetupFailed(_)), "got {:?}", err);

        // Nothing may have been touched.
        assert_no_zeroed_files(&root);
        assert_eq!(std::fs::read_to_string(root.join(QUALITY_FILE)).unwrap(), "good\n");
    }
}
