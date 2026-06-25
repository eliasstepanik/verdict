# Verdict vs. Mastra — Competitive Analysis & Improvement Roadmap

> **Goal**: Make Verdict the most secure, production-ready, and expressive
> Rust-native agent framework — beating Mastra where it matters most.

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Framework Comparison](#2-framework-comparison)
   - 2.1 [Core Philosophy](#21-core-philosophy)
   - 2.2 [Strengths of Verdict](#22-strengths-of-verdict)
   - 2.3 [Weaknesses of Verdict / Strengths of Mastra](#23-weaknesses-of-verdict--strengths-of-mastra)
3. [Feature Gap Analysis](#3-feature-gap-analysis)
4. [Improvement Plan](#4-improvement-plan)
   - Phase A — Quick Wins (0–2 weeks)
   - Phase B — Core DX & Developer Surface (2–6 weeks)
   - Phase C — Memory, Storage, RAG (6–10 weeks)
   - Phase D — Multi-Agent & Orchestration (10–14 weeks)
   - Phase E — Observability & Deployment (14–18 weeks)
   - Phase F — Evaluation & Self-Improvement Polish (18–22 weeks)
5. [Anti-Mastra Differentiators](#5-anti-mastra-differentiators)
6. [Architecture Constraints](#6-architecture-constraints)

---

## 1. Executive Summary

| Axis | Mastra | Verdict |
|------|--------|---------|
| Language | TypeScript | **Rust** |
| Stars | ~30k | early-stage |
| Core philosophy | DX-first, batteries included | **Guards-first, correctness over convenience** |
| Memory | 4-tier built-in (history, observational, working, semantic) | ❌ None |
| Storage | 12+ relational + 15+ vector backends | ❌ None |
| LLM providers | 40+ (model router string) | 1 (OpenAI-compatible) |
| Workflow DSL | Fluent (`.then()`, `.parallel()`, `.branch()`) | Rust types (`AgentStep` structs) |
| Guardrails | Processor pipeline (PII, injection, moderation, cost) | **50+ compile-enforced Guard variants** |
| Observability | OpenTelemetry full stack | Basic audit log + monitoring server |
| Deployment | 8+ deployment targets, built-in deployers | ❌ Manual |
| Dev UI | Studio (full featured) | Monitoring HTTP server only |
| Voice | 12+ TTS/STT providers | ❌ None |
| MCP | Full client + server with OAuth | Client (stdio + HTTP) ✓, server ✓ |
| Eval / Testing | Live scorers, datasets, experiments | Evaluation suites + per-suite cases |
| CLI | `create mastra`, `mastra dev`, hot reload | ❌ None |
| License | Apache 2.0 + Enterprise | (open source, Rust) |

**Bottom line**: Mastra wins on DX, batteries included, ecosystem breadth, and memory/storage.
Verdict wins on safety correctness, Rust performance, and enforceable guardrails.

The path to beating Mastra is: keep what makes Verdict unique (guard-first, Rust, enforceable
correctness), and close the gaps in DX, memory, provider breadth, and observability.

---

## 2. Framework Comparison

### 2.1 Core Philosophy

| | Mastra | Verdict |
|---|---|---|
| Paradigm | "Batteries included TS framework for AI apps" | "Every step ends with a verdict. Prompts suggest, guards enforce." |
| Safety model | Soft — processors can warn/block, but not statically typed | **Hard** — guards are code-enforced, statically composed |
| Composition | Fluent DSL for workflows, class instances for agents | Typed Rust structs, explicit pipelines |
| State sharing | Mutable `state` object shared across workflow | Immutable step outputs, keyed `step_results` map |
| Trust model | Developer-configured input/output processors | Every step runs guard_in → action → guard_out → verdict |

### 2.2 Strengths of Verdict

1. **Guard-first correctness** — 50+ strongly-typed `Guard` variants enforced at runtime on every step,
   every direction (in/out/verdict). No escape hatches. No soft-mode. This is Verdict's identity and should
   be its biggest selling point.

2. **Rust performance and memory safety** — Zero GC pauses, low latency, no runtime memory leaks.
   Excellent for high-throughput agent workloads that TypeScript/Node.js struggles with under load.

3. **Explicit, auditable pipelines** — `AgentStep` structs are 100% inspectable at compile time.
   No hidden magic, no late-bound runtime surprises.

4. **Security guard depth** — `NoSecretsInOutput`, `NoSecretExfiltration`, `NoGuardRemoval`,
   `NoPermissionEscalation`, `NoDangerousShellCommands`, `CargoAuditPass`, `CargoDenyPass` —
   Mastra has no equivalent static security guarantees.

5. **Self-improvement safety** — The full guarded self-update loop (reflect → propose → sandbox-validate →
   compile-check → test-check → evaluation-score → user-approval → apply → version) is unique.
   Mastra has no controlled self-modification.

6. **DAG pipelines with topology sort** — `dependencies` + `parallel` fields on `AgentStep` give
   true DAG execution with cycle detection.

7. **Workspace isolation** — `WorkspaceIsolation::TempDir/Sandboxed` prevents side effects between
   concurrent pipelines.

8. **MCP server exposure** — Verdict exposes its own agents as an MCP server, making it interoperable
   with the entire MCP ecosystem out-of-the-box.

9. **Injection protection** — `InjectionScanner` with entropy-based detection + pattern matching on every
   step output when `InjectionProtection::Strict` is set.

### 2.3 Weaknesses of Verdict / Strengths of Mastra

| Gap Area | Mastra Has | Verdict Missing |
|----------|-----------|-----------------|
| **Memory** | 4-tier memory (history, observational, working memory, semantic recall) | No persistent memory at all |
| **Storage** | 12+ relational + 15+ vector DBs, composite routing | ContextStore (JSON files) only |
| **LLM providers** | 40+ via model router string | Only OpenAI-compatible (1 pattern) |
| **Guardrail UX** | Named processors with strategies (block/warn/redact/translate) | Guards exist but no named pipeline composition |
| **Developer UI** | Full Studio: chat, workflow graphs, time travel, traces, scorers | Only monitoring HTTP server |
| **CLI** | `create mastra`, `dev`, `build`, hot reload | Nothing |
| **Deployment** | Vercel, Netlify, Cloudflare, AWS, Mastra Platform built-ins | Manual |
| **RAG / Knowledge** | Document chunking, 15+ vector stores, embedding model router | None |
| **Voice** | 12+ TTS/STT providers, real-time S2S | None |
| **Observability** | OpenTelemetry traces, correlated logs, metrics (Langfuse, Datadog, etc.) | Audit log only |
| **Workflow DSL** | Fluent: `.then()`, `.parallel()`, `.branch()`, `.foreach()`, `.map()`, `.sleep()` | Verbose struct definitions |
| **Suspend/Resume** | Built-in with state snapshots, time-travel replay | No suspend/resume |
| **Dynamic config** | Per-request: model, instructions, memory, voice via `requestContext` | No runtime overrides |
| **Evaluation** | Live scorers, datasets, experiments, rubric self-correction | Suite-based eval, no live/online eval |
| **Multi-tenant** | Per-request toolsets, per-user memory isolation, dynamic subagent versioning | No tenant isolation primitives |
| **Toolsets (dynamic)** | `toolsets` param for per-request tool scoping (multi-tenant MCP) | Static ToolSet scoping only |
| **Schema DSL** | Zod/Valibot/ArkType for input/output schemas (ergonomic) | JSON Schema `Value` (verbose) |
| **Thread title** | Auto-generate titles from first message | None |
| **Approval API** | Tool-level `requireApproval` pause, client-side resume | `Verdict::UserApproval` on step level only |
| **Delegation hooks** | `onDelegationStart`, `onDelegationComplete`, `onIterationComplete`, `messageFilter` | No delegation hooks |
| **Background tasks** | Non-blocking subagent invocations, `streamUntilIdle()` | Parallel steps but no background-detach |
| **MCP Apps** | HTML UI embedded in MCP tool results via `ui://` resources | Not available |
| **Observational memory** | Background compression of conversation history into dense observations | Not available |

---

## 3. Feature Gap Analysis

### Critical Gaps (Needed to Compete)

These gaps would prevent someone from choosing Verdict for a real production workload.

| # | Gap | Impact | Effort |
|---|-----|--------|--------|
| C1 | No memory/storage system | 🔴 Blocking | High |
| C2 | Only 1 LLM provider (OpenAI-compat) | 🔴 Blocking | Medium |
| C3 | No suspend/resume for long workflows | 🔴 Blocking | Medium |
| C4 | No CLI / scaffolding | 🔴 Blocking for adoption | Medium |
| C5 | No deployment story | 🔴 Blocking for adoption | High |
| C6 | Verbose pipeline DSL vs. fluent API | 🟠 Major DX friction | Medium |

### Important Gaps (Significant Disadvantage)

| # | Gap | Impact | Effort |
|---|-----|--------|--------|
| I1 | No observability export (OpenTelemetry, Langfuse, etc.) | 🟠 Major | Medium |
| I2 | No RAG / vector store / knowledge integration | 🟠 Major | High |
| I3 | No dynamic per-request config (model, instructions, context) | 🟠 Major | Medium |
| I4 | No Developer Studio / UI | 🟠 Major | High |
| I5 | No delegation hooks | 🟠 Major | Low |
| I6 | No multi-tenant tool scoping (dynamic toolsets) | 🟠 Major | Low |
| I7 | Evaluation system not "live" (offline only) | 🟠 Major | Medium |

### Quality-of-Life Gaps

| # | Gap | Impact | Effort |
|---|-----|--------|--------|
| Q1 | JSON Schema `Value` for schemas (verbose, no derive) | 🟡 Friction | Low |
| Q2 | No step-level approvals for tools (only step verdicts) | 🟡 Friction | Low |
| Q3 | No structured logging with trace correlation | 🟡 Friction | Low |
| Q4 | No thread/session title generation | 🟡 Minor | Low |
| Q5 | Conversation history not persisted across runs | 🟡 Friction | Low |
| Q6 | No built-in cost estimation / reporting | 🟡 Friction | Low |
| Q7 | No `sleep(ms)` / `sleepUntil(datetime)` step actions | 🟡 Friction | Low |
| Q8 | No `foreach` parallel mapping over arrays | 🟡 Friction | Low |
| Q9 | No input/output transformers (pre/post step processors) | 🟡 Friction | Low |
| Q10 | No guard short-circuit on first failure vs. collect-all mode | 🟡 Minor | Low |

---

## 4. Improvement Plan

### Phase A — Quick Wins (0–2 weeks)

> High-value, low-effort improvements that immediately raise the quality bar.

#### A1 — Step-Level Tool Approval API

**Problem**: Mastra lets individual tools declare `requireApproval: true` which pauses only that tool call,
not the entire step. Verdict's `Verdict::UserApproval` is coarser (whole step).

**Fix**: Add `StepAction::ToolCallWithApproval` or a tool-registration flag:

```rust
// In ToolRegistry:
registry.register_with_approval(Arc::new(my_tool));
// → before every call, emits OutputEvent::ToolApprovalRequired and blocks
// → runner awaits stdin y/N before continuing
```

Add `OutputEvent::ToolApprovalRequired { step: String, tool: String, args: Value }`.
Add `AuditEvent::ToolApprovalRequested`, `ToolApprovalGranted`, `ToolApprovalDenied`.

#### A2 — Delegation Hooks

**Problem**: No way to intercept/modify delegation inputs or outputs, or bail out loops.

**Fix**: Add hook fields to `DelegationPolicy`:

```rust
pub struct DelegationPolicy {
    // existing fields...
    pub on_delegation_start: Option<Arc<dyn Fn(&DelegationContext) -> DelegationDecision + Send + Sync>>,
    pub on_delegation_complete: Option<Arc<dyn Fn(&DelegationResult) -> DelegationFeedback + Send + Sync>>,
    pub on_iteration_complete: Option<Arc<dyn Fn(&IterationContext) -> IterationDecision + Send + Sync>>,
    pub message_filter: Option<Arc<dyn Fn(&MessageHistory) -> MessageHistory + Send + Sync>>,
}

pub enum DelegationDecision { Proceed, Reject { reason: String }, ModifyInput(Value) }
pub enum DelegationFeedback { Continue, Bail { reason: String }, InjectFeedback(String) }
pub enum IterationDecision { Continue, Stop }
```

#### A3 — Sleep / Delay Step Actions

**Problem**: No way to pause a workflow for N milliseconds or until a datetime.

**Fix**: Add to `StepAction`:

```rust
StepAction::Sleep { duration_ms: u64 }
StepAction::SleepUntil { timestamp: DateTime<Utc> }
```

Implementation: `tokio::time::sleep(Duration::from_millis(duration_ms))`.

#### A4 — `ForEach` Parallel Step Action

**Problem**: No way to map a step over an array of inputs with configurable concurrency.

**Fix**: Add to `StepAction`:

```rust
StepAction::ForEach {
    input_array_key: String,      // key into step_results to get Vec<Value>
    body: Box<StepAction>,        // executed for each item
    concurrency: usize,           // max simultaneous executions (default 1 = sequential)
    collect_results: bool,        // if true, merge all outputs into array
}
```

#### A5 — `Guard::AllOf` Fail Mode: Collect All vs. Short-Circuit

**Problem**: `Guard::AllOf` currently short-circuits on first failure, giving partial info.

**Fix**: Add `Guard::AllOfCollect(Vec<Guard>)` variant that runs all guards and returns
all failures combined in a single `GuardError::Multiple(Vec<GuardError>)`.

#### A6 — Built-in Cost Reporting in `PipelineResult`

**Problem**: Costs are tracked in `BudgetState` but not surfaced in `PipelineResult`.

**Fix**: Add `total_cost_usd: f64` and `total_tokens_used: u32` to `PipelineResult`.

#### A7 — Structured Logging with Trace Correlation

**Problem**: No structured log output correlated to pipeline/step/trace IDs.

**Fix**: Add a `log: Vec<LogEntry>` to `PipelineResult` / emit via `OutputSink`:

```rust
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,          // Trace, Debug, Info, Warn, Error
    pub pipeline: String,
    pub step: String,
    pub trace_id: String,
    pub span_id: String,
    pub message: String,
    pub fields: Value,
}

pub enum LogLevel { Trace, Debug, Info, Warn, Error }
```

Add `OutputEvent::Log(LogEntry)` so log entries stream out via `OutputSink`.

#### A8 — Thread Title Auto-Generation

**Problem**: No session/thread naming.

**Fix**: Add optional `auto_title` to `ConversationRegistry`:

```rust
// In PipelineRunner:
runner.with_auto_title_model(Arc::clone(&llm_client))
// → after first LLM call in a new conversation_id, asynchronously generate a
//   1-sentence title and store it in ConversationRegistry
//   → accessible via conversation_registry.get_title(id)
```

---

### Phase B — Core DX & Developer Surface (2–6 weeks)

> Reduce the verbosity gap vs. Mastra's fluent DSL.

#### B1 — Pipeline Builder DSL (Fluent API)

**Problem**: Mastra has `.then().parallel().branch().foreach().map()` — elegant, readable.
Verdict requires verbose `Vec<AgentStep>` structs with every field explicitly set.

**Fix**: Add a fluent `PipelineBuilder` alongside the existing struct API (fully backward-compatible):

```rust
let pipeline = PipelineBuilder::new("my_pipeline")
    .then(step_a)
    .then(step_b)
    .parallel(vec![step_c, step_d, step_e])
    .branch(vec![
        (Guard::FileExists("/path/to/file".into()), step_if_exists),
        (Guard::None, step_fallback),
    ])
    .foreach("items", step_process, /* concurrency */ 4)
    .map(|ctx| { /* transform ctx */ ctx })
    .sleep(Duration::from_millis(500))
    .build()
    .expect("invalid pipeline");
```

This generates the same `Pipeline { steps: Vec<AgentStep> }` struct under the hood.

#### B2 — `AgentStep` Defaults with Builder Pattern

**Problem**: Every `AgentStep` requires all 8 fields. Most users only care about 3.

**Fix**: Add builder:

```rust
let step = AgentStep::builder("step_name")
    .action(StepAction::LlmCall {
        system: "...".into(),
        user: "...".into(),
        model: None,
        conversation_id: None,
        append_to_history: false,
    })
    .guard_out(Guard::NonEmptyOutput)
    .verdict(Verdict::Pass)
    .tools(ToolSet::ReadOnly)
    .build();
// guard_in defaults to Guard::None
// injection_protection defaults to InjectionProtection::None
// output_schema defaults to None
// dependencies defaults to []
// parallel defaults to false
```

#### B3 — Dynamic Per-Request Configuration

**Problem**: No way to change model, instructions, or tools per request (e.g., based on user tier).

**Fix**: Add `RequestContext` struct and support function-based fields:

```rust
pub struct RequestContext {
    pub fields: HashMap<String, Value>,
}

impl RequestContext {
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T>;
    pub fn set(&mut self, key: &str, value: impl serde::Serialize);
}

// Extend PipelineRunner:
runner.run_with_context(&pipeline, &agent, input, request_context).await

// In AgentStep, allow model to be dynamic:
pub model_resolver: Option<Arc<dyn Fn(&RequestContext) -> ProviderSpec + Send + Sync>>,
// Similarly for system/user prompt:
pub system_resolver: Option<Arc<dyn Fn(&RequestContext) -> String + Send + Sync>>,
```

#### B4 — Named Guardrail Processors

**Problem**: Guards are powerful but anonymous. Mastra gives named processors with strategies.

**Fix**: Add a `GuardProcessor` wrapper and `InputPipeline`/`OutputPipeline` to `AgentStep`:

```rust
pub struct GuardProcessor {
    pub name: String,
    pub guard: Guard,
    pub strategy: GuardStrategy,
    pub on_violation: Option<Arc<dyn Fn(&GuardViolation) + Send + Sync>>,
}

pub enum GuardStrategy {
    Block,      // fail the step (current behavior)
    Warn,       // log warning, continue
    Redact,     // replace detected pattern in output with "[REDACTED]"
    Rewrite,    // call LLM to rewrite the output removing the violation
}

// In AgentStep:
pub input_processors: Vec<GuardProcessor>,
pub output_processors: Vec<GuardProcessor>,
```

Existing `guard_in`/`guard_out` remain fully functional — processors are additive.

#### B5 — Verdict CLI (`cargo install verdict-cli`)

**Problem**: No CLI. Mastra has `create mastra`, `mastra dev`, `mastra build`, hot reload.

**Fix**: Create `verdict-cli` binary crate (separate from the library):

```
verdict new my-project    # scaffold a new verdict project from template
verdict dev               # run with file watch + auto-recompile
verdict check             # run cargo check + all guard static analysis
verdict audit             # run cargo audit + cargo deny
verdict run <agent>       # run a named agent from config
```

Scaffold template includes:
- `verdict.toml` config file
- `src/agents/` directory
- `src/skills/` directory
- `tests/` with example phase tests
- `.env.example`

#### B6 — `verdict.toml` Config File

**Problem**: All configuration is code. Mastra can be configured via files and env vars.

**Fix**: Add `verdict.toml` schema loaded by `PipelineRunner`:

```toml
[runtime]
default_model = "gpt-4o"
max_cost_usd = 10.0
workspace_root = "./workspace"

[llm]
provider = "openai"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"

[mcp.servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "./workspace"]

[mcp.servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
allowed_tools = ["create_issue", "read_pull_request"]

[memory]
backend = "sqlite"
path = "./verdict_memory.db"

[observability]
exporter = "stdout"   # or "opentelemetry", "langfuse"
```

---

### Phase C — Memory, Storage & RAG (6–10 weeks)

> This is the biggest single gap vs. Mastra.

#### C1 — Multi-Tier Memory System

Implement a `verdict-memory` sub-crate (optional dependency) with the following architecture:

```
verdict-memory/
├── src/
│   ├── lib.rs
│   ├── store.rs         # MemoryStore trait
│   ├── thread.rs        # ThreadMemory (message history)
│   ├── working.rs       # WorkingMemory (structured user data)
│   ├── semantic.rs      # SemanticMemory (vector similarity)
│   ├── compress.rs      # ObservationalMemory (LLM compression)
│   └── backends/
│       ├── sqlite.rs    # SQLite backend (default, no deps)
│       ├── postgres.rs  # PostgreSQL backend (feature-gated)
│       └── memory.rs    # In-memory backend (for testing)
```

**`MemoryStore` trait**:

```rust
#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn save_message(&self, thread_id: &str, msg: &ChatMessage) -> Result<(), MemoryError>;
    async fn get_thread(&self, thread_id: &str, last_n: usize) -> Result<Vec<ChatMessage>, MemoryError>;
    async fn save_working_memory(&self, resource_id: &str, data: &Value) -> Result<(), MemoryError>;
    async fn get_working_memory(&self, resource_id: &str) -> Result<Option<Value>, MemoryError>;
    async fn search_semantic(&self, resource_id: &str, query: &str, top_k: usize) -> Result<Vec<SemanticMatch>, MemoryError>;
    async fn upsert_embedding(&self, resource_id: &str, text: &str, embedding: Vec<f32>, metadata: Value) -> Result<(), MemoryError>;
}
```

**Thread Memory** (replaces current `ConversationRegistry`):
- Backed by SQLite by default (no extra deps)
- Persists conversation history across `PipelineRunner` restarts
- `last_n` limit with automatic truncation

**Working Memory**:
- Structured JSON blob per `resource_id` (user/entity)
- LLM can read and update via tool: `memory.get_working_memory`, `memory.set_working_memory`
- Schema-validated updates via `Guard::MatchesSchema`

**Semantic Recall**:
- Stores embeddings alongside messages
- `search_semantic(resource_id, query, top_k)` — cosine similarity search
- Works with any embedding model via `LlmClient::embed()`

**Observational Memory** (LLM compression):
- Background task: when `thread.messages.len() > N`, call LLM to compress old messages into dense "observations"
- Store observations as special message type
- Insert into context window before new messages
- Controlled by `MemoryConfig::compress_threshold` and `compress_model`

**Memory as built-in tools** (auto-registered when memory is configured):
- `memory.get_thread` — retrieve last N messages
- `memory.search` — semantic search
- `memory.set_working` — update working memory
- `memory.get_working` — read working memory

**Integration with `PipelineRunner`**:

```rust
runner.with_memory(Arc::new(SqliteMemoryStore::new("./verdict.db").await?))
```

#### C2 — Vector Store Integration

Add `verdict-vector` sub-crate with a `VectorStore` trait:

```rust
#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn create_index(&self, name: &str, dimension: usize) -> Result<(), VectorError>;
    async fn upsert(&self, index: &str, vectors: Vec<VectorRecord>) -> Result<(), VectorError>;
    async fn query(&self, index: &str, vector: Vec<f32>, top_k: usize) -> Result<Vec<VectorMatch>, VectorError>;
    async fn delete(&self, index: &str, ids: Vec<String>) -> Result<(), VectorError>;
}

pub struct VectorRecord { pub id: String, pub vector: Vec<f32>, pub metadata: Value }
pub struct VectorMatch { pub id: String, pub score: f32, pub metadata: Value }
```

Initial backends (feature-gated):
- `sqlite-vec` — embedded, zero external deps (`verdict-vector/sqlite` feature)
- `qdrant` — Qdrant HTTP client (`verdict-vector/qdrant` feature)
- `postgres` — pgvector extension (`verdict-vector/postgres` feature)

#### C3 — RAG Pipeline Primitives

Add `verdict-rag` sub-crate:

```rust
pub struct Document {
    pub id: String,
    pub content: String,
    pub metadata: Value,
}

pub struct Chunk {
    pub id: String,
    pub doc_id: String,
    pub text: String,
    pub metadata: Value,
}

// Chunking strategies
pub enum ChunkStrategy {
    FixedSize { size: usize, overlap: usize },
    Recursive { max_size: usize },
    Sentence,
    Paragraph,
}

pub struct Chunker { /* ... */ }
impl Chunker {
    pub fn chunk(doc: &Document, strategy: ChunkStrategy) -> Vec<Chunk>;
}

// Indexing pipeline
pub struct RagIndexer {
    pub vector_store: Arc<dyn VectorStore>,
    pub embed_fn: Arc<dyn Fn(&str) -> Pin<Box<dyn Future<Output = Vec<f32>> + Send>> + Send + Sync>,
}
impl RagIndexer {
    pub async fn index(&self, chunks: Vec<Chunk>, index_name: &str) -> Result<(), RagError>;
}

// Query
pub struct RagQuery {
    pub vector_store: Arc<dyn VectorStore>,
    pub embed_fn: Arc<dyn Fn(&str) -> Pin<Box<dyn Future<Output = Vec<f32>> + Send>> + Send + Sync>,
}
impl RagQuery {
    pub async fn search(&self, query: &str, index_name: &str, top_k: usize) -> Result<Vec<VectorMatch>, RagError>;
}
```

**RAG as a built-in skill**: Add `rag_retrieval()` built-in skill with a pipeline that:
1. Embeds the user query
2. Searches the vector store
3. Injects top-K results into the LLM prompt as context

---

### Phase D — Multi-Agent & Orchestration (10–14 weeks)

#### D1 — Supervisor Pattern with Memory Isolation

**Problem**: Child agents share the same `ConversationRegistry` keys, risking history pollution.

**Fix**: Auto-namespace child conversation IDs:
```
{parent_conversation_id}/{depth}/{agent_name}/{step_name}
```
Stable across delegations (same `agent_name` → same memory resource).

**Add `MemoryIsolation` to `DelegationPolicy`**:

```rust
pub enum MemoryIsolation {
    Isolated,           // fresh thread per delegation (default)
    Shared,             // share parent thread
    NamespacedByAgent,  // {parent_resource}-{agent_name} (stable long-term memory)
}
```

#### D2 — Dynamic Toolsets (Multi-Tenant)

**Problem**: `ToolSet` is static at build time. Mastra supports per-request `toolsets` param for
multi-tenant MCP (different credentials per user).

**Fix**: Add `toolsets` to `PipelineRunner::run_with_context()`:

```rust
runner.run_with_context(
    &pipeline,
    &agent,
    input,
    RequestContext::new()
        .with_toolset("mcp_github", Arc::new(user_github_mcp_tools))
        .with_toolset("mcp_slack", Arc::new(user_slack_mcp_tools)),
).await
```

#### D3 — Background / Detached Agent Invocations

**Problem**: All `DelegateAgent` steps block until the child agent completes.

**Fix**: Add `detached: bool` flag to `DelegateAgent`:

```rust
StepAction::DelegateAgent {
    agent: "email_sender".into(),
    input: json!({ "to": "...", "body": "{output}" }),
    detached: true,  // fire-and-forget; step immediately returns empty StepOutput
    ..
}
```

Detached agents run via `tokio::spawn` and report results to `AuditLog` when done.
Add `Guard::DetachedAgentCompleted(String)` to optionally wait at a later step.

#### D4 — Workflow Suspend / Resume

**Problem**: No way to pause a pipeline and resume later (e.g., waiting for human input or external event).

**Fix**: Add to `StepAction`:

```rust
StepAction::Suspend {
    reason: String,
    resume_schema: Option<Value>,     // schema for the data provided at resume time
    timeout: Option<Duration>,        // auto-fail if not resumed within timeout
}
```

Semantics:
1. Step runs `Suspend` action → `PipelineRunner::run()` returns `PipelineResult::Suspended { step, reason, state_token }`
2. State (full `SerializableStepContext`) is saved to `ContextStore` keyed by `state_token`
3. Caller later calls `runner.resume(state_token, resume_data)` → pipeline continues from that step
4. `{suspended_step_name.resume_data}` is accessible in later steps

`Guard::ResumeDataMatchesSchema(Value)` validates the resume payload.

---

### Phase E — Observability & Deployment (14–18 weeks)

#### E1 — OpenTelemetry Export

**Problem**: Audit log is internal-only. No way to send traces to Langfuse, Datadog, Jaeger, etc.

**Fix**: Add `OtelExporter` trait in `verdict-telemetry` sub-crate:

```rust
pub trait OtelExporter: Send + Sync {
    fn export_span(&self, span: OtelSpan);
}

pub struct OtelSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub attributes: HashMap<String, Value>,
    pub status: OtelStatus,
    pub events: Vec<OtelEvent>,
}
```

Adapters (feature-gated):
- `StdoutExporter` — pretty-print spans (default)
- `JaegerExporter` — HTTP POST to Jaeger
- `OtlpExporter` — OTLP gRPC/HTTP (standard OpenTelemetry wire format)
- `LangfuseExporter` — HTTP POST to Langfuse API

**Convert `AuditLog` events to OtelSpans** automatically when exporter is configured.

Integration:
```rust
runner.with_telemetry_exporter(Arc::new(OtlpExporter::from_env()))
```

The `SensitiveDataFilter` span processor from Mastra is equivalent to running
`Guard::NoSecretsInOutput` on all spans before export — add this as an option.

#### E2 — Enhanced Monitoring Server (Studio Light)

**Problem**: Current monitoring server shows basic audit + trace. Mastra's Studio has agent chat,
workflow graph visualization, time-travel, scorers, datasets.

**Fix** (incremental — not trying to match Studio fully):
1. Add WebSocket support to monitoring server for real-time streaming
2. Add `/api/conversations` endpoint (list threads + message history)
3. Add `/api/agents` endpoint (list registered agents + their pipelines as JSON)
4. Add `/api/run` endpoint (POST to trigger a pipeline run from the browser)
5. Improve HTML dashboard with basic pipeline graph visualization (Mermaid.js rendered server-side)
6. Add `/api/eval` endpoint (trigger evaluation suite, stream results)

#### E3 — `cargo-verdict` Deployment Helpers

**Problem**: No deployment story.

**Fix**: Add `verdict-server` sub-crate that wraps `axum` into a production-ready HTTP server:

```rust
// In verdict-server:
pub struct VerdictServer {
    mastra: Arc<PipelineRunner>,
    agents: Arc<AgentRegistry>,
}

impl VerdictServer {
    pub fn new(runner: PipelineRunner) -> Self;
    pub async fn serve(self, addr: SocketAddr) -> Result<(), ServerError>;
}

// Auto-generates REST API:
// POST /agents/{name}/run         → run agent pipeline
// POST /agents/{name}/stream      → stream agent pipeline
// GET  /agents/{name}             → describe agent
// GET  /agents                    → list agents
// POST /workflows/{name}/run      → run workflow (pipeline)
// GET  /health                    → health check
// GET  /swagger-ui                → OpenAPI spec + UI
```

Docker support: generate `Dockerfile` via `verdict new --with-docker`.

---

### Phase F — Evaluation & Self-Improvement Polish (18–22 weeks)

#### F1 — Live / Online Evaluation (Scorer Sampling)

**Problem**: Current evaluation only runs offline against test suites.

**Fix**: Add `ScorerConfig` to `Agent`:

```rust
pub struct Agent {
    // existing fields...
    pub scorers: Vec<ScorerConfig>,
}

pub struct ScorerConfig {
    pub scorer: Arc<dyn Scorer>,
    pub sampling_rate: f64,          // 0.0–1.0, what fraction of runs to evaluate
    pub apply_to: ScorerApplyTo,     // EveryStep | OnlyFinal | StepByName(String)
}

#[async_trait]
pub trait Scorer: Send + Sync {
    fn name(&self) -> &str;
    async fn score(&self, result: &PipelineResult) -> Result<ScorerResult, ScorerError>;
}

pub struct ScorerResult {
    pub score: f64,         // 0.0–1.0
    pub pass: bool,
    pub feedback: Option<String>,
}
```

Built-in scorers:
- `AnswerRelevancyScorer` — LLM-as-judge relevance
- `ToxicityScorer` — detect harmful content
- `RubricScorer` — checklist-based iteration (like Mastra's rubric scorer)
- `CustomScorer(fn)` — arbitrary Rust function

#### F2 — Rubric-Based Self-Correction Loop

**Problem**: No equivalent to Mastra's rubric scorer that makes the agent self-correct until
all rubric items pass.

**Fix**: Add `StepAction::RubricLoop`:

```rust
StepAction::RubricLoop {
    body: Box<StepAction>,           // the action to evaluate
    rubric: Vec<RubricItem>,         // list of criteria to satisfy
    max_iterations: u32,             // max attempts before giving up
    judge_model: Option<ProviderSpec>,
}

pub struct RubricItem {
    pub criterion: String,    // natural language criterion
    pub required: bool,       // must pass vs. nice to have
}
```

Each iteration: run body → send output + rubric to LLM judge → if all required criteria pass, exit;
otherwise, feed failed criteria back as `{rubric_feedback}` template variable for next iteration.

#### F3 — Evaluation Datasets & Experiments

**Problem**: No way to manage test datasets or compare agent versions across experiments.

**Fix**: Add `EvaluationDataset` and `Experiment` types:

```rust
pub struct EvaluationDataset {
    pub name: String,
    pub version: u32,
    pub cases: Vec<EvaluationCase>,
}

pub struct Experiment {
    pub name: String,
    pub dataset: EvaluationDataset,
    pub agent: Agent,
    pub agent_version: Option<String>,
    pub run_at: DateTime<Utc>,
    pub results: Vec<EvaluationResult>,
    pub summary_score: f64,
}

pub struct ExperimentRunner {
    pub runner: Arc<PipelineRunner>,
}
impl ExperimentRunner {
    pub async fn run_experiment(&self, experiment: Experiment) -> ExperimentReport;
    pub fn compare(a: &ExperimentReport, b: &ExperimentReport) -> ExperimentDiff;
}
```

Store experiment results in the configured memory backend for historical comparison.

---

## 5. Anti-Mastra Differentiators

These are Verdict's unique winning angles — things Mastra cannot match even in principle.

### "Correctness over Convenience" Brand

Mastra's message is "batteries included, ship fast." Verdict's message should be:
**"Every step verified. Every guard enforced. Production-ready from day one."**

Target audience: teams that got burned by LLM agents doing the wrong thing in production.
Medical, legal, financial, security domains where "soft guardrails" are not enough.

### Compile-Time Agent Verification

Add a `verify_pipeline!()` procedural macro that checks:
- All referenced agents exist in `AgentRegistry`
- All referenced tools are registered
- `dependencies` form a valid DAG (no cycles)
- `guard_in` / `guard_out` / `verdict` types are well-formed
- Budget guards don't conflict (e.g., `MaxCostUsd(1.0)` in guard + `MaxCostUsd(100.0)` in policy)

This is impossible in TypeScript/Mastra where everything is runtime-checked.

### Fearless Self-Improvement

Market the full self-update pipeline as a unique differentiator:
> "The only AI agent framework where agents can safely improve themselves — in a sandbox, with compile
> verification, test validation, evaluation scoring, and human approval — before any change is applied."

### Guard Composition as a First-Class API

Mastra's processors are a flat pipeline. Verdict's guards compose algebraically:

```rust
Guard::AllOf(vec![
    Guard::NoSecretsInOutput,
    Guard::Not(Box::new(Guard::MaxOutputBytes(1_000_000))),
    Guard::AnyOf(vec![Guard::ValidJson, Guard::ValidYaml]),
    Guard::Custom(my_fn),
])
```

This algebraic composition is not possible in Mastra's sequential processor model.

### Zero-Runtime-Overhead Security

In Rust, security guards are checked synchronously in the same process with zero network round-trips,
zero serialization overhead. Mastra's moderation processors call external APIs (adding 100ms–2s latency
per call). For high-frequency tool calls, this matters.

---

## 6. Architecture Constraints

All improvements must respect the following constraints from `architecture.md` and `AGENTS.md`:

1. **No breaking changes to public API** — all new features are additive
2. **Sub-crates for optional deps** — memory, vector, RAG, telemetry are separate crates with feature flags
3. **Guard/Verdict/Pipeline structs are immutable** — no new required fields without architecture.md update
4. **Any new `Cargo.toml` dependency requires explicit user approval**
5. **Phase tests must not regress** — all existing `tests/phase*.rs` remain green
6. **`architecture.md` must be updated** before implementing any new struct, enum, or public API
7. **No stubs** — every implementation is complete and tested

---

## Implementation Priority Order

Based on impact vs. effort, recommended implementation sequence:

```
Week 1-2:   A1 (tool approval), A2 (delegation hooks), A3 (sleep), A4 (foreach), A5 (AllOfCollect)
Week 2-3:   A6 (cost reporting), A7 (structured logging), A8 (thread titles)
Week 3-5:   B1 (pipeline builder DSL), B2 (step builder), B3 (dynamic config)
Week 5-6:   B4 (named processors), B5 (CLI scaffold), B6 (verdict.toml)
Week 6-10:  C1 (memory system), C2 (vector store), C3 (RAG primitives)
Week 10-14: D1 (memory isolation), D2 (dynamic toolsets), D3 (background agents), D4 (suspend/resume)
Week 14-18: E1 (OpenTelemetry), E2 (enhanced monitoring), E3 (deployment server)
Week 18-22: F1 (live scorers), F2 (rubric loops), F3 (datasets & experiments)
```

**IMPORTANT**: Every feature in Phase C onward requires:
1. Updating `architecture.md` with the new sub-crate design
2. User approval before implementation begins
3. Full integration tests for each new phase

---

*Generated: 2026-06-18 | Framework versions: Mastra (June 2026, ~30k stars), Verdict (June 2026, Phase 16)*
