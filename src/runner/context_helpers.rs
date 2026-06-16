use crate::context::StepContext;

/// Resolve template placeholders in a prompt string.
///
/// Supported placeholders:
/// - `{input}` → the pipeline input value (as string)
/// - `{step_name}` → the raw output of the named prior step
///
/// WARNING: Template substitution uses sequential string replacement. If a step output
/// contains placeholder-like patterns (e.g., `{next_step_name}`), those patterns may be
/// substituted in subsequent iterations, potentially causing unexpected cascading replacements.
/// For safety, avoid step outputs that contain literal `{...}` patterns matching step names.
pub fn resolve_template(template: &str, ctx: &StepContext) -> String {
    let mut result = template.to_string();

    // Substitute {input}
    let input_str = match &ctx.input {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(map) => {
            // If input is {"task": "..."}, extract the "task" field preferentially
            if let Some(serde_json::Value::String(task)) = map.get("task") {
                task.clone()
            } else {
                ctx.input.to_string()
            }
        }
        v => v.to_string(),
    };
    result = result.replace("{input}", &input_str);

    // Substitute {step_name} for each prior step result
    for (step_name, step_result) in &ctx.step_results {
        let placeholder = format!("{{{}}}", step_name);
        result = result.replace(&placeholder, &step_result.output.raw);
    }

    result
}
