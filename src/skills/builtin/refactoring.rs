//! Refactoring skill

use crate::guard::Guard;
use crate::skills::skill::Skill;
use crate::toolset::ToolSet;

/// Create the refactoring skill
pub fn refactoring() -> Skill {
    Skill {
        name: "refactoring".to_string(),
        description: "Refactor code for clarity, maintainability, and performance without changing behavior.".to_string(),
        instructions: r#"When refactoring:
1. Run all tests before starting — ensure green baseline.
2. Make one small change at a time.
3. Run tests after each change.
4. Do not change external behavior or public API signatures.
5. Extract repeated logic into named functions.
6. Simplify nested conditions using early returns.
7. Rename variables and functions for clarity.
8. Remove dead code only after confirming it is unreachable.
9. Document non-obvious logic with inline comments."#
            .to_string(),
        allowed_tools: ToolSet::Allow(vec![
            "fs.read".to_string(),
            "fs.write".to_string(),
            "shell.cargo_check".to_string(),
            "shell.cargo_test".to_string(),
        ]),
        required_guards: vec![Guard::Compiles, Guard::TestsPass],
        pipeline: None,
        examples: vec![],
        eval: None,
    }
}
