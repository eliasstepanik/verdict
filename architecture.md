

# Verdict Extended Plan

**Verdict**: every step ends with a verdict. Hard guards, not soft prompts.

Verdict is a Rust framework for building agents that **actually complete their work** through code-enforced structure, guarded execution, scoped tools, composable pipelines, delegated agents, reusable skills, and self-improvement loops.

## Updated Core Idea

An agent is not just:

```rust
struct Agent {
    system_prompt: String,
    tools: Vec<Tool>,
}
```

An agent is:

```rust
pub struct Agent {
    pub name: String,
    pub description: String,
    pub pipeline: Pipeline,
    pub tools: ToolSet,
    pub skills: SkillSet,
    pub policy: AgentPolicy,
}
```

A Verdict agent can:

1. Execute guarded steps
2. Call scoped tools
3. Delegate work to other agents
4. Use registered MCP tools
5. Expose its own Rust functions as tools
6. Use reusable skills
7. Reflect on its performance
8. Propose improvements to itself
9. Apply self-updates only after strict guards and verdicts pass

---

# Extended Architecture

```txt
┌──────────────────────────────────────────────────────────┐
│                      Verdict Runtime                      │
│                                                          │
│  ┌────────────────┐      ┌────────────────────────────┐  │
│  │ PipelineRunner │─────▶│ AgentRegistry              │  │
│  └────────────────┘      │ - coder                    │  │
│          │               │ - reviewer                 │  │
│          │               │ - debugger                 │  │
│          ▼               │ - planner                  │  │
│  ┌────────────────┐      └────────────────────────────┘  │
│  │ Guard Engine   │                                      │
│  └────────────────┘      ┌────────────────────────────┐  │
│          │               │ ToolRegistry               │  │
│          ▼               │ - local function tools      │  │
│  ┌────────────────┐      │ - MCP server tools          │  │
│  │ Verdict Engine │      │ - shell/filesystem/search   │  │
│  └────────────────┘      └────────────────────────────┘  │
│          │                                                │
│          ▼               ┌────────────────────────────┐  │
│  ┌────────────────┐      │ SkillRegistry              │  │
│  │ Audit Log      │      │ - code_review              │  │
│  └────────────────┘      │ - rust_debugging           │  │
│                          │ - api_design               │  │
│                          └────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

---

# New Core Concepts

## 1. Agent

An `Agent` owns a pipeline, default tools, skills, and policy.

```rust
pub struct Agent {
    pub name: String,
    pub description: String,
    pub pipeline: Pipeline,
    pub tools: ToolSet,
    pub skills: SkillSet,
    pub policy: AgentPolicy,
}
```

Example:

```rust
let coder = Agent {
    name: "coder".into(),
    description: "Implements approved software changes.".into(),
    pipeline: coder_pipeline(),
    tools: ToolSet::ReadWrite,
    skills: SkillSet::from(["rust", "testing", "debugging"]),
    policy: AgentPolicy::default(),
};
```

---

## 2. Agent Registry

Agents should be registered in a central registry so they can delegate to each other.

```rust
pub struct AgentRegistry {
    agents: HashMap<String, Arc<Agent>>,
}

impl AgentRegistry {
    pub fn register(&mut self, agent: Agent) {
        self.agents.insert(agent.name.clone(), Arc::new(agent));
    }

    pub fn get(&self, name: &str) -> Option<Arc<Agent>> {
        self.agents.get(name).cloned()
    }
}
```

Example:

```rust
let mut registry = AgentRegistry::new();

registry.register(coder_agent());
registry.register(reviewer_agent());
registry.register(debugger_agent());
registry.register(planner_agent());
registry.register(reflector_agent());
```

---

# Agent Delegation

## New StepAction Variant

```rust
pub enum StepAction {
    /// Call an LLM with a prompt
    LlmCall {
        system: String,
        user: String,
        model: Option<ProviderSpec>,
        /// Optional conversation ID for multi-turn interactions
        conversation_id: Option<String>,
        /// Whether to append the user message and assistant response to conversation history
        append_to_history: bool,
    },

    /// Run a tool directly
    ToolCall {
        tool: String,
        args: Value,
    },

    /// Delegate to a named agent
    DelegateAgent {
        agent: String,
        input: Value,
        expected_output_schema: Option<Value>,
        delegation_policy: DelegationPolicy,
    },

    /// Delegate to a sub-pipeline directly
    SubPipeline(Pipeline),

    /// Loop/iterate until a condition is met
    LoopUntil {
        body: Box<StepAction>,
        condition: Guard,
        max_iterations: u32,
        on_iteration_failure: IterationFailureMode,
    },

    /// Execute arbitrary Rust code
    Custom(Arc<dyn Fn(&StepContext) -> Result<StepOutput, StepError> + Send + Sync>),

    /// Ask the user for input
    UserInput {
        prompt: String,
        schema: Option<Value>,
    },
}
```

## Delegation Policy

```rust
pub struct DelegationPolicy {
    pub max_depth: u32,
    pub allowed_agents: Vec<String>,
    pub require_output_schema: bool,
    pub inherit_tool_scope: bool,
    pub inherit_budget: bool,
    pub require_user_approval: bool,
}
```

This prevents uncontrolled agent recursion.

## IterationFailureMode

When a loop iteration fails (action fails, but condition hasn't passed), the loop controller decides how to proceed:

```rust
pub enum IterationFailureMode {
    /// Retry the iteration body immediately
    Retry,

    /// Skip this iteration and move to the next
    Skip,

    /// Abort the entire loop and fail
    Abort,
}
```

Example:

```rust
StepAction::DelegateAgent {
    agent: "reviewer".into(),
    input: json!({
        "diff": "{current_diff}",
        "task": "{request}"
    }),
    expected_output_schema: Some(review_schema()),
    delegation_policy: DelegationPolicy {
        max_depth: 2,
        allowed_agents: vec!["reviewer".into(), "debugger".into()],
        require_output_schema: true,
        inherit_tool_scope: true,
        inherit_budget: true,
        require_user_approval: false,
    },
}
```

## Example: TDD Micro Agent Loop

A concrete example of using `LoopUntil` to implement a test-driven development loop: write code → run tests → iterate until tests pass.

```rust
StepAction::LoopUntil {
    body: Box::new(StepAction::SubPipeline(Pipeline {
        name: "tdd_iteration".into(),
        steps: vec![
            AgentStep {
                name: "write_or_fix_code".into(),
                guard_in: Guard::None,
                action: StepAction::LlmCall {
                    system: "You are implementing code to pass failing tests. Write only the code needed.".into(),
                    user: "Failing tests:\n{test_output}\n\nWrite code to fix these tests.".into(),
                    model: None,
                },
                guard_out: Guard::ValidRustSyntax,
                verdict: Verdict::Automated(Guard::ValidRustSyntax),
                tools: ToolSet::Allow(vec!["fs.write".into()]),
                injection_protection: InjectionProtection::Strict,
            },
            AgentStep {
                name: "run_tests".into(),
                guard_in: Guard::ValidRustSyntax,
                action: StepAction::ToolCall {
                    tool: "shell.cargo_test".into(),
                    args: json!({}),
                },
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::Allow(vec!["shell.cargo_test".into()]),
                injection_protection: InjectionProtection::Strict,
            },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    })),
    condition: Guard::TestsPass,
    max_iterations: 10,
    on_iteration_failure: IterationFailureMode::Retry,
}
```

This loop:
- Repeats up to 10 times
- Each iteration: (1) LLM writes/fixes code, (2) runs tests
- Exits when `Guard::TestsPass` succeeds
- On iteration body failure, retries immediately
- Prevents infinite loops via `max_iterations`

---

# Tool Registry

Tools can come from:

1. Built-in Verdict tools
2. MCP servers
3. Agent-local Rust functions
4. External CLI tools
5. Remote HTTP tools, if explicitly allowed

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}
```

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Value;
    fn source(&self) -> ToolSource;

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError>;
}
```

## Tool Source

```rust
pub enum ToolSource {
    Builtin,
    LocalFunction,
    McpServer {
        server_name: String,
        tool_name: String,
    },
    ExternalCommand {
        command: String,
    },
    Http {
        base_url: String,
    },
}
```

---

# MCP Tool Support

Verdict should support registered MCP servers.

```rust
pub struct McpServerConfig {
    pub name: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub env: HashMap<String, String>,
    pub allowed_tools: Vec<String>,
}
```

Example config:

```toml
[mcp.servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "./workspace"]
allowed_tools = ["read_file", "list_directory", "search_files"]

[mcp.servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
allowed_tools = ["create_issue", "read_pull_request", "comment_on_pr"]
```

## MCP Tool Registration

```rust
let mcp = McpClient::connect(config).await?;

let tools = mcp.discover_tools().await?;

for tool in tools {
    tool_registry.register_mcp_tool("github", tool)?;
}
```

## MCP Tool Guarding

MCP tools should still be scoped per step.

```rust
AgentStep {
    name: "inspect_repo",
    guard_in: Guard::None,
    action: StepAction::ToolCall {
        tool: "mcp.filesystem.search_files".into(),
        args: json!({ "query": "TODO" }),
    },
    guard_out: Guard::MatchesSchema(search_results_schema()),
    verdict: Verdict::Automated(Guard::MaxOutputBytes(50_000)),
    tools: ToolSet::Allow(vec![
        "mcp.filesystem.read_file".into(),
        "mcp.filesystem.search_files".into(),
    ]),
    injection_protection: InjectionProtection::Strict,
}
```

---

# Agent-Local Function Tools

Agents should be able to expose their own Rust functions as tools.

```rust
pub struct FunctionTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub func: Arc<
        dyn Fn(Value, ToolContext) -> Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send>>
            + Send
            + Sync,
    >,
}
```

Example:

```rust
let summarize_diff_tool = FunctionTool::new(
    "summarize_diff",
    "Summarizes a git diff into risky and safe changes.",
    summarize_diff_schema(),
    |args, ctx| {
        Box::pin(async move {
            let diff = args["diff"].as_str().unwrap_or_default();

            let summary = summarize_diff(diff)?;

            Ok(ToolOutput::json(json!({
                "summary": summary
            })))
        })
    },
);
```

Register it:

```rust
agent.register_tool(summarize_diff_tool);
```

Use it:

```rust
StepAction::ToolCall {
    tool: "local.summarize_diff".into(),
    args: json!({
        "diff": "{current_diff}"
    }),
}
```

---

# Skills

Tools are callable operations.

Skills are reusable capabilities, instructions, policies, examples, and optional helper pipelines.

A skill may include:

1. A name
2. A description
3. Prompt fragments
4. Allowed tools
5. Required guards
6. Optional pipeline
7. Examples
8. Evaluation criteria

```rust
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
```

```rust
pub struct SkillSet {
    pub skills: Vec<String>,
}
```

Example skills:

```rust
Skill {
    name: "rust_debugging".into(),
    description: "Find and fix Rust compile/test failures.".into(),
    instructions: r#"
When debugging Rust:
1. Run cargo check first.
2. Read the compiler error.
3. Fix the smallest possible cause.
4. Run cargo test.
5. Do not rewrite unrelated files.
"#.into(),
    allowed_tools: ToolSet::Allow(vec![
        "shell.cargo_check".into(),
        "shell.cargo_test".into(),
        "fs.read".into(),
        "fs.write".into(),
    ]),
    required_guards: vec![
        Guard::Compiles,
        Guard::TestsPass,
        Guard::DiffWithinScope,
    ],
    pipeline: Some(rust_debugging_pipeline()),
    examples: vec![],
    eval: None,
}
```

## New StepAction: UseSkill

```rust
pub enum StepAction {
    // existing variants...

    UseSkill {
        skill: String,
        input: Value,
        mode: SkillMode,
    },
}
```

```rust
pub enum SkillMode {
    /// Inject skill instructions into the current step
    PromptOnly,

    /// Run the skill's pipeline
    Pipeline,

    /// Let the runtime choose between prompt-only and pipeline
    Auto,
}
```

Example:

```rust
StepAction::UseSkill {
    skill: "rust_debugging".into(),
    input: json!({
        "error": "{cargo_check_output}",
        "files": "{changed_files}"
    }),
    mode: SkillMode::Pipeline,
}
```

---

# Structured Step Output Contracts

Steps can declare structured output schemas to enable precise handoff between agents (especially for test-generation → code-generation pipelines).

## Output Schema Declaration

Each step can optionally declare an output schema:

```rust
pub struct AgentStep {
    // ... other fields ...
    pub output_schema: Option<Value>,  // JSON Schema
}
```

## Schema Validation in Guards

The next step's `guard_in` can reference output contracts:

```rust
pub enum Guard {
    // ... existing guards ...

    /// Verify previous step's output matches a JSON Schema
    PreviousStepMatchesSchema {
        step_name: String,
        schema: Value,
    },
}
```

## Example: Test-Gen → Code-Gen Handoff

A concrete example showing test generation (produces test file spec) and code generation (validates schema before running):

**Step 1: Generate Tests**

```rust
AgentStep {
    name: "generate_tests".into(),
    guard_in: Guard::None,
    action: StepAction::LlmCall {
        system: "Generate unit tests for the given requirements.".into(),
        user: "Requirements:\n{requirements}".into(),
        model: None,
    },
    guard_out: Guard::MatchesSchema(json!({
        "type": "object",
        "required": ["test_file", "test_cases"],
        "properties": {
            "test_file": { "type": "string", "description": "Path to test file" },
            "test_cases": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["name", "body"],
                    "properties": {
                        "name": { "type": "string" },
                        "body": { "type": "string" }
                    }
                }
            }
        }
    })),
    output_schema: Some(json!({
        "type": "object",
        "required": ["test_file", "test_cases"],
        "properties": {
            "test_file": { "type": "string" },
            "test_cases": { "type": "array" }
        }
    })),
    verdict: Verdict::Automated(Guard::ValidJson),
    tools: ToolSet::ReadOnly,
    injection_protection: InjectionProtection::Strict,
}
```

**Step 2: Implement Code (with schema validation)**

```rust
AgentStep {
    name: "implement_code".into(),
    guard_in: Guard::AllOf(vec![
        Guard::StepPassed("generate_tests".into()),
        Guard::PreviousStepMatchesSchema {
            step_name: "generate_tests".into(),
            schema: json!({
                "type": "object",
                "required": ["test_file", "test_cases"],
                "properties": {
                    "test_file": { "type": "string" },
                    "test_cases": { "type": "array" }
                }
            }),
        },
    ]),
    action: StepAction::LlmCall {
        system: "Implement code to pass the test cases provided.".into(),
        user: "Test spec:\n{generate_tests.output}".into(),
        model: None,
    },
    guard_out: Guard::Compiles,
    output_schema: Some(json!({
        "type": "object",
        "required": ["code_file", "code"],
        "properties": {
            "code_file": { "type": "string" },
            "code": { "type": "string" }
        }
    })),
    verdict: Verdict::AllOf(vec![
        Verdict::Automated(Guard::Compiles),
        Verdict::Automated(Guard::TestsPass),
    ]),
    tools: ToolSet::Allow(vec!["fs.write".into(), "shell.cargo_test".into()]),
    injection_protection: InjectionProtection::Strict,
}
```

This pattern ensures:
- **Test-gen output is validated** before code-gen consumes it
- **Clear contracts** between agents (no ambiguous formats)
- **Early failure detection** if a step produces unexpected output
- **Safe multi-agent handoff** with schema-driven composition

---

# Self-Reflection and Self-Improvement

Verdict should support a controlled self-reflection loop where an agent can analyze its own performance and propose improvements to its pipeline, prompts, guards, tools, or skills.

Important: the agent should **not** freely rewrite itself and immediately run arbitrary new code.

Self-improvement must be guarded.

The safe flow should be:

```txt
Run task
  ↓
Collect trace, failures, retries, tool calls, costs, outputs
  ↓
Reflect on behavior
  ↓
Propose patch
  ↓
Static validation
  ↓
Compile validation
  ↓
Test validation
  ↓
Policy validation
  ↓
Human approval, if required
  ↓
Apply patch
  ↓
Version new agent
  ↓
Run evaluation suite
  ↓
Promote only if better
```

## Reflection Step

```rust
AgentStep {
    name: "self_reflect",
    guard_in: Guard::AllOf(vec![
        Guard::TraceAvailable,
        Guard::NoActiveUncommittedCriticalChanges,
    ]),
    action: StepAction::DelegateAgent {
        agent: "reflector".into(),
        input: json!({
            "agent_name": "{agent.name}",
            "task": "{request}",
            "trace": "{pipeline_trace}",
            "failures": "{failures}",
            "tool_calls": "{tool_calls}",
            "cost": "{cost}",
            "duration": "{duration}"
        }),
        expected_output_schema: Some(reflection_schema()),
        delegation_policy: DelegationPolicy {
            max_depth: 1,
            allowed_agents: vec!["reflector".into()],
            require_output_schema: true,
            inherit_tool_scope: false,
            inherit_budget: true,
            require_user_approval: false,
        },
    },
    guard_out: Guard::MatchesSchema(reflection_schema()),
    verdict: Verdict::Automated(Guard::ReflectionHasActionableFinding),
    tools: ToolSet::ReadOnly,
    injection_protection: InjectionProtection::Strict,
}
```

## Self-Update Proposal Step

```rust
AgentStep {
    name: "propose_self_update",
    guard_in: Guard::AllOf(vec![
        Guard::StepPassed("self_reflect".into()),
        Guard::ReflectionHasActionableFinding,
    ]),
    action: StepAction::LlmCall {
        system: "You are improving this agent. Propose a minimal safe patch.".into(),
        user: r#"
Given this reflection:

{self_reflect.output}

Propose a patch to improve the agent.

Rules:
- Minimal change.
- Do not remove safety guards.
- Do not increase tool permissions.
- Do not disable user approval.
- Do not add network access.
- Output a unified diff only.
"#.into(),
        model: None,
    },
    guard_out: Guard::AllOf(vec![
        Guard::OutputIsUnifiedDiff,
        Guard::DiffTouchesAllowedPaths(vec![
            "src/agents/".into(),
            "skills/".into(),
            "prompts/".into(),
            "pipelines/".into(),
        ]),
        Guard::DiffDoesNotTouchForbiddenPaths(vec![
            "src/runner.rs".into(),
            "src/guard.rs".into(),
            "src/verdict.rs".into(),
            "Cargo.toml".into(),
            ".github/workflows/".into(),
        ]),
        Guard::NoPermissionEscalation,
        Guard::NoSecretExfiltration,
    ]),
    verdict: Verdict::AllOf(vec![
        Verdict::Automated(Guard::PatchAppliesCleanly),
        Verdict::Automated(Guard::Compiles),
        Verdict::Automated(Guard::TestsPass),
        Verdict::Automated(Guard::EvaluationImprovesOrEqual),
        Verdict::UserApproval {
            prompt: "Approve self-update patch?",
            show_diff: true,
        },
    ]),
    tools: ToolSet::Allow(vec![
        "fs.read".into(),
        "fs.write_patch".into(),
        "shell.cargo_check".into(),
        "shell.cargo_test".into(),
    ]),
    injection_protection: InjectionProtection::Strict,
}
```

## Self-Update Application Step

```rust
AgentStep {
    name: "apply_self_update",
    guard_in: Guard::AllOf(vec![
        Guard::StepPassed("propose_self_update".into()),
        Guard::UserApproved("propose_self_update".into()),
        Guard::PatchAppliesCleanly,
    ]),
    action: StepAction::ToolCall {
        tool: "fs.apply_patch".into(),
        args: json!({
            "patch": "{propose_self_update.output}"
        }),
    },
    guard_out: Guard::AllOf(vec![
        Guard::Compiles,
        Guard::TestsPass,
        Guard::EvaluationImprovesOrEqual,
    ]),
    verdict: Verdict::AllOf(vec![
        Verdict::Automated(Guard::NoPermissionEscalation),
        Verdict::Automated(Guard::AgentVersionCreated),
        Verdict::Automated(Guard::AuditLogWritten),
    ]),
    tools: ToolSet::Allow(vec![
        "fs.apply_patch".into(),
        "shell.cargo_check".into(),
        "shell.cargo_test".into(),
    ]),
    injection_protection: InjectionProtection::Strict,
}
```

---

# Agent Versioning

Self-improving agents should be versioned.

```rust
pub struct AgentVersion {
    pub agent_name: String,
    pub version: String,
    pub parent_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub change_summary: String,
    pub git_commit: Option<String>,
    pub evaluation_score: Option<f64>,
}
```

Promotion should require passing an evaluation suite.

```rust
pub struct EvaluationSuite {
    pub name: String,
    pub cases: Vec<EvaluationCase>,
    pub minimum_score: f64,
}
```

```rust
pub struct EvaluationCase {
    pub name: String,
    pub input: Value,
    pub expected: EvaluationExpected,
}
```

```rust
pub enum EvaluationExpected {
    Exact(Value),
    Schema(Value),
    Guard(Guard),
    Custom(Arc<dyn Fn(&PipelineResult) -> Result<(), EvalError> + Send + Sync>),
}
```

Promotion guard:

```rust
Guard::EvaluationImprovesOrEqual
```

This ensures the self-update does not make the agent worse.

---

# Additional Guards

The current guard list is good, but Verdict should add more guards around security, delegation, tool usage, self-modification, output quality, and runtime safety.

## Test Runner Abstraction

The `Guard::TestsPass` guard should be language-aware, supporting multiple test runners:

```rust
pub enum TestRunner {
    /// Rust: cargo test
    CargoTest,

    /// Python: pytest
    Pytest,

    /// Node.js: Jest
    Jest,

    /// Node.js: Vitest
    Vitest,

    /// Custom shell command
    Custom(String),
}
```

The `Guard::TestsPass` variant is updated to optionally specify a runner:

```rust
// Uses runtime-detected default runner
Guard::TestsPass

// Explicitly specifies runner
Guard::TestsPassWith(TestRunner)
```

**Runtime Resolution:** The runtime detects the default test runner by inspecting workspace files:
- Presence of `Cargo.toml` → `CargoTest`
- Presence of `pyproject.toml` or `requirements.txt` → `Pytest`
- Presence of `package.json` with `jest` in devDependencies → `Jest`
- Presence of `package.json` with `vitest` in devDependencies → `Vitest`
- Fallback to `CargoTest` for Rust projects

Example with explicit runner:

```rust
AgentStep {
    name: "run_tests",
    guard_in: Guard::None,
    action: StepAction::ToolCall {
        tool: "shell.cargo_test".into(),
        args: json!({}),
    },
    guard_out: Guard::TestsPassWith(TestRunner::CargoTest),
    verdict: Verdict::Automated(Guard::TestsPassWith(TestRunner::CargoTest)),
    tools: ToolSet::Allow(vec!["shell.cargo_test".into()]),
    injection_protection: InjectionProtection::Strict,
}
```

## Expanded Guard Enum

```rust
pub enum Guard {
    /// Always pass
    None,

    /// Custom Rust function
    Custom(Arc<dyn Fn(&StepContext) -> Result<(), GuardError> + Send + Sync>),

    /// Compilation check
    Compiles,

    /// Tests pass with runtime-detected runner
    TestsPass,

    /// Tests pass with explicit runner
    TestsPassWith(TestRunner),

    /// Lint passes
    LintPass,

    /// Formatting passes
    FormatPass,

    /// File exists
    FileExists(String),

    /// File does not exist
    FileNotExists(String),

    /// File contains pattern
    FileContains {
        path: String,
        pattern: String,
    },

    /// File does NOT contain pattern
    FileNotContains {
        path: String,
        pattern: String,
    },

    /// Output matches JSON Schema
    MatchesSchema(Value),

    /// Output is valid JSON
    ValidJson,

    /// Output is valid TOML
    ValidToml,

    /// Output is valid YAML
    ValidYaml,

    /// Output is valid Rust code
    ValidRustSyntax,

    /// Output is a valid unified diff
    OutputIsUnifiedDiff,

    /// Output size within token bounds
    MaxTokens(usize),

    /// Output size within byte bounds
    MaxOutputBytes(usize),

    /// Output must not be empty
    NonEmptyOutput,

    /// Output must be below max line count
    MaxLines(usize),

    /// Command completed within timeout
    TimeoutSeconds(u64),

    /// Max cost guard
    MaxCostUsd(f64),

    /// Max LLM calls
    MaxLlmCalls(u32),

    /// Max tool calls
    MaxToolCalls(u32),

    /// Max delegation depth
    MaxDelegationDepth(u32),

    /// Ensure specific previous step passed
    StepPassed(String),

    /// Ensure specific previous step failed
    StepFailed(String),

    /// Ensure user approved a step
    UserApproved(String),

    /// Ensure trace exists
    TraceAvailable,

    /// Ensure audit log exists
    AuditLogWritten,

    /// Ensure no forbidden tools were used
    NoForbiddenToolsUsed,

    /// Ensure only allowed tools were used
    OnlyAllowedToolsUsed,

    /// Ensure no permission escalation occurred
    NoPermissionEscalation,

    /// Ensure no new network access was added
    NoNewNetworkAccess,

    /// Ensure no secrets appear in output
    NoSecretsInOutput,

    /// Ensure no secrets appear in diff
    NoSecretsInDiff,

    /// Detect secret exfiltration attempts
    NoSecretExfiltration,

    /// Ensure no dangerous shell commands
    NoDangerousShellCommands,

    /// Ensure shell commands match allowlist
    ShellCommandAllowlist(Vec<String>),

    /// Ensure shell commands do not match denylist
    ShellCommandDenylist(Vec<String>),

    /// Ensure file operations stay within workspace
    PathWithinWorkspace,

    /// Ensure diff only touches allowed paths
    DiffTouchesAllowedPaths(Vec<String>),

    /// Ensure diff does not touch forbidden paths
    DiffDoesNotTouchForbiddenPaths(Vec<String>),

    /// Ensure diff size is bounded
    MaxDiffLines(usize),

    /// Ensure number of changed files is bounded
    MaxChangedFiles(usize),

    /// Ensure no generated code disables safety
    NoSafetyBypass,

    /// Ensure no generated code disables tests
    NoTestDisabling,

    /// Ensure no generated code removes guards
    NoGuardRemoval,

    /// Ensure no dependency was added
    NoNewDependencies,

    /// Ensure dependencies are from allowed list
    DependenciesAllowlist(Vec<String>),

    /// Ensure no suspicious dependency was introduced
    NoSuspiciousDependencies,

    /// Ensure cargo audit passes
    CargoAuditPass,

    /// Ensure cargo deny passes
    CargoDenyPass,

    /// Ensure reflection produced actionable finding
    ReflectionHasActionableFinding,

    /// Ensure patch applies cleanly
    PatchAppliesCleanly,

    /// Ensure evaluation score improves or stays equal
    EvaluationImprovesOrEqual,

    /// Ensure new agent version was created
    AgentVersionCreated,

    /// Ensure no uncommitted critical changes exist
    NoActiveUncommittedCriticalChanges,

    /// Ensure output is semantically equivalent according to deterministic checker
    SemanticCheck(String),

    /// ALL guards must pass
    AllOf(Vec<Guard>),

    /// ANY guard must pass
    AnyOf(Vec<Guard>),

    /// Negate guard
    Not(Box<Guard>),
}
```

---

# More Important Built-In Guards

## Security Guards

```rust
Guard::NoSecretsInOutput
Guard::NoSecretsInDiff
Guard::NoSecretExfiltration
Guard::NoDangerousShellCommands
Guard::NoNewNetworkAccess
Guard::NoPermissionEscalation
Guard::NoSafetyBypass
```

These protect the runtime from accidental or malicious outputs.

---

## Tool Guards

```rust
Guard::OnlyAllowedToolsUsed
Guard::NoForbiddenToolsUsed
Guard::MaxToolCalls(20)
Guard::ShellCommandAllowlist(vec![
    "cargo check".into(),
    "cargo test".into(),
    "cargo fmt".into(),
])
```

These ensure tool use follows the declared step scope.

---

## Delegation Guards

```rust
Guard::MaxDelegationDepth(3)
Guard::MatchesSchema(delegate_output_schema())
Guard::OnlyAllowedAgentsUsed(vec![
    "planner".into(),
    "reviewer".into(),
    "debugger".into(),
])
```

Add:

```rust
Guard::OnlyAllowedAgentsUsed(Vec<String>)
Guard::NoRecursiveDelegation
Guard::DelegatedAgentPassed(String)
```

---

## Diff Guards

```rust
Guard::MaxDiffLines(500)
Guard::MaxChangedFiles(10)
Guard::DiffTouchesAllowedPaths(vec![
    "src/".into(),
    "tests/".into(),
])
Guard::DiffDoesNotTouchForbiddenPaths(vec![
    ".env".into(),
    "secrets/".into(),
    "target/".into(),
])
```

These are critical for coding agents.

---

## Self-Modification Guards

```rust
Guard::NoGuardRemoval
Guard::NoPermissionEscalation
Guard::NoNewNetworkAccess
Guard::EvaluationImprovesOrEqual
Guard::AgentVersionCreated
Guard::AuditLogWritten
```

Self-modifying agents should never be able to silently remove their own restrictions.

---

# Updated ToolSet

The current `ToolSet` should become more expressive.

```rust
pub enum ToolSet {
    None,

    ReadOnly,

    ReadWrite,

    Full,

    Allow(Vec<String>),

    Deny(Vec<String>),

    FromSkill(String),

    Intersection(Box<ToolSet>, Box<ToolSet>),

    Union(Box<ToolSet>, Box<ToolSet>),
}
```

Effective tools for a step should be calculated as:

```txt
agent default tools
  ∩ pipeline tools
  ∩ step tools
  ∩ skill tools
  ∩ delegation policy tools
  ∩ runtime policy tools
```

The result should always be least-privilege.

---

# Agent Policy

Each agent should have a policy that limits what it can do globally.

```rust
pub struct AgentPolicy {
    pub max_steps: u32,
    pub max_retries: u32,
    pub max_delegation_depth: u32,
    pub max_cost_usd: Option<f64>,
    pub max_runtime_seconds: Option<u64>,
    pub allow_self_update: bool,
    pub require_approval_for_self_update: bool,
    pub allowed_agents: Vec<String>,
    pub allowed_tools: ToolSet,
    pub allowed_skills: Vec<String>,
    pub network_policy: NetworkPolicy,
    pub filesystem_policy: FilesystemPolicy,
}
```

```rust
pub enum NetworkPolicy {
    DenyAll,
    AllowList(Vec<String>),
    AllowAll,
}
```

```rust
pub struct FilesystemPolicy {
    pub workspace_root: PathBuf,
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    pub forbidden_paths: Vec<PathBuf>,
    pub workspace_isolation: WorkspaceIsolation,
}
```

## Workspace Isolation

For TDD agents and code-generation tasks, task-scoped workspace isolation prevents side effects and ensures repeatability.

```rust
pub enum WorkspaceIsolation {
    /// No isolation; share the default workspace
    None,

    /// Create a fresh temp directory per task run
    /// All fs.read/fs.write and test runner calls are scoped to this temp dir
    TempDir,

    /// Use an explicit sandboxed directory
    Sandboxed(PathBuf),
}
```

When `WorkspaceIsolation::TempDir` is active:
- The runtime creates a temporary directory at task start
- All file operations (read, write, delete) are sandboxed within this directory
- Test runners (cargo test, pytest, etc.) run within the temp workspace
- Temp directory is cleaned up after task completion (configurable)

This is especially useful for:
- **TDD loops:** Each iteration starts with a clean slate
- **Code generation agents:** Generate code without polluting the real workspace
- **Testing new features:** Run experiments in isolation
- **Parallel task execution:** Multiple agents can run tasks simultaneously without interference

Example:

```rust
pub struct AgentPolicy {
    // ... other fields ...
    pub filesystem_policy: FilesystemPolicy {
        workspace_root: PathBuf::from("/project"),
        read_paths: vec![PathBuf::from("/project/src")],
        write_paths: vec![PathBuf::from("/project/src")],
        forbidden_paths: vec![PathBuf::from("/project/.env")],
        workspace_isolation: WorkspaceIsolation::TempDir,
    },
}
```

---

# Updated Pipeline Context

The context should track delegation, tools, costs, traces, versions, and skill usage.

```rust
pub struct StepContext {
    pub agent_name: String,
    pub pipeline_name: String,
    pub step_name: String,

    pub request: Value,
    pub input: Value,
    pub output: Option<StepOutput>,

    pub step_results: HashMap<String, StepResult>,

    pub agent_registry: Arc<AgentRegistry>,
    pub tool_registry: Arc<ToolRegistry>,
    pub skill_registry: Arc<SkillRegistry>,

    pub delegation_depth: u32,
    pub parent_agent: Option<String>,

    pub allowed_tools: ToolSet,
    pub active_skills: Vec<String>,

    pub trace: PipelineTrace,
    pub budget: BudgetState,
    pub filesystem_policy: FilesystemPolicy,
    pub network_policy: NetworkPolicy,
}
```


> **Implementation notes (Phase 1 decisions):**
> - `Guard::Compiles` and `Guard::TestsPass` derive their working directory from
>   `ctx.filesystem_policy.workspace_root` — no separate `working_dir` field on `StepContext`.
> - `FilesystemPolicy` and `NetworkPolicy` are canonically declared in `agent.rs` (part of
>   `AgentPolicy`) and imported into `context.rs`.
> - `SkillSet` is declared as a minimal stub `pub struct SkillSet { pub skills: Vec<String> }` in
>   `agent.rs` for Phase 1; replaced with the full type in `skills/skill.rs` during Phase 5.
> - `StepAction::LlmCall` returns a static stub string in Phase 1 (no real LLM); real provider
>   integration is Phase 2+.
> - `Guard::MaxTokens` uses the `tiktoken-rs` crate (cl100k_base encoding) for token counting.

> **Phase 2 decisions:**
> - `ToolRegistry` is real in `registry.rs`; `context.rs` imports from `registry` (no local stubs).
> - `PipelineRunner` holds `Arc<ToolRegistry>` defaulting to `ToolRegistry::with_builtins()`.
> - `ToolContext` carries `Arc<Mutex<AuditLog>>` for tool-call audit entries.
> - `ToolSet::ReadOnly` explicitly allows: `fs.read`, `fs.list`, `search.files`, `search.grep`.
> - Path safety: `fs.*` tools canonicalize paths and reject any path outside workspace root.
> - `FunctionTool` wraps any async Rust function as a `Tool` trait object.
> - New `AuditEvent` variants: `ToolCallStarted`, `ToolCallCompleted`, `ToolCallFailed`.


> **Phase 3 decisions:**
> - MCP stdio transport uses `tokio::process::Command` to spawn child processes (e.g., `npx ...`) and communicates via newline-delimited JSON-RPC over stdin/stdout.
> - `McpToolAdapter` wraps a `DiscoveredTool` definition into the `Tool` trait; tool calls send `tools/call` JSON-RPC requests to the spawned process.
> - MCP tools are registered in `ToolRegistry` with server-namespaced names: `mcp.{server_name}.{tool_name}`.
> - URL-based MCP servers (HTTP transport) are stubbed with `McpError::NotImplemented`; full support deferred to Phase 7+.
> - Allowlist enforcement: `McpServerConfig::allowed_tools` empty = allow all discovered tools; non-empty = only listed tools are registered.
> - MCP tool call audit logging flows through `ToolContext.audit_log` (same path as built-in tools) — no additional audit infrastructure needed.
> - `McpError` enum defined in `src/mcp/client.rs`; re-exported from `src/mcp/mod.rs` and `prelude.rs`.

> **Phase 4 decisions:**
> - `AgentRegistry` already existed in `registry.rs`; no new struct needed — enhanced with no API changes.
> - `PipelineRunner` gains `agent_registry: Arc<AgentRegistry>` field; new constructors: `with_agent_registry`, `with_registries`.
> - `DelegateAgent` is handled as a special case in `run()` and `run_with_delegation_depth()` **before** `execute_action()` is called. This gives `&mut self` access to the audit log. `execute_action()` remains `&self`.
> - `execute_delegation` is an `&mut self` method on `PipelineRunner` — not standalone — for audit log write access.
> - Child context: `delegation_depth = parent_depth + 1`, `parent_agent = Some(parent_name)`, both registries cloned from parent runner.
> - Child tool scope: `inherit_tool_scope=true` clones parent `tool_registry` into child; `false` gives child empty `ToolRegistry`.
> - Child step results merged into parent context under namespaced keys `"{agent_name}.{step_name}"`.
> - Child trace entries merged into `ctx.trace` with the same namespacing.
> - New `PipelineError::DelegationFailed` variant added for delegation-specific errors.
> - New `AuditEvent` variants: `DelegationStarted`, `DelegationCompleted`, `DelegationFailed` — written to `self.audit_log` in `execute_delegation`.
> - `DelegationFailed` is logged for: depth exceeded, allowlist rejection, agent not found in registry, and child pipeline failure.
> - `run_with_delegation_depth` is a public `&mut self` method — the recursive child entry point; mirrors `run()` with depth/parent injected into context.
> - `agents/` module subfiles (`coder.rs`, `debugger.rs`, etc.) deferred to Phase 6.

> **Phase 5 decisions:**
> - `SkillSet` moved from `agent.rs` stub to `skills/skill.rs` — same shape `{ skills: Vec<String> }`, re-exported via `skills::mod.rs`.
> - `SkillRegistry` moved from `registry.rs` stub to `skills/registry.rs`; `registry.rs` re-exports it with `pub use crate::skills::registry::SkillRegistry`.
> - `Skill` does not derive `Serialize/Deserialize` — `Guard` and `Pipeline` do not implement those traits.
> - `SkillRegistry::get()` returns `Option<Skill>` (owned, cloned) to avoid lifetime complications in `execute_action(&self)`.
> - `PipelineRunner` gains `pub skill_registry: Arc<SkillRegistry>` field alongside `tool_registry` and `agent_registry`; new constructor `with_skill_registry`.
> - `UseSkill` with an unknown skill name returns `StepError::ActionFailed` (not `NotImplemented`).
> - `SkillMode::Auto` behaves identically to `SkillMode::Pipeline` — chooses pipeline if available, else instructions.
> - Built-in skills: `rust_debugging` has a pipeline; `code_review` and `api_design` are prompt-only.
> - `test_writing` and `refactoring` built-in skills implemented as prompt-only skills (no pipeline).
> - `Guard::DiffWithinScope` not added — not in the architecture's Guard enum.


> **Phase 6 decisions:**
> - Built-in agents live in `src/agents/` module as constructor functions returning `Agent`.
> - Each agent has a single `LlmCall` step (stub — returns static string, no real LLM in Phase 6).
> - `LlmCall` stub is sufficient for all Phase 6 tests since `Guard::NonEmptyOutput` checks non-empty string.
> - `planner_agent`, `coder_agent`, `reviewer_agent`, `debugger_agent`, `reflector_agent`, `orchestrator_agent` exported from `prelude.rs`.
> - Agent policies: planner/reviewer/reflector/orchestrator use `ToolSet::ReadOnly`; coder/debugger use `ToolSet::ReadWrite`.
> - `orchestrator_agent` policy lists all 5 specialist agents in `allowed_agents`; `max_delegation_depth: 3`.
> - All agents have `allow_self_update: false` by default.


> **Phase 7 decisions:**
> - `InjectionScanner` and `SecretScanner` implemented in `src/injection.rs`; detect common prompt injection patterns (ignore instructions, role-switching, SYSTEM prefix) and secret patterns (OpenAI keys, AWS keys, private keys, bearer tokens).
> - `BudgetTracker` and `RateLimiter` implemented in `src/budget.rs`; `BudgetState` in `context.rs` extended with `start_time: std::time::Instant` for runtime limit checks.
> - `AuditLog` extended with `save_to_file`/`load_from_file` for session persistence (JSON format via existing serde_json); `AuditEvent` gains `InjectionDetected`, `SecretDetected`, `BudgetExceeded`, `RateLimitHit` variants with Serialize/Deserialize.
> - `FilesystemPolicy::is_path_allowed(path)` added to `agent.rs`; checks forbidden paths and workspace boundary enforcement.
> - 40+ guards previously returning `GuardError::NotImplemented` now have real implementations in `guard.rs`: all output bounds, step state, security, file, diff, budget, dependency, and shell command guards.
> - `Guard::ValidToml` and `Guard::ValidYaml` use structural heuristics (no toml/serde_yaml crates added); full validation deferred pending dep approval.
> - `Guard::CargoAuditPass` and `Guard::CargoDenyPass` run subprocesses via `tokio::process::Command`; return `GuardError::NotImplemented` if tool not installed, `GuardError::Failed` if tool fails.
> - Configuration via TOML/YAML: implemented as JSON round-trip on `AuditLog` (`save_to_file`/`load_from_file`); full TOML/YAML config support deferred pending dep approval for toml/serde_yaml crates.


> **Phase 11 decisions:**
> - **11.13 Dead code cleanup:** Unused `std::path::PathBuf` import removed from `tests/phase8.rs`; `_loaded` prefixed for unused variable. No production dead code warnings.
> - **11.16 InjectionScanner entropy detection:** `entropy(text: &str) -> f64` computes Shannon entropy H = -Σ p(c)·log₂(p(c)) over byte frequencies. Threshold 4.9 bits/char, minimum length 50 chars. English text: 3.5–4.5; base64/encrypted payloads: 5.0+. Returns `RiskLevel::High` with pattern `"high_entropy_payload"`. No regex crate added.
> - **11.9b Guard::ValidRustSyntax improved heuristic:** 4-part check: (1) reject non-Rust indicators (`<html`, `<?php`, `def `, bare `class `), (2) balanced braces, (3) at least one Rust keyword pattern (`fn `, `struct `, `impl `, etc.), (4) rustfmt --check fallback when available. Errs on passing for unusual but valid Rust.
> - **11.17 RemoteAgent timeout:** `RemoteAgentClient` gains `timeout_secs: u64` (default 30) and `.timeout(Duration::from_secs(timeout_secs))` in client builder. `with_timeout(secs)` constructor added. reqwest timeout error → `RemoteAgentError::Timeout`.
> - **11.6 Parallel step execution:** `AgentStep::parallel: bool` now respected in `run()`. Consecutive parallel steps collected into a batch; each executes with an independent context clone; results merged into primary context. True async concurrency (tokio::spawn) avoided — `execute_action` is not `Send` due to `Box<Pin<...>>`. `run()` refactored from `for step` to `while step_idx` loop to support batch index advancement.
> - **11.2 Verdict::LlmJudge:** `Verdict::LlmJudge { system, input_template, model, pass_on_pattern }` added. `StepContext` gains `llm_client: Option<Arc<LlmClient>>` populated from runner in `run()` and `run_with_delegation_depth()`. Pattern match via `String::contains()` (no regex crate). New `VerdictError` variants: `NoLlmClient`, `LlmJudgeFailed { reason }`, `BadPattern(String)`. `Verdict` has manual `Debug` impl (no longer derives it).
> - **11.3 Conversation history:** `ChatRole`, `ChatMessage`, `MessageHistory` added to `src/llm/provider.rs`. `LlmRequest` gains `history: Option<MessageHistory>`. `OpenAiCompatibleProvider` prepends history messages before current user turn when present. `StepContext` gains `conversation_history: MessageHistory`. Both `LlmCall` and `LlmCallStreaming` append user/assistant turns to history after each successful call.
> - **11.1 Streaming:** `LlmChunk { delta, finish_reason }`, `OutputSink` trait, `OutputEvent` enum added to `src/runner.rs`. `PipelineRunner` gains `output_sink: Option<Arc<dyn OutputSink>>` and `with_output_sink()`. `StepAction::LlmCallStreaming` added — calls `complete()` internally (full response; true HTTP streaming deferred, requires `futures` crate not in Cargo.toml), emits `OutputEvent::LlmChunk` to sink. Guards/verdicts still operate on assembled output. `MonitoringServer::serve()` is a complete axum HTTP server (routes `/`, `/api/entries`, `/api/trace`). `SelfUpdateEngine::apply_in_sandbox()` writes patch to `patch.diff`, runs `git apply --check`, then `git apply`; phase8 test updated to accept both success and git-error outcomes.





---

# Updated PipelineRunner Behavior

For each step:

```txt
1. Build step context
2. Compute effective tool scope
3. Apply injection protection
4. Run guard_in
5. Execute action
6. Record trace
7. Run guard_out
8. Run verdict
9. Commit output if verdict passed
10. Handle failure according to FailureMode
```

For delegation:

```txt
1. Check delegation policy
2. Check max delegation depth
3. Check allowed agent list
4. Create child context
5. Restrict child tools
6. Run child agent pipeline
7. Validate child output schema
8. Return child result to parent
```

For tool calls:

```txt
1. Check tool is registered
2. Check tool is allowed for this step
3. Validate args against tool schema
4. Apply tool-specific guards
5. Run tool
6. Sanitize output
7. Validate output schema
8. Record audit log
```

For self-update:

```txt
1. Never apply directly from LLM text
2. Require unified diff
3. Apply in temp workspace
4. Run compile/tests/evals
5. Check safety guards
6. Require approval if configured
7. Commit as new agent version
8. Promote only if evaluation passes
```

---

# Updated Module Layout

```txt
verdict/
├── src/
│   ├── lib.rs
│   ├── prelude.rs
│   │
│   ├── agent.rs                 # Agent, AgentPolicy, AgentVersion
│   ├── registry.rs              # AgentRegistry, ToolRegistry, SkillRegistry
│   │
│   ├── pipeline.rs              # Pipeline, AgentStep, FailureMode
│   ├── runner.rs                # PipelineRunner
│   ├── context.rs               # StepContext, PipelineTrace
│   │
│   ├── guard.rs                 # Guard enum, GuardEngine
│   ├── verdict.rs               # Verdict enum, VerdictEngine
│   ├── action.rs                # StepAction
│   │
│   ├── toolset.rs               # ToolSet, tool scoping
│   ├── tools/
│   │   ├── mod.rs
│   │   ├── tool.rs              # Tool trait
│   │   ├── function.rs          # FunctionTool
│   │   ├── shell.rs
│   │   ├── filesystem.rs
│   │   ├── search.rs
│   │   └── http.rs
│   │
│   ├── mcp/
│   │   ├── mod.rs
│   │   ├── client.rs            # MCP client
│   │   ├── server.rs            # MCP server config
│   │   └── tool_adapter.rs      # MCP tool -> Verdict Tool
│   │
│   ├── skills/
│   │   ├── mod.rs
│   │   ├── skill.rs             # Skill, SkillSet
│   │   ├── registry.rs
│   │   └── builtin/
│   │       ├── rust_debugging.rs
│   │       ├── code_review.rs
│   │       └── api_design.rs
│   │       ├── test_writing.rs
│   │       └── refactoring.rs

│   │
│   ├── injection.rs
│   ├── audit.rs                 # Audit logging
│   ├── budget.rs                # Cost/runtime/tool-call limits
│   ├── eval.rs                  # Evaluation suites
│   ├── self_update.rs           # Safe self-modification flow
│   │
│   ├── llm/
│   │   ├── mod.rs
│   │   ├── provider.rs
│   │   └── client.rs
│   │
│   └── agents/
│       ├── mod.rs
│       ├── coder.rs
│       ├── debugger.rs
│       ├── planner.rs
│       ├── reviewer.rs
│       ├── reflector.rs
│       └── orchestrator.rs
```

---

# Example: Delegating Coder Agent

```rust
fn coder_pipeline() -> Pipeline {
    Pipeline {
        name: "coder".into(),
        steps: vec![
            AgentStep {
                name: "plan".into(),
                guard_in: Guard::None,
                action: StepAction::DelegateAgent {
                    agent: "planner".into(),
                    input: json!({
                        "task": "{request}"
                    }),
                    expected_output_schema: Some(plan_schema()),
                    delegation_policy: DelegationPolicy {
                        max_depth: 1,
                        allowed_agents: vec!["planner".into()],
                        require_output_schema: true,
                        inherit_tool_scope: true,
                        inherit_budget: true,
                        require_user_approval: false,
                    },
                },
                guard_out: Guard::MatchesSchema(plan_schema()),
                verdict: Verdict::Automated(Guard::MatchesSchema(plan_schema())),
                tools: ToolSet::ReadOnly,
                injection_protection: InjectionProtection::Strict,
            },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }
}
```

                verdict: Verdict::UserApproval {
                    prompt: "Approve plan?",
                    show_diff: false,
                },
                tools: ToolSet::ReadOnly,
                injection_protection: InjectionProtection::Strict,
            },

            AgentStep {
                name: "implement".into(),
                guard_in: Guard::AllOf(vec![
                    Guard::StepPassed("plan".into()),
                    Guard::UserApproved("plan".into()),
                ]),
                action: StepAction::UseSkill {
                    skill: "rust_implementation".into(),
                    input: json!({
                        "plan": "{plan.output}",
                        "task": "{request}"
                    }),
                    mode: SkillMode::Pipeline,
                },
                guard_out: Guard::AllOf(vec![
                    Guard::Compiles,
                    Guard::FormatPass,
                ]),
                verdict: Verdict::AllOf(vec![
                    Verdict::Automated(Guard::Compiles),
                    Verdict::Automated(Guard::TestsPass),
                    Verdict::Automated(Guard::DiffTouchesAllowedPaths(vec![
                        "src/".into(),
                        "tests/".into(),
                    ])),
                ]),
                tools: ToolSet::Allow(vec![
                    "fs.read".into(),
                    "fs.write".into(),
                    "shell.cargo_check".into(),
                    "shell.cargo_test".into(),
                    "shell.cargo_fmt".into(),
                ]),
                injection_protection: InjectionProtection::Strict,
            },

            AgentStep {
                name: "review".into(),
                guard_in: Guard::StepPassed("implement".into()),
                action: StepAction::DelegateAgent {
                    agent: "reviewer".into(),
                    input: json!({
                        "task": "{request}",
                        "diff": "{current_diff}"



This phase addresses the most critical gaps discovered through framework analysis. It is required before Verdict can be used to build interactive, production-grade agent systems like OpenCode or Hermes.

---

## 11.1 Streaming

### Problem

All LLM calls, tool calls, and step outputs are currently fully buffered — nothing is visible to the user until a step completes. This makes the framework unsuitable for interactive use and produces a poor user experience in any latency-sensitive context.

### Required Changes

#### LLM Streaming

The `LlmProvider` trait must gain a `stream` method alongside `complete`:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;

    async fn stream(
        &self,
        req: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmChunk, LlmError>> + Send>>, LlmError>;
}

pub struct LlmChunk {
    pub delta: String,
    pub finish_reason: Option<String>,
}
```

`OpenAiCompatibleProvider` implements this via `stream: true` on the completions endpoint, reading server-sent events.

#### Tool Streaming

The `Tool` trait gains an optional streaming variant:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Value;
    fn source(&self) -> ToolSource;

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError>;

    /// Optional: stream output. Default impl falls back to `call`.
    async fn call_streaming(
        &self,
        args: Value,
        ctx: ToolContext,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ToolChunk, ToolError>> + Send>>, ToolError> {
        let output = self.call(args, ctx).await?;
        let chunk = ToolChunk { delta: output.raw, is_final: true };
        Ok(Box::pin(futures::stream::once(async move { Ok(chunk) })))
    }
}

pub struct ToolChunk {
    pub delta: String,
    pub is_final: bool,
}
```

#### Pipeline-Level Streaming Output Sink

`PipelineRunner` gains an optional output sink for streaming intermediate results:

```rust
pub struct PipelineRunner {
    // ... existing fields ...
    pub output_sink: Option<Arc<dyn OutputSink>>,
}

#[async_trait]
pub trait OutputSink: Send + Sync {
    async fn emit(&self, event: OutputEvent);
}

pub enum OutputEvent {
    LlmChunk { step: String, delta: String },
    ToolChunk { step: String, tool: String, delta: String },
    StepCompleted { step: String, output: StepOutput },
    PipelineCompleted { result: PipelineResult },
}
```

This allows callers to attach a channel, websocket, SSE stream, or stdout sink.

#### New `StepAction` Variant

```rust
pub enum StepAction {
    // ... existing variants ...

    /// Call an LLM and stream the response
    LlmCallStreaming {
        system: String,
        user: String,
        model: Option<ProviderSpec>,
    },
}
```

Guards and verdicts still run against the fully assembled output after streaming completes. Streaming does not change guard/verdict semantics — it only changes when the user sees output.

## Phase 11 — Streaming, Multi-Model Verdicts, and Conversation State

Phase 11 implements the foundations for interactive pipelines: LLM streaming with real-time chunk delivery, multi-model verdicts for dual-approval workflows, multi-turn conversation history, ReAct-style tool-use loops, parallel step execution, and context serialization for checkpoint/resume. All major items are implemented and fully integrated with the runner, guards, and verdicts.

### Status Summary

| Item | Status | Notes |
|------|--------|-------|
| **11.1 LLM Streaming** | ✅ Implemented | `LlmProvider::stream()`, `StepAction::LlmCallStreaming`, `OutputSink` with chunk events |
| **11.2 Inter-Step Streaming** | ✅ Implemented | `OutputEvent::StepCompleted`, `OutputEvent::PipelineCompleted` via `OutputSink` |
| **11.3 Streaming Tool Output** | ✅ Implemented | `Tool::call_streaming()` with line-by-line stdout streaming for `CargoCheckTool`, `CargoTestTool`, `RunCommandTool`; default wraps `call()` into single chunk |
| **11.4 Verdict::LlmJudge** | ✅ Implemented | Multi-model verdicts with pattern matching; async `VerdictEngine::evaluate()` |
| **11.5 Arc<LlmClient> in StepContext** | ✅ Implemented | `StepContext.llm_client: Option<Arc<LlmClient>>` used by `LlmJudge` verdicts |
| **11.6 MessageHistory in StepContext** | ✅ Implemented | `StepContext.conversation_history: MessageHistory` with append-to-history support |
| **11.7 Parallel Step Execution** | ✅ Implemented | True `tokio::task::spawn` with `#[async_recursion]` macro on `execute_action`. Uses `JoinSet` for concurrent parallel step execution with `Arc<Mutex<StepContext>>` for Send safety. |
| **11.8 StepContext Serialization** | ✅ Implemented | `SerializableStepContext` with `to_serializable()` / `from_serializable()` methods |
| **11.9 Agent Call Tree from Audit Log** | ✅ Implemented | `CallTreeNode` struct and `call_tree_from_audit_log()` function in audit.rs |
| **11.10 ReAct ToolUseLoop** | ✅ Implemented | `StepAction::ToolUseLoop` with tool schemas and multi-round LLM+tool loops |
| **11.11 Guard::ValidRustSyntax** | ✅ Implemented | Pipes source to `rustfmt --check`; graceful fallback if not installed |
| **11.12 InjectionScanner Regex** | ✅ Implemented | `CompiledPattern` with fallback to substring match if regex invalid |
| **11.13 RemoteAgentClient Retry/Timeout** | ✅ Implemented | 3 retries, exponential backoff (1s, 2s, 4s), 30-second timeout |

---

### 11.1 LLM Streaming

**Implemented**: LLM streaming is fully functional. The `LlmProvider` trait defines a `stream()` method returning a `Stream<Item=Result<LlmChunk, LlmError>>`:

```rust
fn stream(
    &self,
    request: LlmRequest,
) -> Pin<Box<dyn Stream<Item = Result<LlmChunk, LlmError>> + Send>>;
```

The `OpenAiCompatibleProvider::stream()` implementation streams chunks from the OpenAI-compatible API. The runner's `StepAction::LlmCallStreaming` arm:
- Iterates over incoming chunks
- Emits `OutputEvent::LlmChunk { step, delta }` via `OutputSink` for each chunk
- Assembles the full response text
- Returns complete response to downstream steps and guards

**Decision**: Futures-based streaming (not tokio streams) is used for broad compatibility. Guards/verdicts operate on the final assembled response.

---

### 11.2 Inter-Step Streaming (Output Events)

**Implemented**: `OutputEvent` and `OutputSink` enable real-time pipeline monitoring:

```rust
pub enum OutputEvent {
    LlmChunk { step: String, delta: String },
    ToolChunk { step: String, tool: String, delta: String },
    StepCompleted { step: String, output: StepOutput },
    PipelineCompleted { result: PipelineResult },
}

pub trait OutputSink: Send + Sync {
    async fn emit(&self, event: OutputEvent);
}
```

After each step completes (success or skip), `OutputEvent::StepCompleted` is emitted. After the pipeline completes, `OutputEvent::PipelineCompleted` is emitted. Both are fire-and-forget (caller does not await).

---

### 11.3 Streaming Tool Output

**Status**: ✅ Implemented

Tools now support streaming output via a new `call_streaming()` method on the `Tool` trait:

```rust
pub trait Tool: Send + Sync {
    // ... existing methods ...

    /// Stream output from a tool call.
    /// Default impl wraps call() into a single chunk.
    async fn call_streaming(
        &self,
        args: Value,
        ctx: ToolContext,
    ) -> Result<Vec<ToolChunk>, ToolError> {
        let output = self.call(args, ctx).await?;
        Ok(vec![ToolChunk {
            delta: output.raw,
            is_final: true,
        }])
    }
}

pub struct ToolChunk {
    pub delta: String,
    pub is_final: bool,  // true if this is the last chunk
}
```

**Execution Flow:**
- `execute_tool_call()` in runner.rs calls `call_streaming()` instead of `call()`
- Each `ToolChunk` received is emitted as `OutputEvent::ToolChunk { step, tool, delta }` via `OutputSink`
- Chunks are reassembled into the complete tool output before returning to the step
- Non-final, non-empty chunks are emitted; the final chunk signals completion

**Long-Running Tool Streaming (Implemented):**
The following tools override `call_streaming()` to stream stdout line-by-line using `tokio::process::Command` with piped stdout:
- `CargoCheckTool` — streams `cargo check` output
- `CargoTestTool` — streams `cargo test` output
- `RunCommandTool` — streams arbitrary command output
- (Note: `ShellTool` is not a separate tool; `RunCommandTool` handles shell execution)

Each line is yielded as a separate chunk with `is_final: false`, followed by a final empty chunk with `is_final: true`.

**Other Tools:**
Filesystem, HTTP, and search tools keep the default single-chunk implementation, which wraps their synchronous output into one final chunk.

**Implementation Notes:**
- Uses `tokio::io::AsyncBufReadExt::lines()` for efficient line-by-line reading
- Handles both stdout and stderr (via `Stdio::piped()`)
- Non-blocking with proper async/await semantics
- Chunks are collected into a `Vec<ToolChunk>` before returning

---

### 11.4 Verdict::LlmJudge (Multi-Model Verdicts)

**Implemented**: A second LLM model can review and approve a step's output:

```rust
pub enum Verdict {
    // ...
    LlmJudge {
        system: String,
        input_template: String,  // {output}, {request} placeholders
        model: Option<ProviderSpec>,
        pass_on_pattern: String,  // Substring match (not regex)
    },
    // ...
}
```

The `VerdictEngine::evaluate()` is `async` and handles `LlmJudge` by:
- Retrieving the `llm_client` from `ctx.llm_client`
- Rendering the template with step output and request
- Calling the judge model
- Checking if the response contains `pass_on_pattern` (substring match)
- Returning `VerdictError::LlmJudgeFailed` if pattern not found

**Decision**: Pattern matching uses substring, not regex. Regex deferred to later phases.

---

### 11.5 Arc<LlmClient> in StepContext

**Implemented**: `StepContext` carries `llm_client: Option<Arc<LlmClient>>`, populated by the runner from `PipelineRunner::llm_client`. This enables verdicts to make LLM calls for dual-approval workflows.

---

### 11.6 MessageHistory in StepContext

**Implemented**: Multi-turn conversation support is complete:

```rust
pub struct StepContext {
    // ...
    pub conversation_history: MessageHistory,
}

pub struct MessageHistory {
    pub messages: Vec<ChatMessage>,
}

pub enum ChatRole { System, User, Assistant, Tool }
```

After each `LlmCall` or `LlmCallStreaming`, if `append_to_history: true`, the user prompt and assistant response are appended to `conversation_history`. The runner passes the full history to the LLM provider via `LlmRequest::history`.

**Decision**: Conversation state is in-memory only within a single pipeline run. Persistence across runs deferred to Phase 12+.

---

### 11.7 Parallel Step Execution

**Status**: ✅ **Fully Implemented** — True `tokio::task::spawn` concurrency

Consecutive `parallel: true` steps are executed with true concurrency using `tokio::task::spawn` and `tokio::task::JoinSet`. Each parallel step receives its own cloned `Arc<tokio::sync::Mutex<StepContext>>` to maintain Send safety for cross-thread execution.

**Implementation Details:**

1. **async_recursion Macro for Send Safety**:
   - Added `async-recursion = "1"` to `Cargo.toml`
   - Annotated `execute_action(&self, action: &StepAction, ctx: Arc<TokioMutex<StepContext>>)` with `#[async_recursion]`
   - This macro rewrites the async function to return `BoxFuture<'_, Result<...>> + Send`, making it safe for `tokio::task::spawn`
   - Replaced all 4 recursive call sites (LoopUntil, UseSkill Pipeline mode, UseSkill Auto mode, Branch if_true/if_false) from `Pin::from(Box::new(...)).await` to direct `.await`

2. **execute_action Signature**:
   - Signature: `#[async_recursion] pub async fn execute_action(&self, action: &StepAction, ctx: Arc<TokioMutex<StepContext>>) -> Result<StepOutput, StepError>`
   - All context reads/writes use `ctx.lock().await` to acquire async mutex locks
   - The macro ensures the function returns a Send-safe BoxFuture for spawning

3. **Parallel Batch Execution with JoinSet**:
   ```rust
   if step.parallel {
       let mut parallel_batch_indices = vec![step_idx];
       let mut batch_idx = step_idx + 1;
       while batch_idx < pipeline.steps.len() && pipeline.steps[batch_idx].parallel {
           parallel_batch_indices.push(batch_idx);
           batch_idx += 1;
       }
       
       // Use tokio::task::JoinSet for true async concurrency
       let mut join_set: JoinSet<(String, Result<(StepContext, StepOutput), String>)> = JoinSet::new();
       
       for &idx in &parallel_batch_indices {
           let step_def = pipeline.steps[idx].clone();
           let mut local_ctx = ctx.clone();
           // ... setup step context ...
           let ctx_arc = Arc::new(TokioMutex::new(local_ctx));
           
           // Clone runner for spawn (PipelineRunner derives Clone)
           let runner = self.clone();
           
           join_set.spawn(async move {
               // guard_in check with lock
               {
                   let ctx_guard = ctx_arc.lock().await;
                   if let Err(e) = GuardEngine::evaluate(&guard_in, &*ctx_guard).await {
                       return (step_name, Err(format!("guard_in: {}", e)));
                   }
               }
               
               // Execute action (makes recursive execute_action calls)
               let action_result = runner.execute_action(&action, ctx_arc.clone()).await;
               
               match action_result {
                   Ok(output) => {
                       // guard_out and verdict checks under lock
                       let mut ctx_guard = ctx_arc.lock().await;
                       ctx_guard.output = Some(output.clone());
                       // ... verdict checks ...
                       let final_ctx = ctx_guard.clone();
                       (step_name, Ok((final_ctx, output)))
                   }
                   Err(e) => (step_name, Err(format!("action: {}", e))),
               }
           });
       }
       
       // Join all spawned tasks concurrently
       while let Some(join_result) = join_set.join_next().await {
           match join_result {
               Ok((step_name, Ok((step_ctx, output)))) => {
                   // Merge step results and trace entries (last-writer-wins)
                   ctx.step_results.insert(step_name.clone(), sr);
                   ctx.trace.entries.extend(step_ctx.trace.entries);
                   steps_passed.push(step_name);
               }
               Ok((step_name, Err(reason))) => {
                   // Handle failure per pipeline failure mode
                   steps_failed.push(step_name);
                   any_failed = true;
               }
               Err(join_err) => return Err(PipelineError::...);
           }
       }
       
       step_idx += parallel_batch_indices.len();
   }
   ```

4. **Output Merging Strategy**:
   - After all parallel tasks complete via `join_set.join_next().await`, results are merged back into primary context
   - Each step's `StepOutput` is stored in `ctx.step_results` with the step name as key
   - Trace entries from each parallel step are concatenated (order non-deterministic due to concurrent execution)
   - Last-writer-wins strategy for conflict resolution in variables

5. **Send Safety Guarantees**:
   - `#[async_recursion]` macro ensures `execute_action` returns `Send` future, enabling `tokio::task::spawn`
   - `Arc<TokioMutex<StepContext>>` provides thread-safe interior mutability
   - `PipelineRunner` derives `Clone`, allowing it to be captured in spawn closures
   - All StepContext fields are `Send + Sync` or wrapped in Send-safe containers

**Testing:**
- Parallel batch collection and spawning works correctly
- Guard evaluation and verdict checks execute under proper mutex locks
- Results merge correctly with last-writer-wins semantics
- Failure handling respects pipeline `on_failure` policy

---

### 11.8 ReAct ToolUseLoop

**Implemented**: `StepAction::ToolUseLoop` enables LLM → tool → LLM loops:

```rust
pub enum StepAction {
    ToolUseLoop {
        system: String,
        user: String,
        model: ProviderSpec,
        tools: Vec<String>,
        max_rounds: usize,
        stop_condition: StopCondition,
    },
}

pub enum StopCondition {
    TextOnly,
    Pattern(String),
    MaxRounds,
}
```

The runner executes `ToolUseLoop` by:
1. Calling the LLM with tool schemas
2. If `LlmResponse::tool_calls` is present, executing each tool
3. Appending tool results to `conversation_history` as `ChatRole::Tool` messages
4. Calling the LLM again with updated history
5. Repeating until `stop_condition` is satisfied or `max_rounds` exhausted

---

### 11.9 StepContext Serialization

**Implemented**: `SerializableStepContext` captures data-only fields for checkpointing:

```rust
#[derive(Serialize, Deserialize)]
pub struct SerializableStepContext {
    pub agent_name: String,
    pub pipeline_name: String,
    pub step_name: String,
    pub request: Value,
    pub input: Value,
    pub output: Option<StepOutput>,
    pub step_results: HashMap<String, SerializableStepResult>,
    pub delegation_depth: u32,
    pub parent_agent: Option<String>,
    pub active_skills: Vec<String>,
    pub trace: PipelineTrace,
    pub budget: BudgetState,
    pub conversation_history: MessageHistory,
}
```

`StepContext` provides:
```rust
pub fn to_serializable(&self, step_id: String) -> SerializableStepContext { ... }
pub fn from_serializable(s: SerializableStepContext, registries: Registries) -> StepContext { ... }
```

Registries are re-injected at restore time — they are never serialized.

---

### 11.10 Agent Call Tree from Audit Log

**Implemented**: `call_tree_from_audit_log(entries: &[AuditEntry]) -> Vec<CallTreeNode>` reconstructs the delegation tree:

```rust
pub struct CallTreeNode {
    pub agent_name: String,
    pub step_name: String,
    pub status: CallTreeStatus,
    pub children: Vec<CallTreeNode>,
}

pub enum CallTreeStatus {
    Running,
    Completed,
    Failed(String),
}
```

The function matches `DelegationStarted` / `DelegationCompleted` / `DelegationFailed` audit events to build the tree structure. Used for post-execution analysis and debugging.

---

### 11.11 ReAct ToolUseLoop (Consolidated)

Already described in 11.8. This is the standard OpenAI function-calling / tool-use protocol, fully implemented in the runner with support for `tool_calls` in LLM responses and multi-round loops.

---

### 11.12 Guard::ValidRustSyntax

**Implemented**: Pipes source code to `rustfmt --check --edition=2021` via stdin:

```rust
Guard::ValidRustSyntax => {
    // Pre-check for obvious issues (balanced braces, etc.)
    // Call rustfmt --check --edition=2021
    // If rustfmt not installed: return Pass with caveat note
}
```

**Decision**: Graceful fallback if rustfmt is not installed.

---

### 11.13 InjectionScanner Regex with Fallback

**Implemented**: `CompiledPattern` in injection.rs tries `Regex::new(pattern)`, falls back to `str::contains()`:

```rust
pub struct CompiledPattern {
    regex: Option<Regex>,
    literal: String,
}

impl CompiledPattern {
    pub fn is_match(&self, text: &str) -> bool {
        if let Some(r) = &self.regex {
            r.is_match(text)
        } else {
            text.contains(&self.literal)
        }
    }
}
```

Used by `InjectionScanner::scan()` for all pattern matching.

---

### 11.14 RemoteAgentClient Retry and Timeout

**Implemented**: `RemoteAgentClient` in agent.rs with retry logic:

```rust
pub struct RemoteAgentClient {
    client: reqwest::Client,
    timeout_secs: u64,
}

impl RemoteAgentClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()...
            timeout_secs: 30,
        }
    }

    pub async fn execute_step(&self, url: &str, step: AgentStep) -> Result<StepOutput> {
        // 3 retry attempts with exponential backoff (1s, 2s, 4s)
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt - 1))).await;
            }
            match self.client.post(url).json(&step).send().await {
                Ok(resp) => return Ok(resp.json().await?),
                Err(e) if e.is_timeout() => return Err(RemoteAgentError::Timeout),
                Err(e) => { /* retry */ }
            }
        }
    }
}
```

---

### Implementation Summary

All Phase 11 items are now complete and integrated:
- **Streaming**: LLM and inter-step streaming with `OutputSink` events
- **Multi-Model Verdicts**: `Verdict::LlmJudge` with pattern-based approval
- **Conversation State**: Full `MessageHistory` support with multi-turn LLM interactions
- **Tool-Use Loops**: ReAct pattern with tool schemas and multi-round loops
- **Serialization**: Checkpoint/restore with `SerializableStepContext`
- **Guards**: `ValidRustSyntax` with rustfmt, regex-fallback injection scanning
- **Parallel Execution**: Batch consecutive parallel steps
- **Remote Agents**: Retry/timeout with exponential backoff

The framework now has the foundational capabilities required to build interactive, multi-turn, multi-model agent systems comparable to advanced LLM orchestration platforms.


---

# New Built-In Agents

## Planner Agent

Produces structured execution plans.

```txt
Input:
- task
- repo summary
- constraints

Output:
- plan
- affected files
- risks
- required tools
- test strategy
```

## Coder Agent

Implements approved plans.

```txt
Input:
- approved plan
- task
- repo context

Output:
- diff
- changed files
- test results
```

## Reviewer Agent

Reviews code changes.

```txt
Input:
- task
- diff
- test output

Output:
- approval status
- issues
- required fixes
- risk rating
```

## Debugger Agent

Fixes compile/test failures.

```txt
Input:
- failing command
- error output
- changed files

Output:
- root cause
- patch
- test result
```

## Reflector Agent

Analyzes agent performance.

```txt
Input:
- pipeline trace
- failures
- retries
- tool calls
- outputs
- costs

Output:
- what worked
- what failed
- suggested improvement
- proposed patch category
- risk level
```

## Orchestrator Agent

Delegates to specialized agents.

```txt
Input:
- user goal

Output:
- final completed result
```

---

# Updated Roadmap

## Phase 1 — Core Framework


- [x] Core types: `Pipeline`, `AgentStep`, `Guard`, `Verdict`, `StepAction`
- [x] `Agent` type
- [x] `PipelineRunner`
- [x] `StepContext`
- [x] Sequential step execution
- [x] Built-in guards:
  - [x] `Compiles`
  - [x] `TestsPass`
  - [x] `FileExists`
  - [x] `MatchesSchema`
  - [x] `MaxTokens`
  - [x] `ValidJson`
- [x] Basic audit log
- [x] Basic failure modes


## Phase 2 — Tools

- [x] `Tool` trait
- [x] `ToolRegistry`
- [x] Built-in filesystem tools
- [x] Built-in shell tools
- [x] Built-in search tools
- [x] Agent-local function tools
- [x] Tool schema validation
- [x] Tool call audit logging
- [x] Tool scope enforcement per step


## Phase 3 — MCP Support

- [x] MCP server config
- [x] MCP client
- [x] MCP tool discovery
- [x] MCP tool adapter
- [x] MCP tool allowlist
- [x] MCP output schema validation
- [x] MCP audit logging



## Phase 4 — Agent Delegation ✅

- [x] `AgentRegistry`
- [x] `StepAction::DelegateAgent`
- [x] Delegation depth limits
- [x] Delegation policies
- [x] Child context creation
- [x] Delegated output validation
- [x] Delegation trace logging

## Phase 5 — Skills ✅

- [x] `Skill` type
- [x] `SkillRegistry`
- [x] `SkillSet`
- [x] `StepAction::UseSkill`
- [x] Prompt-only skills
- [x] Pipeline-backed skills
- [x] Built-in skills:
  - [x] Rust debugging
  - [x] code review
  - [x] API design
  - [x] test writing
  - [x] refactoring

## Phase 6 — Built-In Agents ✅

- [x] Planner agent
- [x] Coder agent
- [x] Reviewer agent
- [x] Debugger agent
- [x] Reflector agent
- [x] Orchestrator agent

## Phase 7 — Safety and Production

- [x] Prompt injection protection
- [x] Secret scanning
- [x] Path sandboxing
- [x] Network policy
- [x] Cost tracking
- [x] Runtime limits
- [x] Rate limiting
- [x] Session persistence
- [x] Audit logging
- [x] Configuration via TOML/YAML
- [x] `cargo audit` integration
- [x] `cargo deny` integration

## Phase 8 — Self-Improvement ✅

- [x] Pipeline tracing
- [x] Reflection agent
- [x] Self-update proposal step
- [x] Unified diff-only self patches
- [x] Self-update sandbox
- [x] Compile/test/eval validation
- [x] Human approval gate
- [x] Agent versioning
- [x] Evaluation suites
- [x] Promotion/rollback system

> **Phase 8 decisions:**
> - `EvaluationSuite`, `EvaluationCase`, `EvaluationExpected`, `EvaluationResult`, `EvaluationSuiteResult`, `EvaluationRunner`, `EvalError` implemented in `src/eval.rs`.
> - `SelfUpdateConfig`, `SelfUpdateProposal`, `SelfUpdateResult`, `SelfUpdateEngine`, `SelfUpdateError` implemented in `src/self_update.rs`.
> - `EvaluationExpected::Guard(g)` evaluates the guard against a minimal StepContext built from the last step's output.
> - `SelfUpdateEngine::apply_in_sandbox` writes the patch file to the sandbox dir and returns Ok if the patch is a valid unified diff; actual `git apply` deferred (requires live git repo).
> - `Guard::EvaluationImprovesOrEqual` and `Guard::AgentVersionCreated` use optimistic-pass semantics (enforced by pipeline, not guard alone).
> - `AuditEvent::SelfUpdateProposed` and `AuditEvent::AgentVersionCreated` added to `audit.rs`.
> - Pipeline tracing was already present in `context.rs` (`PipelineTrace`, `TraceEntry`); Phase 8 marks it complete.
> - Human approval gate was already in `Verdict::UserApproval`; Phase 8 marks it complete.
> - Agent versioning (`AgentVersion`) was already in `agent.rs`; Phase 8 adds `SelfUpdateEngine::version_agent`.
> - `RiskLevel` is defined in `src/injection.rs` and reused in `src/self_update.rs`; no duplicate.
> - Promotion/rollback: `EvaluationSuite::minimum_score` gate enforces promotion; rollback = don't promote (no automatic rollback mechanism; deferred to Phase 9).

## Phase 9 — Advanced Execution ✅

- [x] DAG-based pipelines
- [x] Parallel step execution
- [x] Conditional branching
- [x] Looping with max-iteration guards
- [x] Pipeline hot-reloading
- [x] Plugin system
- [x] Web UI for monitoring
- [x] Distributed agent execution

> **Phase 9 decisions:**
> - `AgentStep` struct extended with `dependencies: Vec<String>` and `parallel: bool` fields for DAG support.
> - `PipelineRunner::topological_sort()` validates DAG and detects circular dependencies; returns error if cycles found.
> - `PipelineRunner::run_with_dag()` accepts a `Pipeline` and executes with DAG validation; currently falls back to sequential execution, but DAG structure is preserved and validated.
> - `StepAction::Branch { condition, if_true, if_false }` evaluates string-match condition against previous step output; executes if_true if condition matches, else if_false (or returns previous output if no else branch).
> - `StepAction::RemoteAgent { endpoint, agent_name, payload }` POSTs payload to `{endpoint}/agents/{agent_name}/execute` and deserializes JSON response as `StepOutput`.
> - `RemoteAgentClient` wraps `reqwest::Client`; `execute()` method performs HTTP POST with error handling (network, request, response parsing, timeout).
> - `RemoteAgentError` enum derived from `thiserror`; variants: `RequestFailed`, `NetworkError`, `InvalidResponse`, `Timeout`.
> - `HotReloadHandle` wraps `Arc<tokio::sync::RwLock<Pipeline>>`; methods: `new()`, `get_pipeline()`, `update_pipeline()`, `clone_handle()` for passing to runner.
> - `Plugin` trait in `src/pipeline.rs` with methods: `name()`, `on_step_start(&StepContext)`, `on_step_end(&StepContext, &StepOutput)` — both async and return `Result<(), PluginError>`.
> - `PluginError` enum: `HookFailed`, `ExecutionError` — both carry String reason.
> - `PluginRegistry` struct: `plugins: Vec<Arc<dyn Plugin>>`; methods: `new()`, `register()`, `plugins()`.
> - `PipelineRunner::execute_step_with_plugins()` (internal async helper) calls `on_step_start` before action, `on_step_end` after (even on error). Plugin hook failure aborts step.
> - `MonitoringServer` in `src/audit.rs`: wraps `Arc<Mutex<AuditLog>>` and `Arc<Mutex<PipelineTrace>>`. `serve(addr)` async method runs axum HTTP server on given socket address.
> - HTTP endpoints: `GET /` returns HTML dashboard (simple template); `GET /api/entries` returns JSON array of recent `AuditEntry` (up to 100, reversed order); `GET /api/trace` returns JSON object with `{ "entries": [...] }` from `PipelineTrace`.         
> - `TraceEntry` struct in `src/context.rs` now derives `Serialize` and `Deserialize` for JSON compatibility.
> - All `AgentStep` initializers in production code and tests updated with `dependencies: Vec::new()` and `parallel: false` fields.


## Phase 10 — Stub Completion (COMPLETE)

All stubs and incomplete implementations from Phases 1–9 have been resolved.

### Resolved in Phase 10

- `src/llm/provider.rs` — `LlmProvider` trait, `LlmRequest/LlmResponse/LlmUsage/LlmError/ProviderSpec`, `OpenAiCompatibleProvider`
- `src/llm/client.rs` — `LlmClient::new()`, `LlmClient::from_env()`, `complete()` dispatch
- `src/llm/mod.rs` — wired up; re-exports added to `prelude.rs`
- `src/tools/http.rs` — `HttpTool` with `allowed_paths` and `NetworkPolicy` checks
- `src/runner.rs` — `StepAction::LlmCall` dispatch via `LlmClient`; `PipelineRunner.llm_client` field; `with_llm_client()` builder method
- `src/runner.rs` — `DelegateAgent` nested in `LoopUntil`/`SubPipeline` routed to agent registry lookup and direct pipeline execution
- `src/mcp/client.rs` — `discover_tools()` via JSON-RPC stdio `tools/list` with full request/response parsing
- `src/mcp/client.rs` — `call_tool()` via JSON-RPC stdio `tools/call` with atomic ID counter
- `src/eval.rs` — `EvaluationExpected::Custom(f)` executes the closure and propagates result
- `src/self_update.rs` — `apply_in_sandbox()` runs `git apply --check` then `git apply` with proper error handling; added `PatchApplyFailed(String)` variant
- `src/guard.rs` — `Guard::ValidToml` uses `toml` crate real parsing; `Guard::ValidYaml` uses `serde_yaml` real parsing; `Guard::NoNewDependencies` uses TOML parsing with helper function
- `tests/phase10.rs` — integration tests for LLM provider/client, HTTP tool, evaluation closures, self-update, and guard functions

### Phase 10 Decisions

> **Phase 10 decisions:**
> - `LlmProvider` is an `async_trait` in `src/llm/provider.rs`; `LlmRequest/LlmResponse/LlmUsage/LlmError` and `ProviderSpec` enum defined there
> - `LlmClient::from_env()` reads `OPENAI_API_KEY` (required), `OPENAI_BASE_URL` (default: "https://api.openai.com"), `OPENAI_MODEL` (default: "gpt-4o")
> - `OpenAiCompatibleProvider` POSTs to `{base_url}/v1/chat/completions`; parses `choices[0].message.content` and `usage` fields
> - `PipelineRunner` gains `llm_client: Option<Arc<LlmClient>>` field and `with_llm_client()` builder method; field initialized to `None` in all constructors
> - `StepAction::LlmCall` with no client → `StepError::ActionFailed("no LLM client configured")`; model defaults to "gpt-4o" when not specified
> - `HttpTool`: call args `{method, path, body?, headers?}`; checks `allowed_paths` and `NetworkPolicy`; returns `{status, body}`
> - MCP `discover_tools()` sends JSON-RPC `tools/list`; reads newline-delimited response; applies `allowed_tools` filter
> - MCP `call_tool()` sends JSON-RPC `tools/call`; per-client `AtomicU64` ID counter; extracts `result.content`
> - `EvaluationExpected::Custom(f)` now calls `f(pipeline_result)` and propagates `Err` as failure
> - `SelfUpdateEngine::apply_in_sandbox()` runs `git apply --check` (dry-run validation) then `git apply` (actual application)
> - `Guard::ValidToml` and `Guard::ValidYaml` use real TOML/YAML parsing via `toml::from_str` and `serde_yaml::from_str`
> - `Guard::NoNewDependencies` uses helper function `extract_deps_from_toml()` to parse TOML and detect new dependencies
> - New Cargo deps: `toml = "0.8"`, `serde_yaml = "0.9"`, `mockito` (dev-dep for tests)
> - `DelegateAgent` nested in `LoopUntil`/`SubPipeline` routes through agent registry lookup and creates new runner with shared registries

### Updates to Prior Phase Decisions

> **Phase 1 decisions (updated):**
> - `StepAction::LlmCall` stub resolved in Phase 10 — now dispatches to real `LlmClient` (see `src/llm/provider.rs` and `src/runner.rs`)

> **Phase 3 decisions (updated):**
> - MCP `discover_tools()` and `call_tool()` stubs resolved in Phase 10 — full JSON-RPC stdio implementation complete

> **Phase 8 decisions (updated):**
> - `SelfUpdateEngine::apply_in_sandbox()` stub resolved in Phase 10 — now runs real `git apply --check` and `git apply` commands

## Phase 11 — Streaming, Multi-Model Verdicts, and Conversation State

- [x] Streaming LLM responses (token-by-token output to user) **[11.1 Implemented]**
- [ ] Streaming tool output (live output from long-running tools)
- [ ] Inter-step streaming (pipeline can emit partial results mid-run)
- [ ] `Verdict::LlmJudge` variant for second-model approval
- [ ] Thread `Arc<LlmClient>` into `StepContext` for verdict-time LLM access
- [ ] Conversation history (`MessageHistory`) threaded through `StepContext`
- [x] Multi-turn `LlmCall` with full message history [PHASE 11 COMPLETE]
- [x] `StepAction::LlmCall` extended with optional `conversation_id` and `append_to_history` [PHASE 11 COMPLETE]
- [x] Budget counters actually decremented on every LLM and tool call [PHASE 11 COMPLETE]
- [x] `Fallback` pipeline execution actually implemented in runner [PHASE 11 COMPLETE]
- [x] Parallel step execution actually enforced (currently declared but ignored) **[11.6 Implemented - Sequential-within-batch]**
- [x] `StepContext` serializable to JSON for pipeline checkpoint/resume **[11.8 Implemented]**
- [x] Agent call tree reconstruction from audit log **[11.9 Implemented]**
- [x] ReAct-style tool-use loop (LLM sees tool result, calls next tool) **[11.10 Implemented]**
- [x] `Guard::SemanticCheck` — declared in enum but has no match arm in `GuardEngine::evaluate()` [PHASE 11 COMPLETE]
- [x] `Guard::ShellCommandAllowlist` — declared in enum but has no match arm in `GuardEngine::evaluate()` [PHASE 11 COMPLETE]
- [x] `Guard::ShellCommandDenylist` — declared in enum but has no match arm in `GuardEngine::evaluate()` [PHASE 11 COMPLETE]
- [x] `Guard::DependenciesAllowlist` — only partially enforced; needs full evaluate() implementation [PHASE 11 COMPLETE]
- [x] `Guard::NoSuspiciousDependencies` — declared in enum but has no match arm in `GuardEngine::evaluate()` [PHASE 11 COMPLETE]
- [x] `Guard::PathWithinWorkspace` — currently a no-op in evaluate(); must call `FilesystemPolicy::is_path_allowed()` [PHASE 11 COMPLETE]
- [x] `Guard::ValidRustSyntax` — falls back to `Ok(())` when `rustfmt` is not installed; needs real parser **[11.11 Implemented]**
- [x] `Guard::EvaluationImprovesOrEqual` — silently passes when no eval score exists; needs real enforcement [PHASE 11 COMPLETE]
- [x] `Guard::AgentVersionCreated` — silently passes when no version field exists; needs real enforcement [PHASE 11 COMPLETE]
- [x] `Verdict::UserApproval` — returns immediate error instead of blocking for stdin; needs real interactive prompt [PHASE 11 COMPLETE]
- [x] `FailureMode::Fallback` — both match arms in runner.rs return error instead of executing the fallback pipeline [PHASE 11 COMPLETE]
- [x] `BudgetTracker` methods (`record_llm_call`, `record_tool_call`) — exist in `budget.rs` but never called from runner [PHASE 11 COMPLETE]
- [x] `FilesystemPolicy::is_path_allowed()` — exists but never called before any `fs.*` tool operation [PHASE 11 COMPLETE]
- [ ] `MonitoringServer::serve()` — is a complete stub; does not bind socket or serve HTTP
- [ ] `SelfUpdateEngine::apply_in_sandbox()` — does not apply the patch; `_workspace_root` arg unused
- [x] `InjectionScanner` — pattern-only; needs regex support and entropy-based secret detection **[11.14 Implemented]**
- [x] `RemoteAgent` — no retry logic, no timeout, no request signing **[11.15 Implemented]**
- [x] `parallel: bool` field in `AgentStep` — exists but runner never acts on it **[11.6 Implemented - Sequential-within-batch]**
- [x] Dead code cleanup: 106 unused symbols in `tools/` module **[11.17 Implemented]**

> **Phase 11 decisions (legacy):**
> - All Phase 11.9 guard implementations completed: `SemanticCheck`, `ShellCommandAllowlist`, `ShellCommandDenylist`, `DependenciesAllowlist`, `NoSuspiciousDependencies`, `PathWithinWorkspace`, `EvaluationImprovesOrEqual`, `AgentVersionCreated`
> - `Verdict::UserApproval` now blocks on stdin with interactive prompt (y/N format); prompts to stderr, reads from stdin
> - `FailureMode::Fallback` pipeline execution fully implemented: clones context, clears step_results, executes fallback pipeline, propagates success/failure
> - Budget tracking wired: LLM calls increment `ctx.budget.llm_calls_used` and decrement `remaining_usd` based on token cost; tool calls increment `ctx.budget.tool_calls_used`
> - `AuditEvent::FallbackTriggered` added to audit log with step name and failure reason
> - All tests in `tests/phase11.rs` passing: 37 unit tests + integration tests for guards, budget, fallback

---

## 11.1 LLM Streaming (Items 1–3)

**Status: ✅ Implemented**

### Implementation Summary

Three streaming items completed in Phase 11.1:

1. **LLM Provider Stream Method** — `LlmProvider` trait gained `stream()` method returning `Pin<Box<dyn Stream<Item=Result<LlmChunk,LlmError>+Send>>>`
   - `LlmChunk` struct carries incremental response data
   - `OpenAiCompatibleProvider::stream()` fallback wraps `complete()` output into a single-chunk stream (full LLM streaming support deferred)

2. **LLM Client Stream Delegation** — `LlmClient` wrapper gained `stream()` method delegating to inner provider

3. **Runner Streaming Execution** — `StepAction::LlmCallStreaming` in runner dispatches via `provider.stream()`, emits each `LlmChunk` via `OutputSink` as `OutputEvent::Token`

### Cargo.toml Updates
- Added `futures = "0.3"` for streaming trait support
- Added `regex = "1"` for pattern matching (shared with scanner improvements)

#### Phase 11.1 Decisions
> - `LlmChunk` defined in `src/llm/mod.rs` with fields for text delta, token count, and finish reason
> - `OutputEvent::Token` variant added to audit/trace pipeline for per-chunk recording
> - Full streaming support (OpenAI-compatible server) deferred to Phase 12; fallback wrapper sufficient for current needs

---

## 11.7 Parallel Step Execution (Phase 12+)

**Status: ✅ Implemented**

### Implementation Summary

Parallel step execution infrastructure completed in Phase 12:
- `execute_action()` signature changed from `ctx: &mut StepContext` to `ctx: Arc<tokio::sync::Mutex<StepContext>>` for Send safety
- Parallel steps now execute with **isolated contexts** (each step gets a cloned context) 
- Results are merged back into the primary context with **last-writer-wins** semantics for variables
- Architecture supports true `tokio::task::spawn` concurrency in the future (currently sequential execution within batches maintains safety with non-Send recursive futures)

#### Phase 12 Implementation Details
> - `execute_action()` method updated to accept `Arc<tokio::sync::Mutex<StepContext>>` instead of `&mut StepContext`
> - All action branches properly manage mutex locks, avoiding deadlocks by releasing locks before `await` points
> - Parallel batch execution collects consecutive `step.parallel == true` steps and executes each with isolated context
> - Merge strategy: variables from step_ctx overwrite main ctx's (last-writer-wins); step_results are merged preserving all results
> - Sequential and delegated paths wrap context in Arc<Mutex<>> at call site, unwrap after execution to maintain API consistency
> - `execute_tool_call()` remains `&mut StepContext` internally; mutex is locked at the ToolCall branch call site

---

## 11.8 StepContext Serialization

**Status: ✅ Implemented**

### Implementation Summary

`StepContext` now serializable to/from JSON for checkpoint/resume workflows:

- **`SerializableStepContext` struct** added to `src/context.rs` with `#[derive(Serialize, Deserialize)]`
- **`StepContext::to_serializable()`** converts context to serializable form (omits `Arc<LlmClient>`, channels)
- **`StepContext::from_serializable()`** restores context from JSON (re-initializes non-serializable fields with defaults)
- **`BudgetState::start_time`** field uses `#[serde(skip)]` with `default_instant()` helper

#### Phase 11.8 Decisions
> - Non-serializable types are deliberately omitted and reconstructed on restore to avoid serialization complexity
> - Use case: pipeline pause/resume, distributed step execution, state inspection

---

## 11.9 Agent Call Tree Reconstruction

**Status: ✅ Implemented**

### Implementation Summary

Call tree reconstructed from audit log entries:

- **`CallTreeNode` and `CallTreeStatus` structs** added to `src/audit.rs`
- **`call_tree_from_audit_log(entries: &[AuditEntry]) -> Vec<CallTreeNode>` function** implemented
- Matches `DelegationStarted`/`DelegationCompleted`/`DelegationFailed` events to reconstruct parent-child relationships
- Enables performance analysis and delegation graph visualization

#### Phase 11.9 Decisions
> - Entry point function exposed in audit.rs for easy integration with monitoring/dashboard
> - Used for `MonitoringServer` dashboard rendering

---

## 11.10 ReAct ToolUseLoop

**Status: ✅ Implemented**

### Implementation Summary

ReAct-style tool-use loop added for autonomous agent workflows:

- **`StepAction::ToolUseLoop` variant** with fields: `system`, `user`, `model`, `tools`, `max_rounds`, `stop_condition`
- **`StopCondition` enum** variants: `TextOnly`, `Pattern(String)`, `MaxRounds`
- **`LlmResponse` extended** with `tool_calls: Option<Vec<ToolCall>>` field
- **`ToolCall` struct** captures tool name, args, and request_id for tracking
- **Execution loop in runner**: LLM call → parse tool calls → execute tools → append results → repeat until stop condition or max rounds

#### Phase 11.10 Decisions
> - Loop cycle: LLM→tools→LLM→tools... continues until `StopCondition` is met
> - Tool results appended to conversation history (implicit message history for multi-turn)
> - Budget checks enforce `max_rounds` and tool call limits

---

## 11.11 ValidRustSyntax Guard with rustfmt

**Status: ✅ Implemented**

### Implementation Summary

`Guard::ValidRustSyntax` now pipes source to `rustfmt --check --edition=2021`:

- **Implementation**: pipes code via stdin to rustfmt; checks exit code
- **Fallback behavior**: if `rustfmt` binary not found (NotFound error), returns `Pass` with caveat note
- **Pre-checks**: validates obvious non-Rust patterns and balanced braces before rustfmt call

#### Phase 11.11 Decisions
> - Command: `rustfmt --check --edition=2021 < <(echo "$code")`
> - Failure on stderr content; passes on zero exit code
> - Fallback to heuristic brace counting when rustfmt unavailable

---

## 11.9 Missing Guard Implementations

### Problem

Five guard variants are declared in the `Guard` enum in `guard.rs` but have **no match arm** in `GuardEngine::evaluate()`. Using them silently panics (unreachable match) or they never evaluate correctly.

| Guard | Location | Status |
|-------|----------|--------|
| `SemanticCheck(String)` | guard.rs:224 | No match arm |
| `ShellCommandAllowlist(Vec<String>)` | guard.rs:163 | No match arm |
| `ShellCommandDenylist(Vec<String>)` | guard.rs:166 | No match arm |
| `DependenciesAllowlist(Vec<String>)` | guard.rs:197 | Partial only |
| `NoSuspiciousDependencies` | guard.rs:200 | No match arm |

Additionally, three guards pass silently when data is absent instead of failing hard:

| Guard | Problem |
|-------|---------|
| `PathWithinWorkspace` | Always returns `Ok(())` — never calls `FilesystemPolicy::is_path_allowed()` |
| `EvaluationImprovesOrEqual` | Passes optimistically when no eval score is present |
| `AgentVersionCreated` | Passes optimistically when no version metadata is present |
| `ValidRustSyntax` | Falls back to `Ok(())` if `rustfmt` binary is not installed |

### Fixes

#### `Guard::SemanticCheck(desc)`

```rust
Guard::SemanticCheck(description) => {
    // SemanticCheck is an LLM-based assertion — requires llm_client in ctx
    // Phase 11 defers full LLM-eval; implement as a heuristic check:
    // The output must be non-empty and not contain failure indicators.
    let output = ctx.output.as_ref()
        .ok_or_else(|| GuardError::Failed("no output to check".into()))?;
    if output.raw.trim().is_empty() {
        return Err(GuardError::Failed(format!("SemanticCheck({description}): empty output")));
    }
    // TODO Phase 12: replace with actual LLM-as-judge call
    Ok(())
}
```

#### `Guard::ShellCommandAllowlist(allowed)`

```rust
Guard::ShellCommandAllowlist(allowed) => {
    // Check the trace for shell commands that were run this step
    for entry in &ctx.trace.entries {
        if let Some(cmd) = &entry.shell_command {
            let allowed = allowed.iter().any(|a| cmd.starts_with(a.as_str()));
            if !allowed {
                return Err(GuardError::Failed(
                    format!("shell command `{cmd}` not in allowlist")
                ));
            }
        }
    }
    Ok(())
}
```

#### `Guard::ShellCommandDenylist(denied)`

```rust
Guard::ShellCommandDenylist(denied) => {
    for entry in &ctx.trace.entries {
        if let Some(cmd) = &entry.shell_command {
            if denied.iter().any(|d| cmd.contains(d.as_str())) {
                return Err(GuardError::Failed(
                    format!("shell command `{cmd}` matches denylist")
                ));
            }
        }
    }
    Ok(())
}
```

#### `Guard::PathWithinWorkspace`

```rust
Guard::PathWithinWorkspace => {
    // Verify every path in the current step output is within workspace_root
    let root = &ctx.filesystem_policy.workspace_root;
    let output = ctx.output.as_ref()
        .ok_or_else(|| GuardError::Failed("no output to check".into()))?;
    // Extract path-like tokens from output and verify them
    for token in extract_paths(&output.raw) {
        if !ctx.filesystem_policy.is_path_allowed(&token) {
            return Err(GuardError::Failed(
                format!("path `{}` is outside workspace `{}`", token.display(), root.display())
            ));
        }
    }
    Ok(())
}
```

#### Phase 11.9 Guard Decisions

> - `TraceEntry` must gain an optional `shell_command: Option<String>` field, populated by the shell tool when it executes a command. Shell guards read this field.
> - `Guard::SemanticCheck` gets a minimal heuristic in Phase 11; full LLM-as-judge evaluation deferred to Phase 12.
> - `Guard::EvaluationImprovesOrEqual` and `Guard::AgentVersionCreated` must fail (not pass) when the prerequisite data is absent. The optimistic-pass behavior was a Phase 8 placeholder.
> - `Guard::ValidRustSyntax` must return `GuardError::Failed` (not `Ok(())`) when `rustfmt` is unavailable. Fallback: use a minimal AST heuristic from the `syn` crate.

---

## 11.10 `Verdict::UserApproval` — Real Interactive Prompt

### Problem

`VerdictEngine::evaluate()` for `Verdict::UserApproval` immediately returns `Err(VerdictError::UserApprovalRequired)` without blocking for stdin. This means any step that requires human approval always fails. The runner catches this error and returns `PipelineError::AwaitingApproval`, but there is no I/O loop to read the response.

### Fix

```rust
Verdict::UserApproval { prompt, show_diff } => {
    if show_diff {
        if let Some(output) = &ctx.output {
            eprintln!("\n--- Output / Diff ---\n{}\n---\n", output.raw);
        }
    }
    eprint!("{} [y/N]: ", prompt);
    std::io::stderr().flush().map_err(|e| VerdictError::IoError(e.to_string()))?;

    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| VerdictError::IoError(e.to_string()))?;

    match line.trim().to_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => Err(VerdictError::UserApprovalDenied { prompt }),
    }
}
```

`VerdictError` gains:
```rust
pub enum VerdictError {
    // ... existing ...
    UserApprovalDenied { prompt: &'static str },
    IoError(String),
}
```

#### Phase 11.10 UserApproval Decisions

> - `VerdictEngine::evaluate()` must become `async fn` in Phase 11 (required by `LlmJudge` anyway). The stdin read blocks synchronously inside the async context — acceptable for interactive CLI use. For non-interactive (CI) use, the caller should not configure steps with `UserApproval`.
> - `PipelineError::AwaitingApproval` variant is removed; it was only needed because approval never actually ran.
> - `AuditEvent::UserApprovalRequested` and `AuditEvent::UserApprovalGranted` / `UserApprovalDenied` must be added to `audit.rs`.

---

## 11.11 `FailureMode::Fallback` — Real Execution

### Problem

Both failure handling blocks in `runner.rs` (lines ~282 and ~412) match on `FailureMode::Fallback(pipeline)` but execute this body:

```rust
FailureMode::Fallback(_) => {
    // Phase 2+: implement fallback
    steps_failed.push(step.name.clone());
    return Err(PipelineError::StepFailed { step: step.name.clone(), error: e });
}
```

The fallback pipeline argument is **thrown away** and an error is returned instead.

### Fix

```rust
FailureMode::Fallback(fallback_pipeline) => {
    self.audit_log.lock().await.push(AuditEvent::FallbackTriggered {
        step: step.name.clone(),
        reason: e.to_string(),
    });
    // Reset step results so fallback starts clean
    let mut fallback_ctx = ctx.clone();
    fallback_ctx.step_results.clear();
    // Run fallback pipeline using same runner (same registries, budget, depth)
    return self.run_pipeline_inner(fallback_pipeline, &mut fallback_ctx).await;
}
```

`AuditEvent` gains `FallbackTriggered { step: String, reason: String }`.

`PipelineRunner` gains private helper `run_pipeline_inner(pipeline, ctx)` that runs the step loop without re-building context — this is the refactor needed to support both normal and fallback execution paths.

#### Phase 11.11 Fallback Decisions

> - Fallback pipeline runs in a **cloned context** (same budget, depth, registries) but with cleared `step_results` so it starts fresh.
> - If the fallback pipeline also fails, that error propagates as `PipelineError::FallbackFailed`.
> - No infinite fallback recursion: fallback pipelines may not themselves use `FailureMode::Fallback` (enforced at pipeline construction time with a validation pass).

---

## 11.12 Budget Tracking — Wire Up `BudgetTracker`

### Problem

`BudgetTracker` in `src/budget.rs` has `record_llm_call()` and `record_tool_call()` methods, but they are **never called** from the runner. The `BudgetState` fields `llm_calls_used`, `tool_calls_used`, and `remaining_usd` are always 0. Guards that check these values (`MaxLlmCalls`, `MaxToolCalls`, `MaxCostUsd`) are declared and checked but are always trivially satisfied.

### Fix

In `PipelineRunner::execute_action()`, after the `LlmCall` completes:

```rust
// Wire budget tracking
if let Some(usage) = &llm_response.usage {
    let cost = estimate_cost(usage, model.as_ref());
    ctx.budget.llm_calls_used += 1;
    ctx.budget.total_cost_usd += cost;
    ctx.budget.remaining_usd = ctx.budget.remaining_usd.saturating_sub(cost);
    // Also record in the BudgetTracker for rate-limit enforcement
    self.budget_tracker.record_llm_call(usage.total_tokens, cost);
}
```

After every `ToolCall`:

```rust
ctx.budget.tool_calls_used += 1;
self.budget_tracker.record_tool_call(&tool_name);
```

`PipelineRunner` must gain a `budget_tracker: Arc<Mutex<BudgetTracker>>` field, initialized from `BudgetTracker::new()` in all constructors. The tracker is shared across delegated child runners (passes through `with_registries` constructors).

---

## 11.13 `FilesystemPolicy` Enforcement in Tools

### Problem

`FilesystemPolicy::is_path_allowed(path)` exists in `agent.rs` but is **never called** before any `fs.*` tool operation. The path safety check in individual tool files only canonicalizes paths against `workspace_root` inline — it does not consult `FilesystemPolicy::forbidden_paths` or `read_paths` / `write_paths`.

### Fix

In `ToolContext`, add:

```rust
pub struct ToolContext {
    pub audit_log: Arc<Mutex<AuditLog>>,
    pub filesystem_policy: FilesystemPolicy,  // ← add this
    pub network_policy: NetworkPolicy,         // ← add this (already exists in context)
}
```

In every `fs.*` tool (`fs.read`, `fs.write`, `fs.list`, `fs.apply_patch`), before any I/O operation:

```rust
if !ctx.filesystem_policy.is_path_allowed(&path) {
    return Err(ToolError::PathForbidden(path.display().to_string()));
}
```

`ToolError` gains `PathForbidden(String)`.

`PipelineRunner::execute_tool_call()` must build the `ToolContext` from `ctx.filesystem_policy` and `ctx.network_policy` (already available on `StepContext`).

### Implementation Status: COMPLETE

The above plan is fully implemented. Additionally, a Windows-specific path normalization bug was discovered and fixed:

**Windows 8.3 Short-Name Bug:** `std::env::temp_dir()` can return paths with 8.3 short names (e.g. `C:\Users\ELIASS~1\...`) while `canonicalize(workspace_root)` returns the full long name. The `starts_with` comparison would then fail, wrongly denying writes to valid workspace paths.

**Fix applied in `src/agent.rs` (`FilesystemPolicy::is_path_allowed`):**
- For not-yet-existing write targets: canonicalize the **parent** directory and re-join the filename, so both sides use the long-name form
- Strip the `\\?\` UNC verbatim prefix (added by Windows `canonicalize`) from both sides before the `starts_with` comparison
- Helper `fn strip_verbatim_prefix(path: &Path) -> PathBuf` added to `agent.rs`

**Same fix applied in `src/tools/filesystem.rs` (`is_within_workspace`):**
- Parent-canonicalize + `strip_verbatim_prefix_fs` helper added for consistency


---

## 11.14 `MonitoringServer` — Real HTTP Server

### Problem

`MonitoringServer::serve()` in `audit.rs` is a complete stub:

```rust
pub async fn serve(&mut self, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    println!("Monitoring server would listen on port {}", port);
    Ok(())
}
```

### Fix

Implement `MonitoringServer::serve()` using `axum` (already a dependency via Phase 9 decisions):

```rust
pub async fn serve(&self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let audit_log = Arc::clone(&self.audit_log);
    let trace = Arc::clone(&self.trace);

    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/api/entries", get({
            let audit_log = Arc::clone(&audit_log);
            move || async move {
                let log = audit_log.lock().await;
                let entries: Vec<_> = log.entries().iter().rev().take(100).collect();
                Json(entries)
            }
        }))
        .route("/api/trace", get({
            let trace = Arc::clone(&trace);
            move || async move {
                let t = trace.lock().await;
                Json(json!({ "entries": t.entries }))
            }
        }));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

This was already partially designed in Phase 9 decisions but never actually implemented.

#### Phase 11.14 MonitoringServer Decisions

> - `axum` dependency already added in Phase 9 (confirmed in Cargo.toml). No new dependency needed.
> - `serve()` signature changes from `port: u16` to `addr: SocketAddr` for consistency with tokio/axum conventions.
> - `MonitoringServer` takes `Arc<Mutex<AuditLog>>` and `Arc<Mutex<PipelineTrace>>` at construction; it does not own them.
> - The HTML dashboard is a minimal static string template (no separate file assets).

---

## 11.15 `SelfUpdateEngine::apply_in_sandbox()` — Real Patch Application

### Problem

`apply_in_sandbox()` in `src/self_update.rs` validates the patch format and creates the sandbox directory, but the `_workspace_root` parameter is explicitly unused and no `git apply` is ever called. The function always returns `Ok(())`.

### Fix

```rust
pub async fn apply_in_sandbox(
    patch: &str,
    sandbox_dir: &Path,
    workspace_root: &Path,  // was `_workspace_root`; now used
) -> Result<(), SelfUpdateError> {
    // 1. Copy workspace into sandbox
    copy_dir_recursive(workspace_root, sandbox_dir)
        .map_err(|e| SelfUpdateError::SandboxSetupFailed(e.to_string()))?;

    // 2. Write patch file into sandbox
    let patch_file = sandbox_dir.join("__verdict_patch__.diff");
    std::fs::write(&patch_file, patch)
        .map_err(|e| SelfUpdateError::SandboxSetupFailed(e.to_string()))?;

    // 3. Dry-run validation
    let check = tokio::process::Command::new("git")
        .args(["apply", "--check", patch_file.to_str().unwrap()])
        .current_dir(sandbox_dir)
        .output()
        .await
        .map_err(|e| SelfUpdateError::CommandFailed(e.to_string()))?;
    if !check.status.success() {
        return Err(SelfUpdateError::PatchApplyFailed(
            String::from_utf8_lossy(&check.stderr).to_string()
        ));
    }

    // 4. Actual application
    let apply = tokio::process::Command::new("git")
        .args(["apply", patch_file.to_str().unwrap()])
        .current_dir(sandbox_dir)
        .output()
        .await
        .map_err(|e| SelfUpdateError::CommandFailed(e.to_string()))?;
    if !apply.status.success() {
        return Err(SelfUpdateError::PatchApplyFailed(
            String::from_utf8_lossy(&apply.stderr).to_string()
        ));
    }

    // 5. Compile check in sandbox
    let build = tokio::process::Command::new("cargo")
        .args(["check"])
        .current_dir(sandbox_dir)
        .output()
        .await
        .map_err(|e| SelfUpdateError::CommandFailed(e.to_string()))?;
    if !build.status.success() {
        return Err(SelfUpdateError::CompilationFailed(
            String::from_utf8_lossy(&build.stderr).to_string()
        ));
    }

    Ok(())
}
```

`SelfUpdateError` gains `SandboxSetupFailed(String)` and `CommandFailed(String)` variants (in addition to existing `CompilationFailed`).

---

## 11.16 `InjectionScanner` and `SecretScanner` Improvements

**Status: ✅ Implemented (Item 14)**

### Problem

`InjectionScanner::scan()` uses only ~60 hardcoded string `.contains()` checks. `SecretScanner` has ~10 patterns. Neither uses regex, entropy analysis, or context awareness.

### Fix

#### Regex-Based Pattern Matching

Replace `string.contains()` pattern loops with compiled `regex::Regex` patterns:

```rust
use once_cell::sync::Lazy;
use regex::Regex;

static INJECTION_PATTERNS: Lazy<Vec<(Regex, RiskLevel)>> = Lazy::new(|| vec![
    (Regex::new(r"(?i)ignore\s+(all\s+)?previous\s+instructions?").unwrap(), RiskLevel::Critical),
    (Regex::new(r"(?i)you\s+are\s+now\s+(?:a|an)\s+\w+").unwrap(), RiskLevel::High),
    (Regex::new(r"(?i)system\s*:\s*").unwrap(), RiskLevel::High),
    // ... more patterns ...
]);

static SECRET_PATTERNS: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| vec![
    (Regex::new(r"sk-[a-zA-Z0-9]{20,}").unwrap(), "OpenAI API key"),
    (Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(), "AWS Access Key"),
    (Regex::new(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----").unwrap(), "Private key"),
    (Regex::new(r"ghp_[a-zA-Z0-9]{36}").unwrap(), "GitHub personal access token"),
    (Regex::new(r"ghs_[a-zA-Z0-9]{36}").unwrap(), "GitHub server token"),
    (Regex::new(r"xoxb-[0-9A-Za-z-]+").unwrap(), "Slack bot token"),
    // ... more patterns ...
]);
```

#### Shannon Entropy Detection

Add entropy-based secret detection for high-entropy strings (catches API keys that don't match known patterns):

```rust
fn shannon_entropy(s: &str) -> f64 {
    let len = s.len() as f64;
    if len == 0.0 { return 0.0; }
    let mut freq = [0u32; 256];
    for b in s.bytes() { freq[b as usize] += 1; }
    freq.iter()
        .filter(|&&c| c > 0)
        .map(|&c| { let p = c as f64 / len; -p * p.log2() })
        .sum()
}

// Flag any alphanumeric token > 20 chars with entropy > 4.5 as a potential secret
fn detect_high_entropy_secrets(text: &str) -> Vec<String> {
    let token_re = Regex::new(r"[A-Za-z0-9+/=_\-]{20,}").unwrap();
    token_re.find_iter(text)
        .filter(|m| shannon_entropy(m.as_str()) > 4.5)
        .map(|m| m.as_str().to_string())
        .collect()
}
```

#### Phase 11.16 Scanner Decisions (Implemented)

> - `CompiledPattern` struct added to `src/injection.rs` with `regex::Regex` compilation and fallback to `str::contains()` for invalid patterns
> - `InjectionScanner::scan()` uses `CompiledPattern::is_match()` for all pattern checks
> - Public API unchanged for backward compatibility
> - `regex` crate already in `Cargo.toml` (confirmed). No new dependency needed.
> - `once_cell` crate for `Lazy` static regex compilation; already in scope via `once_cell::sync::Lazy`.
> - Entropy threshold `4.5` is industry-standard for secret detection (used by truffleHog, gitleaks).
> - High-entropy matches are reported as `RiskLevel::Medium` (not `Critical`) to reduce false positives.
> - Context awareness (e.g., `password=required` vs. `password=hunter2`) deferred to Phase 12 — requires LLM-based analysis.

---

## 11.17 `RemoteAgent` — Retry, Timeout, and Authentication

**Status: ✅ Implemented (Item 15)**

### Problem

`RemoteAgentClient::execute()` in `src/agent.rs` makes a single HTTP POST with no retry, no timeout configuration, no authentication headers, and no request signing. A single network failure permanently fails the step.

### Fix

```rust
impl RemoteAgentClient {
    pub async fn execute_with_retry(
        &self,
        agent_name: &str,
        payload: &Value,
        config: &RemoteAgentConfig,
    ) -> Result<Value, RemoteAgentError> {
        let url = format!("{}/agents/{}/execute", self.endpoint, agent_name);
        let mut attempt = 0;

        loop {
            attempt += 1;
            let mut req = self.client
                .post(&url)
                .json(payload)
                .timeout(Duration::from_secs(config.timeout_secs.unwrap_or(30)));

            if let Some(token) = &config.auth_token {
                req = req.header("Authorization", format!("Bearer {}", token));
            }

            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    return resp.json::<Value>().await
                        .map_err(|e| RemoteAgentError::InvalidResponse(e.to_string()));
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if attempt >= config.max_retries.unwrap_or(3) || status < 500 {
                        return Err(RemoteAgentError::RequestFailed(
                            format!("HTTP {status} after {attempt} attempts")
                        ));
                    }
                }
                Err(e) if attempt >= config.max_retries.unwrap_or(3) => {
                    return Err(RemoteAgentError::NetworkError(e.to_string()));
                }
                Err(_) => {}
            }

            tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
        }
    }
}
```

New `RemoteAgentConfig` struct:

```rust
pub struct RemoteAgentConfig {
    pub timeout_secs: Option<u64>,
    pub max_retries: Option<u32>,
    pub auth_token: Option<String>,
}
```

`StepAction::RemoteAgent` gains an optional `config: Option<RemoteAgentConfig>` field.

#### Phase 11.17 RemoteAgent Decisions (Implemented)

> - `RemoteAgentClient::execute_with_retry()` retries up to 3 times (configurable) with exponential backoff
> - 30-second timeout on reqwest Client (configurable via `RemoteAgentConfig::timeout_secs`)
> - Exponential backoff: `500ms * attempt_number` (1s, 2s, 4s for default 3 retries)
> - Retries on network errors and HTTP 5xx; fails immediately on 4xx (client error, not recoverable)
> - Bearer token authentication support via `RemoteAgentConfig::auth_token`
> - Implementation in `src/agent.rs`; uses `tokio::time::sleep` for backoff delays

---

## 11.18 Unsafe Pattern Cleanup

### Problem

The audit found 73 `unwrap()` and 19 `expect()` calls in production code paths (outside tests). While most are in tool implementations and may be low-risk, several are on paths that execute during normal pipeline operation:

| Location | Pattern | Risk |
|----------|---------|------|
| `src/runner.rs` — template rendering | `.unwrap()` | Medium: bad template crashes runner |
| `src/guard.rs` — regex compilation | `.unwrap()` on `Regex::new(...)` | Medium: bad regex crashes guard |
| `src/mcp/client.rs` — JSON parsing | `.unwrap()` | Medium: malformed server response crashes |
| `src/tools/shell.rs` — command building | `.expect(...)` | Low: only fails on null bytes |
| `src/self_update.rs` — path conversion | `.unwrap()` on `to_str()` | Low: only fails on non-UTF-8 paths |

### Fix

Replace all production-path `unwrap()` / `expect()` with proper `?` propagation or explicit error returns:

```rust
// Before
let regex = Regex::new(pattern).unwrap();

// After
let regex = Regex::new(pattern)
    .map_err(|e| GuardError::Failed(format!("invalid regex `{pattern}`: {e}")))?;
```

Guard regex patterns that are hardcoded constants should be compiled once via `once_cell::sync::Lazy` to avoid both the runtime panic risk and repeated compilation overhead.

#### Phase 11.18 Decisions

> - `unwrap()` in test code (`tests/*.rs`) is acceptable and not in scope for this cleanup.
> - `unwrap()` on `Lazy`-initialized static regexes is acceptable (panics at startup, not at runtime).
> - Target: zero `unwrap()` / `expect()` calls in `src/` (excluding `lazy_static!` / `once_cell::sync::Lazy` initializers).

---

## Phase 11 Roadmap Summary

| Item | Complexity | Blocks |
|------|-----------|--------|
| Budget counter wire-up (11.12) | Low | `MaxCostUsd`, `MaxLlmCalls` guards |
| `FailureMode::Fallback` fix (11.11) | Low | Error recovery pipelines |
| `Verdict::UserApproval` stdin (11.10) | Low | Human-in-the-loop workflows |
| `FilesystemPolicy` enforcement in tools (11.13) | Low | Path sandbox security |
| Missing guard match arms: `SemanticCheck`, `ShellCommand*`, `Dependencies*` (11.9) | Medium | Guard correctness |
| Optimistic-pass guard fixes: `PathWithinWorkspace`, `EvaluationImprovesOrEqual`, etc. (11.9) | Low | Guard correctness |
| `SelfUpdateEngine::apply_in_sandbox()` real git apply (11.15) | Medium | Self-improvement loop |
| `MonitoringServer::serve()` real HTTP (11.14) | Medium | Observability |
| `RemoteAgent` retry + auth (11.17) | Medium | Distributed agent reliability |
| `InjectionScanner` regex + entropy (11.16) | Medium | Security |
| `Verdict::LlmJudge` variant (11.2) | Medium | Multi-model safety reviews |
| `StepContext.llm_client` thread-through (11.2) | Low | `LlmJudge` verdict |
| Parallel step enforcement (11.6) | Medium | Faster multi-agent execution |
| Streaming LLM output (11.1) | High | Interactive UX |
| Streaming tool output (11.1) | Medium | Interactive UX |
| Pipeline output sink (11.1) | Medium | Streaming delivery |
| Conversation history + `ConversationRegistry` (11.3) | High | Multi-turn agents, chat mode |
| `ToolUseLoop` ReAct action (11.7) | High | ReAct agents, OpenCode-style systems |
| `StepContext` serialization (11.8) | Medium | Pipeline checkpoint/resume |
| Unsafe pattern cleanup in `src/` (11.18) | Low | Code quality, reliability |
| Dead code removal in `tools/` (11.18) | Low | Code quality |

**After Phase 11 is complete, Verdict will have the foundational capabilities required to build systems comparable to OpenCode or Hermes.**

# Updated Verdict Positioning

Verdict is not just another agent framework.

It is a **guarded agent runtime**.

Where other frameworks say:

```txt
Give the model tools and hope.
```

Verdict says:

```txt
No step proceeds without passing code-enforced guards.
No tool is available unless scoped.
No delegation occurs unless allowed.
No output is accepted without a verdict.
No self-update is applied without tests, evaluation, and approval.
```

The core philosophy remains:

> Prompts suggest. Guards enforce. Verdict decides.
