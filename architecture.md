

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
                    }),
                    expected_output_schema: Some(review_schema()),
                    delegation_policy: DelegationPolicy {
                        max_depth: 1,
                        allowed_agents: vec!["reviewer".into()],
                        require_output_schema: true,
                        inherit_tool_scope: true,
                        inherit_budget: true,
                        require_user_approval: false,
                    },
                },
                guard_out: Guard::MatchesSchema(review_schema()),
                verdict: Verdict::AllOf(vec![
                    Verdict::Automated(Guard::NoSecretsInDiff),
                    Verdict::Automated(Guard::NoPermissionEscalation),
                    Verdict::UserApproval {
                        prompt: "Approve reviewed changes?",
                        show_diff: true,
                    },
                ]),
                tools: ToolSet::ReadOnly,
                injection_protection: InjectionProtection::Strict,
            },

            AgentStep {
                name: "self_reflect".into(),
                guard_in: Guard::TraceAvailable,
                action: StepAction::DelegateAgent {
                    agent: "reflector".into(),
                    input: json!({
                        "trace": "{pipeline_trace}",
                        "task": "{request}",
                        "result": "{pipeline_result}"
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
            },
        ],
        on_failure: FailureMode::Retry,
        max_retries: 3,
    }
}
```

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

## Phase 5 — Skills

- [ ] `Skill` type
- [ ] `SkillRegistry`
- [ ] `SkillSet`
- [ ] `StepAction::UseSkill`
- [ ] Prompt-only skills
- [ ] Pipeline-backed skills
- [ ] Built-in skills:
  - [ ] Rust debugging
  - [ ] code review
  - [ ] API design
  - [ ] test writing
  - [ ] refactoring

## Phase 6 — Built-In Agents

- [ ] Planner agent
- [ ] Coder agent
- [ ] Reviewer agent
- [ ] Debugger agent
- [ ] Reflector agent
- [ ] Orchestrator agent

## Phase 7 — Safety and Production

- [ ] Prompt injection protection
- [ ] Secret scanning
- [ ] Path sandboxing
- [ ] Network policy
- [ ] Cost tracking
- [ ] Runtime limits
- [ ] Rate limiting
- [ ] Session persistence
- [ ] Audit logging
- [ ] Configuration via TOML/YAML
- [ ] `cargo audit` integration
- [ ] `cargo deny` integration

## Phase 8 — Self-Improvement

- [ ] Pipeline tracing
- [ ] Reflection agent
- [ ] Self-update proposal step
- [ ] Unified diff-only self patches
- [ ] Self-update sandbox
- [ ] Compile/test/eval validation
- [ ] Human approval gate
- [ ] Agent versioning
- [ ] Evaluation suites
- [ ] Promotion/rollback system

## Phase 9 — Advanced Execution

- [ ] DAG-based pipelines
- [ ] Parallel step execution
- [ ] Conditional branching
- [ ] Looping with max-iteration guards
- [ ] Pipeline hot-reloading
- [ ] Plugin system
- [ ] Web UI for monitoring
- [ ] Distributed agent execution

---

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
