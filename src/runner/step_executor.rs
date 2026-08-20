// Skill-related step execution: methods used exclusively by UseSkill action.
// Extracted from execution.rs (Task 2).

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
}

// NOTE: Task 3 will add further skill-related methods (UseSkill handler logic) to this file.
// Keep pub(crate) on all methods.
