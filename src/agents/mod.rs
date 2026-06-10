//! Built-in agents for the Verdict framework
//!
//! This module provides six specialized agents:
//! - `planner_agent`: Produces structured execution plans
//! - `coder_agent`: Implements approved software changes
//! - `reviewer_agent`: Reviews code changes for quality and safety
//! - `debugger_agent`: Diagnoses and fixes compile/test failures
//! - `reflector_agent`: Analyzes agent performance
//! - `orchestrator_agent`: Delegates work to specialized agents

pub mod planner;
pub mod coder;
pub mod reviewer;
pub mod debugger;
pub mod reflector;
pub mod orchestrator;

pub use planner::planner_agent;
pub use coder::coder_agent;
pub use reviewer::reviewer_agent;
pub use debugger::debugger_agent;
pub use reflector::reflector_agent;
pub use orchestrator::orchestrator_agent;
