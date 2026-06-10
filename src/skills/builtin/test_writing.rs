//! Test writing skill

use crate::guard::Guard;
use crate::skills::skill::Skill;
use crate::toolset::ToolSet;

/// Create the test_writing skill
pub fn test_writing() -> Skill {
    Skill {
        name: "test_writing".to_string(),
        description: "Write comprehensive unit and integration tests for Rust code.".to_string(),
        instructions: r#"When writing tests:
1. Identify all public functions and their edge cases.
2. Write unit tests for pure functions first.
3. Write integration tests for multi-component interactions.
4. Use descriptive test names: test_<function>_<scenario>.
5. Test both success paths and failure/error paths.
6. Use #[should_panic] or Result returns for error cases.
7. Avoid testing implementation details; test behavior.
8. Group related tests in a mod tests {} block."#
            .to_string(),
        allowed_tools: ToolSet::Allow(vec![
            "fs.read".to_string(),
            "fs.write".to_string(),
            "shell.cargo_test".to_string(),
        ]),
        required_guards: vec![Guard::TestsPass, Guard::NonEmptyOutput],
        pipeline: None,
        examples: vec![],
        eval: None,
    }
}
