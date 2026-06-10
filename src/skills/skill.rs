//! Core Skill type and related structures

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::guard::Guard;
use crate::pipeline::Pipeline;
use crate::toolset::ToolSet;

/// A reusable skill with instructions, tools, guards, and optional pipeline
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub allowed_tools: ToolSet,
    pub required_guards: Vec<Guard>,
    pub pipeline: Option<Pipeline>,
    pub examples: Vec<SkillExample>,
    pub eval: Option<SkillEval>,
}

/// Example input/output pair for a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillExample {
    pub input: Value,
    pub expected_output: String,
    pub description: String,
}

/// Evaluation criteria for a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEval {
    pub criteria: Vec<String>,
    pub min_score: f64,
}

/// A set of skill names
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSet {
    pub skills: Vec<String>,
}

impl Default for SkillSet {
    fn default() -> Self {
        Self {
            skills: vec![],
        }
    }
}

impl From<Vec<&str>> for SkillSet {
    fn from(skills: Vec<&str>) -> Self {
        Self {
            skills: skills.into_iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl SkillSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_skills(skills: Vec<String>) -> Self {
        Self { skills }
    }

    pub fn add(&mut self, skill: String) {
        if !self.skills.contains(&skill) {
            self.skills.push(skill);
        }
    }
}
