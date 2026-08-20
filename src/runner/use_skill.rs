// UseSkill action handler and related methods for skill invocation and evaluation.

use crate::action::StepOutput;
use crate::runner::PipelineRunner;

impl PipelineRunner {
    /// Build a system prompt by combining skill instructions with examples.
    ///
    /// If examples are provided, formats them as "Example N" sections with input/output/description.
    /// Otherwise returns instructions as-is.
    pub(crate) fn build_skill_system_prompt(
        &self,
        instructions: &str,
        examples: &[crate::skills::SkillExample],
    ) -> String {
        if examples.is_empty() {
            return instructions.to_string();
        }

        let mut prompt = instructions.to_string();
        prompt.push_str("\n\n");

        for (idx, example) in examples.iter().enumerate() {
            let example_num = idx + 1;
            prompt.push_str(&format!("Example {}:\n", example_num));
            prompt.push_str(&format!("Input: {}\n", example.input));
            prompt.push_str(&format!(
                "Expected Output: {}\n",
                example.expected_output
            ));
            prompt.push_str(&format!("Description: {}\n", example.description));
            if idx < examples.len() - 1 {
                prompt.push_str("\n");
            }
        }

        prompt
    }

    /// Run skill evaluation and return a formatted eval result string.
    ///
    /// Evaluation is informational (non-blocking) — results are attached to output
    /// for audit/visibility purposes but do not gate step success.
    pub(crate) async fn run_skill_eval(
        &self,
        skill_eval: &crate::skills::SkillEval,
        output: &StepOutput,
        skill_name: &str,
    ) -> String {
        // Simple evaluation: check if all criteria are mentioned in the output
        // More sophisticated evaluation (with scoring, LLM checks, etc.) can be added later
        let output_text = &output.raw;
        let mut met_criteria = 0;

        for criterion in &skill_eval.criteria {
            if output_text.contains(criterion) {
                met_criteria += 1;
            }
        }

        let criteria_count = skill_eval.criteria.len();
        let score = if criteria_count == 0 {
            1.0
        } else {
            met_criteria as f64 / criteria_count as f64
        };

        let threshold_check = if score < skill_eval.min_score {
            format!(" [BELOW threshold]")
        } else {
            String::new()
        };

        format!(
            "SkillEval[{}]: {}/{} criteria met (score: {:.2}, min: {:.2}){}",
            skill_name, met_criteria, criteria_count, score, skill_eval.min_score, threshold_check
        )
    }

    /// Handle UseSkill action.
    ///
    /// Applies skill instructions, validates skill guards, and runs skill pipeline or prompt.
    /// Implements FIX #1: narrows allowed_tools by skill's allowed_tools via intersection,
    /// then restores original scope after skill execution.
    pub(crate) async fn handle_use_skill(
        &mut self,
        skill: &str,
        mode: &crate::action::SkillMode,
        ctx: &mut crate::context::StepContext,
    ) -> Result<crate::action::StepOutput, crate::action::StepError> {
        use crate::guards::GuardEngine;

        let skill_def =
            self.skill_registry
                .get(skill)
                .ok_or_else(|| crate::action::StepError::ActionFailed {
                    reason: format!("Skill '{}' not found", skill),
                })?;

        // Check required guards
        for guard in &skill_def.required_guards {
            GuardEngine::evaluate(guard, ctx).await.map_err(
                |e: crate::guards::GuardError| crate::action::StepError::ActionFailed {
                    reason: format!("Skill required guard failed: {e}"),
                },
            )?;
        }

        // FIX #1: Narrow allowed_tools by skill's allowed_tools
        // Effective scope: agent ∩ pipeline ∩ step ∩ skill
        let saved_tools = ctx.allowed_tools.clone();
        ctx.allowed_tools = crate::toolset::ToolSet::Intersection(
            Box::new(ctx.allowed_tools.clone()),
            Box::new(skill_def.allowed_tools.clone()),
        );

        let result = match mode {
            crate::action::SkillMode::PromptOnly => {
                // If no LLM client, return the instructions directly
                if self.llm_client.is_none() {
                    Ok(crate::action::StepOutput::new(skill_def.instructions.clone()))
                } else {
                    // Inject skill instructions and few-shot examples into the system prompt
                    let system_prompt = self.build_skill_system_prompt(
                        &skill_def.instructions,
                        &skill_def.examples,
                    );

                    let llm_call = crate::action::StepAction::LlmCall {
                        system: system_prompt,
                        user: String::new(),
                        model: None,
                        conversation_id: None,
                        append_to_history: false,
                    };

                    self.execute_action(&llm_call, ctx).await
                }
            }

            crate::action::SkillMode::Pipeline | crate::action::SkillMode::Auto => {
                if let Some(pipeline) = &skill_def.pipeline {
                    // Run as a sub-pipeline
                    let sub_action = crate::action::StepAction::SubPipeline(Box::new(pipeline.clone()));
                    self.execute_action(&sub_action, ctx).await
                } else {
                    // Fall back to PromptOnly if no pipeline available (for both Pipeline and Auto modes)
                    // If no LLM client, return the instructions directly (same as PromptOnly without LLM)
                    if self.llm_client.is_none() {
                        Ok(crate::action::StepOutput::new(skill_def.instructions.clone()))
                    } else {
                        // Inject skill instructions and few-shot examples
                        let system_prompt = self.build_skill_system_prompt(
                            &skill_def.instructions,
                            &skill_def.examples,
                        );

                        let llm_call = crate::action::StepAction::LlmCall {
                            system: system_prompt,
                            user: String::new(),
                            model: None,
                            conversation_id: None,
                            append_to_history: false,
                        };
                        self.execute_action(&llm_call, ctx).await
                    }
                }
            }
        };

        // Restore original tool scope after skill execution
        ctx.allowed_tools = saved_tools;

        // FEATURE: Run skill evaluation if present and step succeeded
        let mut final_result = result?;
        if let Some(skill_eval) = &skill_def.eval {
            let eval_output =
                self.run_skill_eval(skill_eval, &final_result, &skill_def.name)
                    .await;
            // Attach eval result to output metadata (informational, non-blocking)
            final_result.set_eval_result(eval_output);
        }

        Ok(final_result)
    }
}
