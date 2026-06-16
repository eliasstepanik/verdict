//! Evaluation suites — Phase 8
//! Full implementation of evaluation system for agent testing and validation

use crate::context::StepContext;
use crate::guards::{Guard, GuardEngine, GuardError};
use crate::runner::{PipelineRunner, PipelineResult};
use crate::pipeline::Pipeline;
use crate::agent::Agent;
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

/// Error type for evaluation operations
#[derive(Error, Debug)]
pub enum EvalError {
    #[error("evaluation failed: {reason}")]
    Failed { reason: String },

    #[error("no output from pipeline")]
    NoOutput,
}

/// Expected output type for an evaluation case
#[derive(Clone)]
pub enum EvaluationExpected {
    /// Output must match exactly (string comparison)
    Exact(Value),

    /// Output must match JSON Schema
    Schema(Value),

    /// Output must pass a guard check
    Guard(Guard),

    /// Output validated by custom function
    Custom(Arc<dyn Fn(&PipelineResult) -> Result<(), EvalError> + Send + Sync>),
}

/// A single evaluation test case
#[derive(Clone)]
pub struct EvaluationCase {
    pub name: String,
    pub input: Value,
    pub expected: EvaluationExpected,
}

/// Result of evaluating a single case
#[derive(Debug, Clone)]
pub struct EvaluationResult {
    pub case_name: String,
    pub passed: bool,
    pub score: f64,
    pub reason: Option<String>,
}

/// Result of running a full evaluation suite
#[derive(Debug, Clone)]
pub struct EvaluationSuiteResult {
    pub suite_name: String,
    pub results: Vec<EvaluationResult>,
    pub overall_score: f64,
    pub passed: bool,
}

/// A suite of evaluation cases to test an agent's capabilities
pub struct EvaluationSuite {
    pub name: String,
    pub cases: Vec<EvaluationCase>,
    pub minimum_score: f64,
}

/// Engine for running evaluation suites
pub struct EvaluationRunner;

impl EvaluationRunner {
    /// Run an evaluation suite against a pipeline/agent
    pub async fn run_suite(
        suite: &EvaluationSuite,
        runner: &mut PipelineRunner,
        pipeline: &Pipeline,
        agent: &Agent,
    ) -> Result<EvaluationSuiteResult, EvalError> {
        let mut results = Vec::new();
        // Guard against empty evaluation suite
        if suite.cases.is_empty() {
            return Err(EvalError::Failed {
                reason: "EvaluationSuite has no test cases — misconfigured suite".into(),
            });
        }


        for case in &suite.cases {
            // Create a new context for this evaluation case
            let case_input = case.input.clone();
            
            // Run the pipeline with the case input
            let pipeline_result = runner
                .run(pipeline, agent, case_input.clone())
                .await
                .map_err(|e| EvalError::Failed {
                    reason: format!("pipeline execution failed: {}", e),
                })?;


            // Check if pipeline had output (get the last step result from steps_passed)
            let last_step_name = pipeline_result
                .steps_passed
                .last()
                .ok_or(EvalError::NoOutput)?;

            let last_output = pipeline_result
                .step_results
                .get(last_step_name)
                .ok_or(EvalError::NoOutput)?
                .output
                .clone();


            // Evaluate the case
            let (passed, reason): (bool, Option<String>) = match &case.expected {
                EvaluationExpected::Exact(expected_val) => {
                    // First try direct string comparison
                    if last_output.raw == expected_val.to_string() {
                        (true, None)
                    } else {
                        // Then try JSON-equality: parse output as JSON and compare
                        match serde_json::from_str::<Value>(&last_output.raw) {
                            Ok(parsed) => {
                                if &parsed == expected_val {
                                    (true, None)
                                } else {
                                    (false, Some(format!(
                                        "Expected {:?}, got: {}",
                                        expected_val,
                                        &last_output.raw[..100.min(last_output.raw.len())]
                                    )))
                                }
                            }
                            Err(_) => {
                                (false, Some(format!(
                                    "Expected {:?}, got: {}",
                                    expected_val,
                                    &last_output.raw[..100.min(last_output.raw.len())]
                                )))
                            }
                        }
                    }
                }

                EvaluationExpected::Schema(schema) => {
                    match serde_json::from_str::<Value>(&last_output.raw) {
                        Ok(parsed) => {
                            // Validate against schema
                            match jsonschema::JSONSchema::compile(schema) {
                                Ok(json_schema) => {
                                    match json_schema.validate(&parsed) {
                                        Ok(()) => (true, None),
                                        Err(_e) => (false, Some(
                                            "schema validation failed".to_string()
                                        )),
                                    }
                                }
                                Err(e) => (false, Some(format!(
                                    "invalid schema: {}",
                                    e
                                ))),
                            }
                        }
                        Err(e) => (false, Some(format!(
                            "output is not valid JSON: {}",
                            e
                        ))),
                    }
                }


                EvaluationExpected::Guard(guard) => {
                    // Build a context from the pipeline result with real step results and context
                    let last_step = agent.pipeline.steps.last()
                        .ok_or(EvalError::Failed { reason: "No steps in pipeline".into() })?;

                    let ctx = StepContext {
                        agent_name: agent.name.clone(),
                        pipeline_name: pipeline.name.clone(),
                        step_name: last_step.name.clone(),
                        step_id: last_step.name.clone(),
                        request: case_input.clone(),
                        input: case_input.clone(),
                        output: Some(last_output.clone()),
                        step_results: pipeline_result.step_results.clone(),
                        agent_registry: runner.agent_registry.clone(),
                        tool_registry: runner.tool_registry.clone(),
                        skill_registry: runner.skill_registry.clone(),
                        delegation_depth: 0,
                        parent_agent: None,
                        allowed_tools: agent.tools.clone(),
                        active_skills: agent.skills.skills.clone(),
                        trace: Default::default(),
                        budget: Default::default(),
                        filesystem_policy: agent.policy.filesystem_policy.clone(),
                        network_policy: agent.policy.network_policy.clone(),
                        llm_client: runner.llm_client.clone(),
                        conversation_history: Default::default(),
                        tools_used: vec![],
                    };

                    match GuardEngine::evaluate(guard, &ctx).await {
                        Ok(()) => (true, None),
                        Err(GuardError::Failed { reason, .. }) => {
                            (false, Some(format!("guard failed: {}", reason)))
                        }
                        Err(e) => (false, Some(format!("guard error: {}", e))),
                    }
                }


                EvaluationExpected::Custom(f) => {
                    match f(&pipeline_result) {
                        Ok(()) => (true, None),
                        Err(e) => (false, Some(e.to_string())),
                    }
                }
            };

            let score = if passed { 1.0 } else { 0.0 };

            results.push(EvaluationResult {
                case_name: case.name.clone(),
                passed,
                score,
                reason,
            });
        }

        // Calculate overall score
        let overall_score = if results.is_empty() {
            1.0
        } else {
            results.iter().map(|r| r.score).sum::<f64>() / results.len() as f64
        };

        let passed = overall_score >= suite.minimum_score;

        Ok(EvaluationSuiteResult {
            suite_name: suite.name.clone(),
            results,
            overall_score,
            passed,
        })
    }
}
