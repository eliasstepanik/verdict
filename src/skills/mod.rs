//! Skills system — Phase 5+
//! Reusable capabilities with instructions, tools, guards, and optional pipelines

pub mod builtin;
pub mod registry;
pub mod skill;

pub use registry::SkillRegistry;
pub use skill::{Skill, SkillEval, SkillExample, SkillSet};
