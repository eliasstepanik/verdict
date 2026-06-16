//! Verdict: A Rust framework for building agents with guarded execution.
//!
//! Every step ends with a verdict. Hard guards, not soft prompts.

pub mod action;
pub mod agent;
pub mod agents;
pub mod audit;
pub mod budget;
pub mod context;
pub mod eval;
pub mod guards;
pub mod injection;
pub mod llm;
pub mod mcp;
pub mod pipeline;
pub mod prelude;
pub mod registry;
#[path = "runner/mod.rs"]
pub mod runner;
pub mod self_update;
pub mod skills;
pub mod tools;
pub mod toolset;
pub mod verdict;

pub use context::{ContextStore, ContextStoreError};
pub use prelude::*;
