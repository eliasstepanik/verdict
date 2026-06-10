//! API design skill

use crate::guard::Guard;
use crate::skills::skill::Skill;
use crate::toolset::ToolSet;

/// Create the api_design skill
pub fn api_design() -> Skill {
    Skill {
        name: "api_design".to_string(),
        description: "Design clear, consistent, and safe API interfaces.".to_string(),
        instructions: r#"API Design Principles:
1. Keep interfaces simple and orthogonal.
2. Use consistent naming conventions throughout.
3. Make invalid states unrepresentable.
4. Document all public APIs clearly.
5. Consider backward compatibility.
6. Minimize the surface area.
7. Use strong types over strings.
8. Provide good error types.
9. Make common use cases ergonomic.
10. Test the API from a user's perspective."#
            .to_string(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![Guard::NonEmptyOutput],
        pipeline: None,
        examples: vec![],
        eval: None,
    }
}
