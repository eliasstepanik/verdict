//! Skills system — Phase 5+
//! Reusable capabilities with instructions, tools, guards, and optional pipelines

pub mod builtin;
pub mod registry;
pub mod skill;

pub use skill::{Skill, SkillExample, SkillEval, SkillSet};
pub use registry::SkillRegistry;
