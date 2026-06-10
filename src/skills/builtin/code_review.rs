//! Code review skill

use crate::guard::Guard;
use crate::skills::skill::Skill;
use crate::toolset::ToolSet;

/// Create the code_review skill
pub fn code_review() -> Skill {
    Skill {
        name: "code_review".to_string(),
        description: "Review code changes for quality, safety, and correctness.".to_string(),
        instructions: r#"Code Review Process:
1. Read the diff or changed code carefully.
2. Check for logical errors and edge cases.
3. Verify safety: no unsafe blocks without justification.
4. Check naming consistency with the codebase.
5. Look for performance issues.
6. Ensure test coverage is adequate.
7. Provide constructive feedback and specific suggestions.
8. Focus on high-impact issues first."#
            .to_string(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![Guard::NonEmptyOutput],
        pipeline: None,
        examples: vec![],
        eval: None,
    }
}
