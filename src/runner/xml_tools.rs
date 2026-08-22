// XML and template utilities: template resolution and XML tool-call parsing/stripping.

use crate::context::StepContext;

/// Resolve template placeholders in a string using step context values.
///
/// Substitutes:
/// - `{input}` → ctx.input (or "task" field if object)
/// - `{step_name}` → prior step's output for that step name
///
/// NOTE: O(n²) performance when many placeholders exist — addressed in future optimization phase.
/// If a step output contains literal `{...}` patterns matching step names, those patterns may be
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
        let value_str = step_result.output.raw.clone();
        result = result.replace(&format!("{{{}}}", step_name), &value_str);
    }

    result
}

/// Strip XML tool-call artifacts that Claude sometimes halluccinates in synthesis responses.
/// Removes `<function_calls>...</function_calls>`, `<function_response>...</function_response>`, and `<invoke>...</invoke>` blocks.
pub fn strip_xml_tool_calls(text: &str) -> String {
    let mut result = text.to_string();
    // Remove <function_calls>...</function_calls> blocks (greedy, handles multiline)
    while let (Some(start), Some(end)) = (
        result.find("<function_calls>"),
        result.find("</function_calls>"),
    ) {
        if start <= end {
            result = format!(
                "{}{}",
                &result[..start],
                &result[end + "</function_calls>".len()..]
            );
        } else {
            break;
        }
    }
    // Remove <function_response>...</function_response> blocks
    while let (Some(start), Some(end)) = (
        result.find("<function_response>"),
        result.find("</function_response>"),
    ) {
        if start <= end {
            result = format!(
                "{}{}",
                &result[..start],
                &result[end + "</function_response>".len()..]
            );
        } else {
            break;
        }
    }
    // Remove <invoke>...</invoke> blocks (standalone XML tool calls)
    while let (Some(start), Some(end)) = (result.find("<invoke"), result.rfind("</invoke>")) {
        if start < end {
            result = format!("{}{}", &result[..start], &result[end + "</invoke>".len()..]);
        } else {
            break;
        }
    }
    // Collapse multiple blank lines
    let re_multi_blank = result
        .split('\n')
        .fold((String::new(), 0usize), |(mut acc, blanks), line| {
            if line.trim().is_empty() {
                if blanks < 1 {
                    acc.push('\n');
                }
                (acc, blanks + 1)
            } else {
                acc.push_str(line);
                acc.push('\n');
                (acc, 0)
            }
        })
        .0;
    re_multi_blank.trim().to_string()
}

/// Parse XML-format tool calls from text (Claude's legacy XML format).
///
/// Extracts all `<invoke name="TOOL_NAME">...</invoke>` blocks and converts them to (tool_name, args_json) pairs.
/// Each block should contain `<parameter name="KEY">VALUE</parameter>` entries.
///
/// Returns a Vec of (tool_name, args_json) pairs, or an empty Vec if no XML tool calls are found.
pub fn parse_xml_tool_calls(text: &str) -> Vec<(String, serde_json::Value)> {
    let mut result = Vec::new();

    // Find all <invoke name="..."> blocks
    let mut remaining = text;
    while let Some(start) = remaining.find("<invoke") {
        // Look for the closing > of the opening tag
        if let Some(tag_end) = remaining[start..].find('>') {
            let tag_end = start + tag_end;

            // Extract the opening tag to get the tool name
            let open_tag = &remaining[start..=tag_end];
            if let Some(name_start) = open_tag.find("name=\"") {
                let name_start = start + name_start + 6; // len("name=\"")
                if let Some(name_end) = remaining[name_start..].find('"') {
                    let tool_name = remaining[name_start..name_start + name_end].to_string();

                    // Find the closing </invoke> tag
                    if let Some(close_pos) = remaining[tag_end + 1..].find("</invoke>") {
                        let close_pos = tag_end + 1 + close_pos;
                        let block_content = &remaining[tag_end + 1..close_pos];

                        // Parse <parameter name="KEY">VALUE</parameter> entries
                        let mut args = serde_json::json!({});
                        let mut param_remaining = block_content;

                        while let Some(param_start) = param_remaining.find("<parameter") {
                            if let Some(param_tag_end) = param_remaining[param_start..].find('>') {
                                let param_tag_end = param_start + param_tag_end;
                                let param_tag = &param_remaining[param_start..=param_tag_end];

                                // Extract parameter name
                                if let Some(pname_start) = param_tag.find("name=\"") {
                                    let pname_start = param_start + pname_start + 6;
                                    if let Some(pname_end) =
                                        param_remaining[pname_start..].find('"')
                                    {
                                        let param_name = param_remaining
                                            [pname_start..pname_start + pname_end]
                                            .to_string();

                                        // Find the closing </parameter> tag
                                        if let Some(pclose_pos) = param_remaining
                                            [param_tag_end + 1..]
                                            .find("</parameter>")
                                        {
                                            let pclose_pos = param_tag_end + 1 + pclose_pos;
                                            let param_value = param_remaining
                                                [param_tag_end + 1..pclose_pos]
                                                .trim()
                                                .to_string();

                                            // Try to parse as JSON, otherwise treat as string
                                            if let Ok(json_val) = serde_json::from_str(&param_value)
                                            {
                                                args[&param_name] = json_val;
                                            } else {
                                                args[&param_name] =
                                                    serde_json::Value::String(param_value);
                                            }

                                            param_remaining = &param_remaining
                                                [pclose_pos + "</parameter>".len()..];
                                        } else {
                                            break;
                                        }
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }

                        result.push((tool_name, args));
                        remaining = &remaining[close_pos + "</invoke>".len()..];
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }

    result
}
