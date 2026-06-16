use crate::context::StepContext;

use super::{
    Guard, GuardError, output, filesystem, compilation, budget, step_state,
    trace, tools, security, dependencies, diff, self_improve, delegation,
};

/// Engine for evaluating guards
pub struct GuardEngine;

impl GuardEngine {
    /// Evaluate a guard against a step context
    pub async fn evaluate(guard: &Guard, ctx: &StepContext) -> Result<(), GuardError> {
        match guard {
            Guard::None => Ok(()),

            Guard::Custom(f) => f(ctx),

            // Output guards
            Guard::ValidJson => output::validate_json(guard, ctx),
            Guard::ValidToml => output::validate_toml(guard, ctx),
            Guard::ValidYaml => output::validate_yaml(guard, ctx),
            Guard::ValidRustSyntax => output::validate_rust_syntax(guard, ctx).await,
            Guard::OutputIsUnifiedDiff => output::validate_unified_diff(guard, ctx),
            Guard::MaxTokens(max) => output::validate_max_tokens(guard, ctx, *max),
            Guard::MaxOutputBytes(max) => output::validate_max_output_bytes(guard, ctx, *max),
            Guard::NonEmptyOutput => output::validate_non_empty(guard, ctx),
            Guard::MaxLines(max) => output::validate_max_lines(guard, ctx, *max),
            Guard::MatchesSchema(schema) => output::validate_schema(guard, ctx, schema),

            // Filesystem guards
            Guard::FileExists(path) => filesystem::check_file_exists(guard, ctx, path),
            Guard::FileNotExists(path) => filesystem::check_file_not_exists(guard, ctx, path),
            Guard::FileContains { path, pattern } => filesystem::check_file_contains(guard, ctx, path, pattern),
            Guard::FileNotContains { path, pattern } => filesystem::check_file_not_contains(guard, ctx, path, pattern),
            Guard::PathWithinWorkspace => filesystem::check_path_within_workspace(guard, ctx),

            // Compilation guards
            Guard::Compiles => compilation::check_compiles(guard, ctx).await,
            Guard::TestsPass => compilation::check_tests_pass(guard, ctx).await,
            Guard::TestsPassWith(runner) => compilation::check_tests_pass_with(guard, ctx, runner).await,

            // Budget guards
            Guard::MaxCostUsd(max) => budget::check_max_cost_usd(guard, ctx, *max),
            Guard::MaxLlmCalls(max) => budget::check_max_llm_calls(guard, ctx, *max),
            Guard::MaxToolCalls(max) => budget::check_max_tool_calls(guard, ctx, *max),
            Guard::MaxDelegationDepth(max) => budget::check_max_delegation_depth(guard, ctx, *max),
            Guard::TimeoutSeconds(max) => budget::check_timeout_seconds(guard, ctx, *max),

            // Step state guards
            Guard::StepPassed(step_name) => step_state::check_step_passed(guard, ctx, step_name),
            Guard::StepFailed(step_name) => step_state::check_step_failed(guard, ctx, step_name),
            Guard::UserApproved(step_name) => step_state::check_user_approved(guard, ctx, step_name),
            Guard::PreviousStepMatchesSchema { step_name, schema } => {
                step_state::check_previous_step_matches_schema(guard, ctx, step_name, schema)
            }

            // Trace guards
            Guard::TraceAvailable => trace::check_trace_available(guard, ctx),
            Guard::AuditLogWritten => trace::check_audit_log_written(guard, ctx),

            // Tool guards
            Guard::NoForbiddenToolsUsed => tools::check_no_forbidden_tools(guard, ctx),
            Guard::OnlyAllowedToolsUsed => tools::check_only_allowed_tools(guard, ctx),
            Guard::ShellCommandAllowlist(cmds) => tools::check_shell_allowlist(guard, ctx, cmds),
            Guard::ShellCommandDenylist(cmds) => tools::check_shell_denylist(guard, ctx, cmds),

            // Security guards
            Guard::NoSecretsInOutput => security::check_no_secrets_in_output(guard, ctx),
            Guard::NoSecretsInDiff => security::check_no_secrets_in_diff(guard, ctx),
            Guard::NoSecretExfiltration => security::check_no_secret_exfiltration(guard, ctx),
            Guard::NoDangerousShellCommands => security::check_no_dangerous_shell_commands(guard, ctx),
            Guard::NoNewNetworkAccess => security::check_no_new_network_access(guard, ctx),
            Guard::NoPermissionEscalation => security::check_no_permission_escalation(guard, ctx),
            Guard::NoSafetyBypass => security::check_no_safety_bypass(guard, ctx),
            Guard::NoTestDisabling => security::check_no_test_disabling(guard, ctx),
            Guard::NoGuardRemoval => security::check_no_guard_removal(guard, ctx),

            // Dependency guards
            Guard::NoNewDependencies => dependencies::check_no_new_dependencies(guard, ctx),
            Guard::DependenciesAllowlist(allowed) => dependencies::check_dependencies_allowlist(guard, ctx, allowed),
            Guard::NoSuspiciousDependencies => dependencies::check_no_suspicious_dependencies(guard, ctx),
            Guard::CargoAuditPass => dependencies::check_cargo_audit_pass(guard, ctx).await,
            Guard::CargoDenyPass => dependencies::check_cargo_deny_pass(guard, ctx).await,

            // Diff guards
            Guard::MaxDiffLines(max) => diff::check_max_diff_lines(guard, ctx, *max),
            Guard::MaxChangedFiles(max) => diff::check_max_changed_files(guard, ctx, *max),
            Guard::DiffTouchesAllowedPaths(allowed) => diff::check_diff_touches_allowed_paths(guard, ctx, allowed),
            Guard::DiffDoesNotTouchForbiddenPaths(forbidden) => {
                diff::check_diff_does_not_touch_forbidden_paths(guard, ctx, forbidden)
            }

            // Self-improvement guards
            Guard::ReflectionHasActionableFinding => self_improve::check_reflection_has_actionable_finding(guard, ctx),
            Guard::PatchAppliesCleanly => self_improve::check_patch_applies_cleanly(guard, ctx),
            Guard::EvaluationImprovesOrEqual => self_improve::check_evaluation_improves_or_equal(guard, ctx),
            Guard::AgentVersionCreated => self_improve::check_agent_version_created(guard, ctx),
            Guard::NoActiveUncommittedCriticalChanges => {
                self_improve::check_no_active_uncommitted_critical_changes(guard, ctx)
            }

            // Delegation guards
            Guard::OnlyAllowedAgentsUsed => delegation::check_only_allowed_agents_used(guard, ctx),
            Guard::NoRecursiveDelegation => delegation::check_no_recursive_delegation(guard, ctx),
            Guard::DelegatedAgentPassed(agent_name) => delegation::check_delegated_agent_passed(guard, ctx, agent_name),

            // Composition guards
            Guard::AllOf(guards) => {
                for g in guards {
                    std::pin::Pin::from(Box::new(Self::evaluate(g, ctx))).await?;
                }
                Ok(())
            }

            Guard::AnyOf(guards) => {
                let mut last_err = GuardError::Failed {
                    guard: "AnyOf".to_string(),
                    reason: "no guards passed".to_string(),
                };
                for g in guards {
                    match std::pin::Pin::from(Box::new(Self::evaluate(g, ctx))).await {
                        Ok(()) => return Ok(()),
                        Err(e) => last_err = e,
                    }
                }
                Err(last_err)
            }

            Guard::Not(inner) => {
                match std::pin::Pin::from(Box::new(Self::evaluate(inner, ctx))).await {
                    Ok(()) => Err(GuardError::Failed {
                        guard: "Not".to_string(),
                        reason: "inner guard passed".to_string(),
                    }),
                    Err(_) => Ok(()),
                }
            }

            // Format & Lint
            Guard::LintPass => compilation::check_lint_pass(guard, ctx).await,
            Guard::FormatPass => compilation::check_format_pass(guard, ctx).await,
            Guard::SemanticCheck(description) => compilation::check_semantic_check(guard, ctx, description).await,
        }
    }
}

/// Guard evaluation phase (in or out)
#[derive(Debug, Clone, Copy)]
pub enum GuardPhase {
    /// Pre-condition guard (guard_in)
    In,
    /// Post-condition guard (guard_out)
    Out,
}
