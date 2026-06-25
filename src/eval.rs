//! Evaluation suites — Phase 8
//! Full implementation of evaluation system for agent testing and validation

use crate::context::StepContext;
use crate::guards::{Guard, GuardEngine, GuardError};
use crate::runner::{PipelineRunner, PipelineResult};
use crate::pipeline::Pipeline;
use crate::agent::Agent;
use crate::llm::LlmClient;

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
                        session_meta: None,
                        cancellation_token: crate::cancel::CancellationToken::new(),
                        request_context: crate::context::RequestContext::new(),
                        memory: None,
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


// ============================================================================
// Phase F: Scorer Sampling, RubricLoop, and Experiment Runner
// ============================================================================

use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Trait for scoring pipeline results during online evaluation
#[async_trait]
pub trait Scorer: Send + Sync {
    /// Name of this scorer
    fn name(&self) -> &str;

    /// Score a pipeline result
    /// Returns a score (0.0–1.0) and whether it passes
    async fn score(&self, result: &PipelineResult) -> Result<ScorerResult, ScorerError>;
}

/// Result from a scorer
#[derive(Clone, Debug)]
pub struct ScorerResult {
    /// Score from 0.0 to 1.0
    pub score: f64,
    /// Whether the result passes this scorer's criteria
    pub pass: bool,
    /// Optional feedback from the scorer
    pub feedback: Option<String>,
}

/// Errors that can occur during scoring
#[derive(Error, Debug, Clone)]
pub enum ScorerError {
    #[error("scorer failed: {0}")]
    Failed(String),

    #[error("no LLM client available for scorer")]
    NoLlmClient,
}

/// Configuration for a scorer with sampling
#[derive(Clone)]
pub struct ScorerConfig {
    /// The scorer instance
    pub scorer: Arc<dyn Scorer>,
    /// Sampling rate (0.0–1.0): probability this scorer runs on each pipeline execution
    pub sampling_rate: f64,
}

impl std::fmt::Debug for ScorerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScorerConfig")
            .field("scorer", &self.scorer.name())
            .field("sampling_rate", &self.sampling_rate)
            .finish()
    }
}

/// Built-in scorer: checks if answer is relevant according to LLM
pub struct AnswerRelevancyScorer {
    pub llm_client: Arc<LlmClient>,
    pub threshold: f64,  // 0.0–1.0; pass if score >= threshold
}

#[async_trait]
impl Scorer for AnswerRelevancyScorer {
    fn name(&self) -> &str {
        "answer_relevancy"
    }

    async fn score(&self, result: &PipelineResult) -> Result<ScorerResult, ScorerError> {
        // Get the last step's output
        let output = result
            .steps_passed
            .last()
            .and_then(|step| result.step_results.get(step))
            .map(|sr| sr.output.raw.clone())
            .unwrap_or_default();

        if output.is_empty() {
            return Ok(ScorerResult {
                score: 0.0,
                pass: false,
                feedback: Some("No output to evaluate".into()),
            });
        }

        // Call LLM to evaluate relevancy (0-10 scale)
        let prompt = format!(
            "Rate the relevancy of this output on a scale of 0-10.\nOutput: {}\n\nRespond with just a number.",
            output
        );

        let req = crate::llm::LlmRequest {
            system: "You are an expert evaluator. Be objective and precise.".into(),
            user: prompt,
            model: String::new(),
            max_tokens: Some(10),
            history: None,
            temperature: None,
            tools: None,
            tool_choice: None,
        };

        match self.llm_client.complete(req).await {
            Ok(response) => {
                // Try to parse the response as a number 0-10
                let score_str = response.content.trim();
                match score_str.parse::<f64>() {
                    Ok(llm_score) => {
                        let normalized_score: f64 = (llm_score / 10.0).max(0.0).min(1.0);
                        let pass = normalized_score >= self.threshold;
                        Ok(ScorerResult {
                            score: normalized_score,
                            pass,
                            feedback: Some(format!("LLM score: {}/10", llm_score)),
                        })
                    }
                    Err(_) => Ok(ScorerResult {
                        score: 0.5,
                        pass: false,
                        feedback: Some("Could not parse LLM response as number".into()),
                    }),
                }
            }
            Err(e) => Err(ScorerError::Failed(format!("LLM call failed: {}", e))),
        }
    }
}

/// Built-in scorer: checks for toxic patterns
pub struct ToxicityScorer {
    pub blocked_patterns: Vec<String>,
}

impl ToxicityScorer {
    pub fn new() -> Self {
        Self {
            blocked_patterns: vec![],
        }
    }
}

impl Default for ToxicityScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Scorer for ToxicityScorer {
    fn name(&self) -> &str {
        "toxicity"
    }

    async fn score(&self, result: &PipelineResult) -> Result<ScorerResult, ScorerError> {
        let output = result
            .steps_passed
            .last()
            .and_then(|step| result.step_results.get(step))
            .map(|sr| sr.output.raw.clone())
            .unwrap_or_default();

        let output_lower = output.to_lowercase();

        for pattern in &self.blocked_patterns {
            if output_lower.contains(&pattern.to_lowercase()) {
                return Ok(ScorerResult {
                    score: 0.0,
                    pass: false,
                    feedback: Some(format!("Blocked pattern found: {}", pattern)),
                });
            }
        }

        Ok(ScorerResult {
            score: 1.0,
            pass: true,
            feedback: None,
        })
    }
}

/// Custom scorer using a closure
pub struct CustomScorer {
    pub name: String,
    pub func: Arc<dyn Fn(&PipelineResult) -> Result<ScorerResult, ScorerError> + Send + Sync>,
}

#[async_trait]
impl Scorer for CustomScorer {
    fn name(&self) -> &str {
        &self.name
    }

    async fn score(&self, result: &PipelineResult) -> Result<ScorerResult, ScorerError> {
        (self.func)(result)
    }
}

// ============================================================================
// Phase F: RubricLoop Step Action (helper types)
// ============================================================================

/// Item in a rubric for self-correction loops
#[derive(Clone, Debug)]
pub struct RubricItem {
    /// The criterion to evaluate
    pub criterion: String,
    /// Whether this criterion must pass for success
    pub required: bool,
}

// ============================================================================
// Phase F: Evaluation Dataset and Experiment Runner
// ============================================================================

/// A dataset of evaluation cases
#[derive(Clone)]
pub struct EvaluationDataset {
    pub name: String,
    pub version: u32,
    pub cases: Vec<EvaluationCase>,
}

impl EvaluationDataset {
    /// Create a new evaluation dataset
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: 1,
            cases: Vec::new(),
        }
    }

    /// Set the version
    pub fn with_version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// Add a test case
    pub fn add_case(mut self, case: EvaluationCase) -> Self {
        self.cases.push(case);
        self
    }
}

/// Result of running an experiment on a dataset
pub struct Experiment {
    pub name: String,
    pub dataset: EvaluationDataset,
    pub agent_name: String,
    pub run_at: DateTime<Utc>,
    pub results: Vec<EvaluationResult>,
    pub summary_score: f64,
}

/// Comparison between two experiments
pub struct ExperimentDiff {
    pub experiment_a: String,
    pub experiment_b: String,
    /// experiment_b.summary_score - experiment_a.summary_score
    pub score_delta: f64,
    /// Case names that improved
    pub improved_cases: Vec<String>,
    /// Case names that regressed
    pub regressed_cases: Vec<String>,
}

/// Runner for experiments on datasets
pub struct ExperimentRunner {
    pub runner: Arc<PipelineRunner>,
}

impl ExperimentRunner {
    /// Create a new experiment runner
    pub fn new(runner: Arc<PipelineRunner>) -> Self {
        Self { runner }
    }

    /// Run an experiment on a dataset with an agent
    pub async fn run_experiment(
        &self,
        name: impl Into<String>,
        dataset: EvaluationDataset,
        agent: &Agent,
    ) -> Result<Experiment, EvalError> {
        let mut results = Vec::new();
        
        for case in &dataset.cases {
            let mut runner_mut = (*self.runner).clone();
            
            let pipeline_result = runner_mut
                .run(&agent.pipeline, agent, case.input.clone())
                .await
                .map_err(|e| EvalError::Failed {
                    reason: format!("pipeline execution failed: {}", e),
                })?;

            // Determine pass/fail
            let last_step_name = pipeline_result
                .steps_passed
                .last()
                .map(|s| s.clone());

            let passed = last_step_name.is_some();

            results.push(EvaluationResult {
                case_name: case.name.clone(),
                passed,
                score: if passed { 1.0 } else { 0.0 },
                reason: None,
            });
        }

        let summary_score = if results.is_empty() {
            1.0
        } else {
            results.iter().map(|r| r.score).sum::<f64>() / results.len() as f64
        };

        Ok(Experiment {
            name: name.into(),
            dataset,
            agent_name: agent.name.clone(),
            run_at: Utc::now(),
            results,
            summary_score,
        })
    }

    /// Compare two experiments
    pub fn compare(a: &Experiment, b: &Experiment) -> ExperimentDiff {
        let mut improved_cases = Vec::new();
        let mut regressed_cases = Vec::new();

        let a_map: std::collections::HashMap<String, &EvaluationResult> = a
            .results
            .iter()
            .map(|r| (r.case_name.clone(), r))
            .collect();

        for b_result in &b.results {
            if let Some(a_result) = a_map.get(&b_result.case_name) {
                if b_result.score > a_result.score {
                    improved_cases.push(b_result.case_name.clone());
                } else if b_result.score < a_result.score {
                    regressed_cases.push(b_result.case_name.clone());
                }
            }
        }

        ExperimentDiff {
            experiment_a: a.name.clone(),
            experiment_b: b.name.clone(),
            score_delta: b.summary_score - a.summary_score,
            improved_cases,
            regressed_cases,
        }
    }
}