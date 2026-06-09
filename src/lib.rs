//! Verdict: A Rust framework for building agents with guarded execution.
//!
//! Every step ends with a verdict. Hard guards, not soft prompts.

pub mod action;
pub mod agent;
pub mod audit;
pub mod budget;
pub mod context;
pub mod eval;
pub mod guard;
pub mod injection;
pub mod llm;
pub mod mcp;
pub mod pipeline;
pub mod prelude;
pub mod registry;
pub mod runner;
pub mod self_update;
pub mod skills;
pub mod toolset;
pub mod tools;
pub mod verdict;

pub use prelude::*;
