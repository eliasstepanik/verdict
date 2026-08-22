use crate::context::StepContext;
use jsonschema::JSONSchema;
use serde_json::Value;
use std::sync::OnceLock;

use super::Guard;
use super::GuardError;

static CL100K_BPE: OnceLock<Result<tiktoken_rs::CoreBPE, String>> = OnceLock::new();

fn get_cl100k_bpe() -> Result<&'static tiktoken_rs::CoreBPE, GuardError> {
    let cached = CL100K_BPE.get_or_init(|| tiktoken_rs::cl100k_base().map_err(|e| e.to_string()));
    cached.as_ref().map_err(|e| GuardError::Failed {
        guard: "CL100K_BPE".to_string(),
        reason: format!("failed to load CL100K BPE tokenizer: {}", e),
    })
}

pub fn validate_json(_guard: &Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        serde_json::from_str::<Value>(&output.raw)
            .map(|_| ())
            .map_err(|e| GuardError::Failed {
                guard: "ValidJson".to_string(),
                reason: e.to_string(),
            })
    } else {
        Err(GuardError::Failed {
            guard: "ValidJson".to_string(),
            reason: "no output to validate".to_string(),
        })
    }
}

pub fn validate_toml(_guard: &Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let text = &output.raw;
        match toml::from_str::<toml::Value>(text) {
            Ok(_) => Ok(()),
            Err(e) => Err(GuardError::Failed {
                guard: "ValidToml".to_string(),
                reason: format!("invalid TOML: {}", e),
            }),
        }
    } else {
        Err(GuardError::Failed {
            guard: "ValidToml".to_string(),
            reason: "no output available".to_string(),
        })
    }
}

pub fn validate_yaml(_guard: &Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let text = &output.raw;
        match serde_yaml::from_str::<serde_yaml::Value>(text) {
            Ok(_) => Ok(()),
            Err(e) => Err(GuardError::Failed {
                guard: "ValidYaml".to_string(),
                reason: format!("invalid YAML: {}", e),
            }),
        }
    } else {
        Err(GuardError::Failed {
            guard: "ValidYaml".to_string(),
            reason: "no output available".to_string(),
        })
    }
}

pub async fn validate_rust_syntax(_guard: &Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let text = &output.raw;

        // Check 1: Reject obvious non-Rust syntax
        let lines_lower: Vec<&str> = text.lines().collect();
        for line in &lines_lower {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("<html")
                || trimmed.starts_with("<?php")
                || trimmed.starts_with("def ")
                || trimmed.starts_with("class ")
            {
                return Err(GuardError::Failed {
                    guard: "ValidRustSyntax".to_string(),
                    reason: format!("output contains non-Rust syntax: {}", trimmed),
                });
            }
        }

        // Check 2: Balanced braces
        let open_braces = text.matches('{').count();
        let close_braces = text.matches('}').count();
        if open_braces > 0 && open_braces != close_braces {
            return Err(GuardError::Failed {
                guard: "ValidRustSyntax".to_string(),
                reason: format!(
                    "unbalanced braces: {} opening vs {} closing",
                    open_braces, close_braces
                ),
            });
        }

        // Check 3: Common Rust patterns
        let has_rust_pattern = text.contains("fn ")
            || text.contains("struct ")
            || text.contains("impl ")
            || text.contains("enum ")
            || text.contains("trait ")
            || text.contains("use ")
            || text.contains("mod ")
            || text.contains("pub ");

        if !has_rust_pattern {
            return Err(GuardError::Failed {
                guard: "ValidRustSyntax".to_string(),
                reason: "output does not contain Rust syntax patterns".to_string(),
            });
        }

        // Check 4: Try to run rustfmt --check via stdin if available
        use std::io::Write;
        match std::process::Command::new("rustfmt")
            .arg("--check")
            .arg("--edition=2021")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // rustfmt not installed — pass with note
                Ok(())
            }
            Err(e) => Err(GuardError::Failed {
                guard: "ValidRustSyntax".to_string(),
                reason: format!("rustfmt error: {}", e),
            }),
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                match child.wait_with_output() {
                    Ok(output) => {
                        if output.status.success() {
                            Ok(())
                        } else {
                            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                            Err(GuardError::Failed {
                                guard: "ValidRustSyntax".to_string(),
                                reason: format!("rustfmt check failed: {}", stderr),
                            })
                        }
                    }
                    Err(e) => Err(GuardError::Failed {
                        guard: "ValidRustSyntax".to_string(),
                        reason: format!("rustfmt failed: {}", e),
                    }),
                }
            }
        }
    } else {
        Err(GuardError::Failed {
            guard: "ValidRustSyntax".to_string(),
            reason: "no output available".to_string(),
        })
    }
}

pub fn validate_unified_diff(_guard: &Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let text = &output.raw;
        if text.starts_with("---") || text.starts_with("+++") || text.contains("@@") {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "OutputIsUnifiedDiff".to_string(),
                reason: "output does not look like a unified diff".to_string(),
            })
        }
    } else {
        Err(GuardError::Failed {
            guard: "OutputIsUnifiedDiff".to_string(),
            reason: "no output available".to_string(),
        })
    }
}

pub fn validate_max_tokens(
    _guard: &Guard,
    ctx: &StepContext,
    max: usize,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let bpe = get_cl100k_bpe()?;
        let token_count = bpe.encode_with_special_tokens(&output.raw).len();
        if token_count <= max {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "MaxTokens".to_string(),
                reason: format!("output has {} tokens, max is {}", token_count, max),
            })
        }
    } else {
        Err(GuardError::Failed {
            guard: "MaxTokens".to_string(),
            reason: "no output to count".to_string(),
        })
    }
}

pub fn validate_max_output_bytes(
    _guard: &Guard,
    ctx: &StepContext,
    max: usize,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        if output.raw.len() <= max {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "MaxOutputBytes".to_string(),
                reason: format!("output is {} bytes, max is {}", output.raw.len(), max),
            })
        }
    } else {
        Err(GuardError::Failed {
            guard: "MaxOutputBytes".to_string(),
            reason: "no output available".to_string(),
        })
    }
}

pub fn validate_non_empty(_guard: &Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        if output.raw.is_empty() {
            Err(GuardError::Failed {
                guard: "NonEmptyOutput".to_string(),
                reason: "output is empty".to_string(),
            })
        } else {
            Ok(())
        }
    } else {
        Err(GuardError::Failed {
            guard: "NonEmptyOutput".to_string(),
            reason: "no output available".to_string(),
        })
    }
}

pub fn validate_max_lines(_guard: &Guard, ctx: &StepContext, max: usize) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let line_count = output.raw.lines().count();
        if line_count <= max {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "MaxLines".to_string(),
                reason: format!("output has {} lines, max is {}", line_count, max),
            })
        }
    } else {
        Err(GuardError::Failed {
            guard: "MaxLines".to_string(),
            reason: "no output available".to_string(),
        })
    }
}

pub fn validate_schema(
    _guard: &Guard,
    ctx: &StepContext,
    schema: &Value,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let json: Value =
            serde_json::from_str(&output.raw).map_err(|e| GuardError::ParseError(e.to_string()))?;
        match JSONSchema::compile(schema) {
            Ok(validator) => match validator.validate(&json) {
                Ok(_) => Ok(()),
                Err(_e) => Err(GuardError::Failed {
                    guard: "MatchesSchema".to_string(),
                    reason: "output does not match schema".to_string(),
                }),
            },
            Err(e) => Err(GuardError::ParseError(e.to_string())),
        }
    } else {
        Err(GuardError::Failed {
            guard: "MatchesSchema".to_string(),
            reason: "no output to validate".to_string(),
        })
    }
}
#[cfg(test)] #[path = "output_tests.rs"] mod tests;
