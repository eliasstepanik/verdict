//! Agent delegation — Phase 4
//!
//! Implements `execute_delegation()`: policy checks, child-runner setup,
//! result merging, schema validation, and audit events.

use super::PipelineRunner;
use crate::action::{DelegationPolicy, StepError, StepOutput};
use crate::audit::{AuditEntry, AuditEvent};
use crate::context::{StepContext, TraceEntry};
use crate::registry::ToolRegistry;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

impl PipelineRunner {
    /// Execute delegation to a named agent — Phase 4
    pub(crate) async fn execute_delegation(
        &mut self,
        agent_name: &str,
        delegate_input: &Value,
        expected_output_schema: Option<&Value>,
        delegation_policy: &DelegationPolicy,
        ctx: &mut StepContext,
    ) -> Result<StepOutput, StepError> {
        // Step 1: Check max delegation depth
        if ctx.delegation_depth >= delegation_policy.max_depth {
            self.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: ctx.step_name.clone(),
                event: AuditEvent::DelegationFailed {
                    parent_agent: ctx.agent_name.clone(),
                    child_agent: agent_name.to_string(),
                    depth: ctx.delegation_depth,
                    reason: format!(
                        "Max delegation depth {} exceeded",
                        delegation_policy.max_depth
                    ),
                },
            });

            return Err(StepError::ActionFailed {
                reason: format!(
                    "Delegation depth {} exceeds max {}",
                    ctx.delegation_depth, delegation_policy.max_depth
                ),
            });
        }

        // Step 2: Check allowed agents list
        if !delegation_policy.allowed_agents.is_empty()
            && !delegation_policy
                .allowed_agents
                .contains(&agent_name.to_string())
        {
            self.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: ctx.step_name.clone(),
                event: AuditEvent::DelegationFailed {
                    parent_agent: ctx.agent_name.clone(),
                    child_agent: agent_name.to_string(),
                    depth: ctx.delegation_depth,
                    reason: format!("Agent '{}' not in allowed list", agent_name),
                },
            });

            return Err(StepError::ActionFailed {
                reason: format!("Agent '{}' not in allowed_agents list", agent_name),
            });
        }

        // Step 2.5: Check require_output_schema
        if delegation_policy.require_output_schema && expected_output_schema.is_none() {
            self.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: ctx.step_name.clone(),
                event: AuditEvent::DelegationFailed {
                    parent_agent: ctx.agent_name.clone(),
                    child_agent: agent_name.to_string(),
                    depth: ctx.delegation_depth,
                    reason: "require_output_schema is true but no schema provided".into(),
                },
            });

            return Err(StepError::ActionFailed {
                reason: "Delegation requires an output schema but none was provided".into(),
            });
        }

        // Step 2.6: Check require_user_approval
        if delegation_policy.require_user_approval {
            self.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: ctx.step_name.clone(),
                event: AuditEvent::DelegationFailed {
                    parent_agent: ctx.agent_name.clone(),
                    child_agent: agent_name.to_string(),
                    depth: ctx.delegation_depth,
                    reason: "Delegation requires user approval".into(),
                },
            });

            return Err(StepError::ActionFailed {
                reason: "Delegation requires user approval before proceeding".into(),
            });
        }

        // Step 3: Look up agent in registry
        let agent = self.agent_registry.get(agent_name).ok_or_else(|| {
            self.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: ctx.step_name.clone(),
                event: AuditEvent::DelegationFailed {
                    parent_agent: ctx.agent_name.clone(),
                    child_agent: agent_name.to_string(),
                    depth: ctx.delegation_depth,
                    reason: "Agent not found in registry".into(),
                },
            });
            StepError::ActionFailed {
                reason: format!("Agent '{}' not found in registry", agent_name),
            }
        })?;

        // Step 4: Log delegation start
        self.audit_log.append(AuditEntry {
            timestamp: Utc::now(),
            pipeline_name: ctx.pipeline_name.clone(),
            step_name: ctx.step_name.clone(),
            event: AuditEvent::DelegationStarted {
                parent_agent: ctx.agent_name.clone(),
                child_agent: agent_name.to_string(),
                depth: ctx.delegation_depth + 1,
            },
        });

        // Step 5: Create child runner with inherited registries
        let child_tool_registry = if delegation_policy.inherit_tool_scope {
            self.tool_registry.clone()
        } else {
            Arc::new(ToolRegistry::with_builtins())
        };

        let mut child_runner = PipelineRunner {
            audit_log: crate::audit::AuditLog::new(),
            tool_registry: child_tool_registry,
            agent_registry: self.agent_registry.clone(),
            skill_registry: self.skill_registry.clone(),
            llm_client: self.llm_client.clone(),
            output_sink: self.output_sink.clone(),
            conversation_registry: self.conversation_registry.clone(),
            context_store: self.context_store.clone(),
            plugin_registry: self.plugin_registry.clone(),
            auto_title_llm: self.auto_title_llm.clone(),
            memory: self.memory.clone(),
        };

        // Step 6: Run child agent pipeline with increased delegation depth
        let inherited_budget = if delegation_policy.inherit_budget {
            Some(ctx.budget.clone())
        } else {
            None
        };

        let child_result = child_runner
            .run_with_delegation_depth_and_budget(
                &agent.pipeline,
                &agent,
                delegate_input.clone(),
                ctx.delegation_depth + 1,
                ctx.agent_name.clone(),
                inherited_budget,
            )
            .await;

        match child_result {
            Ok(result) => {
                // Step 7: Merge child step results into parent
                for (step_name, step_result) in &result.step_results {
                    let namespaced_key = format!("{}.{}", agent_name, step_name);
                    ctx.step_results.insert(namespaced_key, step_result.clone());
                }

                // Propagate child's budget back to parent if inherit_budget is true
                if delegation_policy.inherit_budget {
                    ctx.budget = result.budget.clone();
                }

                // Merge trace entries
                ctx.trace
                    .entries
                    .extend(result.audit_log.entries().iter().filter_map(|entry| {
                        if let crate::audit::AuditEvent::StepCompleted { .. } = entry.event {
                            Some(TraceEntry {
                                step_name: entry.step_name.clone(),
                                status: "delegated".to_string(),
                                timestamp: entry.timestamp,
                            })
                        } else {
                            None
                        }
                    }));

                // Merge audit log entries
                for entry in result.audit_log.entries() {
                    self.audit_log.append(entry.clone());
                }

                // Step 8: Validate output schema if specified
                if let Some(schema) = expected_output_schema {
                    let last_output = result
                        .step_results
                        .values()
                        .last()
                        .map(|sr| &sr.output.raw)
                        .ok_or_else(|| StepError::ActionFailed {
                            reason: "Delegated agent produced no output".into(),
                        })?;

                    if let Ok(parsed) = serde_json::from_str::<Value>(last_output) {
                        match jsonschema::JSONSchema::compile(schema) {
                            Ok(validator) => {
                                if let Err(e) = validator.validate(&parsed) {
                                    let errors: Vec<_> = e.collect();
                                    return Err(StepError::ActionFailed {
                                        reason: format!(
                                            "Delegated output does not match schema: {} validation errors",
                                            errors.len()
                                        ),
                                    });
                                }
                            }
                            Err(e) => {
                                return Err(StepError::ActionFailed {
                                    reason: format!("Invalid output schema: {}", e),
                                });
                            }
                        }
                    } else {
                        return Err(StepError::ActionFailed {
                            reason: "Delegated output is not valid JSON for schema validation"
                                .into(),
                        });
                    }
                }

                // Log delegation completion
                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: ctx.pipeline_name.clone(),
                    step_name: ctx.step_name.clone(),
                    event: AuditEvent::DelegationCompleted {
                        parent_agent: ctx.agent_name.clone(),
                        child_agent: agent_name.to_string(),
                        depth: ctx.delegation_depth + 1,
                    },
                });

                let agent_output = result
                    .step_results
                    .values()
                    .last()
                    .map(|sr| sr.output.clone())
                    .unwrap_or_else(|| StepOutput::new(String::new()));

                // Return a summary that includes the agent name
                let summary = format!(
                    "Delegation to '{}' completed. Output: {}",
                    agent_name, agent_output.raw
                );
                Ok(StepOutput::new(summary))
            }
            Err(e) => {
                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: ctx.pipeline_name.clone(),
                    step_name: ctx.step_name.clone(),
                    event: AuditEvent::DelegationFailed {
                        parent_agent: ctx.agent_name.clone(),
                        child_agent: agent_name.to_string(),
                        depth: ctx.delegation_depth + 1,
                        reason: e.to_string(),
                    },
                });

                Err(StepError::ActionFailed {
                    reason: format!("Delegation to '{}' failed: {}", agent_name, e),
                })
            }
        }
    }
}
