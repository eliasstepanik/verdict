pub mod observational;
pub mod semantic;
/// Memory tiers - high-level abstractions over MemoryStore
///
/// Each tier provides a specialized interface for a particular type of memory:
/// - ThreadMemory: conversation history
/// - WorkingMemory: structured JSON state
/// - SemanticMemory: embeddings and similarity search
/// - ObservationalMemory: LLM-compressed summaries
pub mod thread;
pub mod working;

pub use observational::ObservationalMemory;
pub use semantic::SemanticMemory;
pub use thread::ThreadMemory;
pub use working::WorkingMemory;
