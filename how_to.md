# How to Build with Verdict

Verdict is a **guarded agent runtime** for Rust. Every piece of work is described as a
`Pipeline` of `AgentStep`s. Each step specifies what it does (`action`), what must be
true before it runs (`guard_in`), what must be true after it runs (`guard_out`), and
whether a human or automated system must approve the result (`verdict`).

---

## Table of Contents

1. [Core concepts](#1-core-concepts)
2. [AgentStep — anatomy of a step](#2-agentstep--anatomy-of-a-step)
3. [StepAction — what a step does](#3-stepaction--what-a-step-does)
4. [Guard — preconditions and postconditions](#4-guard--preconditions-and-postconditions)
5. [Verdict — approval logic](#5-verdict--approval-logic)
6. [ToolSet — scoping tool access](#6-toolset--scoping-tool-access)
7. [InjectionProtection — input safety](#7-injectionprotection--input-safety)
8. [Pipeline — assembling steps](#8-pipeline--assembling-steps)
9. [PipelineRunner — running it all](#9-pipelinerunner--running-it-all)
10. [Passing data between steps](#10-passing-data-between-steps)
11. [LLM calls with per-step models](#11-llm-calls-with-per-step-models)
12. [LoopUntil — iteration until a condition](#12-loopuntil--iteration-until-a-condition)
13. [Full worked example](#13-full-worked-example)

---

## 1. Core Concepts

| Concept | What it is |
|---------|-----------|
| `Pipeline` | An ordered list of `AgentStep`s with failure handling |
| `AgentStep` | A single unit of work with guards, an action, and a verdict |
| `Guard` | A code-enforced check — pass = proceed, fail = abort |
| `Verdict` | Approval logic — automated (guard-based) or human |
| `StepAction` | What the step actually does (LLM call, tool call, loop, …) |
| `ToolSet` | Which tools this step may use |
| `InjectionProtection` | Whether inputs are scanned for prompt injection |
| `PipelineRunner` | Executes the pipeline, enforcing all guards and verdicts |

The core philosophy:
> **Prompts suggest. Guards enforce. Verdict decides.**

---

## 2. AgentStep — Anatomy of a Step

```rust
AgentStep {
    name: "generate_code".into(),          // (1) Unique name within the pipeline
    guard_in: Guard::None,                 // (2) Pre-condition — must pass before action runs
    action: StepAction::LlmCall { … },     // (3) What to do
    guard_out: Guard::NonEmptyOutput,      // (4) Post-condition — must pass after action runs
    verdict: Verdict::Automated(           // (5) Approval — who/what decides the step succeeded
        Guard::NonEmptyOutput
    ),
    tools: ToolSet::None,                  // (6) Which tools the action may call
    injection_protection:                  // (7) Whether to scan inputs for prompt injection
        InjectionProtection::Strict,
    output_schema: None,                   // (8) Optional JSON Schema the output must match
    dependencies: vec!["generate_tests".into()], // (9) DAG — which steps must complete first
    parallel: false,                       // (10) Whether this step may run in parallel
}
```

### Field reference

| Field | Type | Description |
|-------|------|-------------|
| `name` | `String` | Unique identifier for this step. Used by `Guard::StepPassed("name")`, `dependencies`, and `ctx.step_results.get("name")`. |
| `guard_in` | `Guard` | Evaluated **before** the action runs. If it fails, the step is skipped and the pipeline aborts (or retries, per `on_failure`). |
| `action` | `StepAction` | The work to perform. See [§3](#3-stepaction--what-a-step-does). |
| `guard_out` | `Guard` | Evaluated **after** the action completes. Checks the output. If it fails, the step is marked failed. |
| `verdict` | `Verdict` | Decides whether the step's result is accepted. See [§5](#5-verdict--approval-logic). |
| `tools` | `ToolSet` | Scopes which tools are available inside this step. See [§6](#6-toolset--scoping-tool-access). |
| `injection_protection` | `InjectionProtection` | Whether inputs are scanned for prompt injection attacks. See [§7](#7-injectionprotection--input-safety). |
| `output_schema` | `Option<Value>` | A JSON Schema (`serde_json::Value`). If `Some(schema)`, the output must deserialize as valid JSON matching the schema. |
| `dependencies` | `Vec<String>` | Names of steps that must complete before this one. Used by the DAG runner for parallel execution ordering. |
| `parallel` | `bool` | If `true`, the runner may execute this step concurrently with other `parallel: true` steps that have all their dependencies satisfied. |

---

## 3. StepAction — What a Step Does

`StepAction` is an enum. Each variant describes a different kind of work.

### `LlmCall` — call an LLM

```rust
StepAction::LlmCall {
    system: "You are a helpful assistant.".into(),
    user: "Summarise this text.".into(),
    model: None,   // None = use runner's default model
}
```

With a per-step model override:

```rust
StepAction::LlmCall {
    system: "You are a critic.".into(),
    user: "Critique this.".into(),
    model: Some(verdict::action::ProviderSpec {
        model: "claude-opus-4-7".into(),
        provider: "openai-compatible".into(),
    }),
}
```

| Field | Description |
|-------|-------------|
| `system` | System prompt — defines the LLM's role |
| `user` | User prompt — the actual task |
| `model` | `None` → use client default. `Some(ProviderSpec)` → override for this step only. `ProviderSpec.model` is the model ID string; `ProviderSpec.provider` is the provider name (e.g. `"openai-compatible"`). |

> **Note:** The `LlmCall` action does **not** automatically inject previous step outputs.
> Read them from `ctx.step_results` manually using `Custom` if you need chaining.
> See [§10](#10-passing-data-between-steps).

---

### `Custom` — arbitrary Rust code

```rust
StepAction::Custom(Arc::new(move |ctx| {
    // ctx: &StepContext — access previous results, shared state, etc.
    let previous = ctx.step_results
        .get("some_step")
        .map(|r| r.output.raw.as_str())
        .unwrap_or("");
    Ok(StepOutput::new(format!("processed: {}", previous)))
}))
```

The closure signature is **synchronous**: `Fn(&StepContext) -> Result<StepOutput, StepError>`.

To call async code (e.g. an LLM client) from inside a `Custom` closure, use
`tokio::task::block_in_place`:

```rust
StepAction::Custom(Arc::new(move |_ctx| {
    let client = client.clone();
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async move {
            let resp = client.complete(req).await
                .map_err(|e| StepError::ActionFailed { reason: e.to_string() })?;
            Ok(StepOutput::new(resp.content))
        })
    })
}))
```

---

### `ToolCall` — call a registered tool

```rust
StepAction::ToolCall {
    tool: "word_count".into(),
    args: json!({ "text": "hello world" }),
}
```

---

### `DelegateAgent` — hand off to another agent

```rust
StepAction::DelegateAgent {
    agent: "reviewer".into(),
    input: json!({ "code": "fn main() {}" }),
    expected_output_schema: None,
    delegation_policy: DelegationPolicy {
        max_depth: 1,
        allowed_agents: vec!["reviewer".into()],
        require_output_schema: false,
        inherit_tool_scope: false,
        inherit_budget: true,
        require_user_approval: false,
    },
}
```

---

### `SubPipeline` — nest a full pipeline as one step

```rust
StepAction::SubPipeline(Box::new(Pipeline {
    name: "inner".into(),
    steps: vec![step_a, step_b],
    on_failure: FailureMode::Abort,
    max_retries: 0,
}))
```

---

### `LoopUntil` — repeat until a condition passes

```rust
StepAction::LoopUntil {
    body: Box::new(StepAction::SubPipeline(Box::new(inner_pipeline))),
    condition: Guard::Custom(Arc::new(|ctx| {
        // Return Ok(()) to STOP the loop, Err(_) to continue
        if ctx.step_results.get("check").map(|r| r.output.raw.as_str()) == Some("SUCCESS") {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "loop_exit".into(),
                reason: "not done yet".into(),
            })
        }
    })),
    max_iterations: 5,
    on_iteration_failure: IterationFailureMode::Retry,
}
```

> **Exit semantics:** The loop **stops** when `condition` returns `Ok(())`.
> It **continues** when `condition` returns `Err(…)`.
> Use `max_iterations` as a safety limit.

| Field | Description |
|-------|-------------|
| `body` | The `StepAction` to execute each iteration (usually a `SubPipeline`) |
| `condition` | A `Guard` evaluated after each body execution. `Ok` = stop. `Err` = continue. |
| `max_iterations` | Hard limit — loop stops regardless of condition after this many runs |
| `on_iteration_failure` | `IterationFailureMode::Retry` continues on failure; `Abort` stops |

---

### `Branch` — conditional execution

```rust
StepAction::Branch {
    condition: "success".into(),      // String matched against previous step output
    if_true: Box::new(StepAction::LlmCall { … }),
    if_false: Some(Box::new(StepAction::LlmCall { … })),
}
```

---

### `UserInput` — pause and ask the user

```rust
StepAction::UserInput {
    prompt: "What should the function be named?".into(),
    schema: None,
}
```

---

### `UseSkill` — apply a registered skill

```rust
StepAction::UseSkill {
    skill: "code_review".into(),
    input: json!({ "code": "fn main() {}" }),
    mode: SkillMode::Pipeline,
}
```

---

### `RemoteAgent` — call an agent on a remote server

```rust
StepAction::RemoteAgent {
    endpoint: "http://other-host:8080".into(),
    agent_name: "summariser".into(),
    payload: json!({ "text": "…" }),
}
```

---

## 4. Guard — Preconditions and Postconditions

A `Guard` is a code-enforced check. It either passes (`Ok(())`) or fails (`Err(GuardError)`).

Use guards in `guard_in` (before action) and `guard_out` (after action) on any `AgentStep`.

### Always-pass

```rust
Guard::None
```

### Output checks

```rust
Guard::NonEmptyOutput           // output must not be empty string
Guard::ValidJson                // output must be valid JSON
Guard::ValidToml                // output must be valid TOML
Guard::ValidYaml                // output must be valid YAML
Guard::ValidRustSyntax          // output must be valid Rust source
Guard::OutputIsUnifiedDiff      // output must be a unified diff
Guard::MaxTokens(4096)          // output must fit within N tokens
Guard::MaxOutputBytes(65536)    // output must fit within N bytes
Guard::MaxLines(200)            // output must be under N lines
Guard::MatchesSchema(schema)    // output must match a JSON Schema
```

### Step state checks

```rust
Guard::StepPassed("step_name")  // a previous step must have passed
Guard::StepFailed("step_name")  // a previous step must have failed
Guard::UserApproved("step_name")// a previous step must have been approved
```

### File checks

```rust
Guard::FileExists("/path/to/file")
Guard::FileNotExists("/path/to/file")
Guard::FileContains { path: "…".into(), pattern: "TODO".into() }
Guard::FileNotContains { path: "…".into(), pattern: "password".into() }
```

### Security checks

```rust
Guard::NoSecretsInOutput        // no secrets (API keys, tokens) in output
Guard::NoSecretsInDiff          // no secrets in a diff output
Guard::NoSecretExfiltration     // no attempt to exfiltrate secrets
Guard::NoPermissionEscalation   // no privilege escalation
Guard::NoNewNetworkAccess       // no new network calls added
Guard::NoDangerousShellCommands // no rm -rf, curl | bash, etc.
Guard::PathWithinWorkspace      // file operations stay in allowed paths
Guard::NoSafetyBypass           // output doesn't disable safety checks
Guard::NoTestDisabling          // output doesn't skip/delete tests
Guard::NoGuardRemoval           // output doesn't remove guards
```

### Code quality checks

```rust
Guard::Compiles                 // code compiles (cargo check)
Guard::TestsPass                // test suite passes
Guard::TestsPassWith(TestRunner::Pytest)  // specific runner
Guard::LintPass                 // linter passes
Guard::FormatPass               // formatter passes
Guard::NoNewDependencies        // no new deps added
Guard::DependenciesAllowlist(vec!["tokio".into()])
Guard::NoSuspiciousDependencies
Guard::CargoAuditPass
Guard::CargoDenyPass
```

### Diff / change bounds

```rust
Guard::DiffTouchesAllowedPaths(vec!["src/".into()])
Guard::DiffDoesNotTouchForbiddenPaths(vec!["Cargo.lock".into()])
Guard::MaxDiffLines(500)
Guard::MaxChangedFiles(10)
```

### Budget / rate guards

```rust
Guard::MaxCostUsd(0.50)         // total cost must be under $0.50
Guard::MaxLlmCalls(10)          // at most 10 LLM calls
Guard::MaxToolCalls(20)         // at most 20 tool calls
Guard::MaxDelegationDepth(3)    // at most 3 levels of agent delegation
Guard::TimeoutSeconds(30)       // step must complete within 30s
```

### Composition

```rust
Guard::AllOf(vec![
    Guard::NonEmptyOutput,
    Guard::ValidJson,
    Guard::NoSecretsInOutput,
])

Guard::AnyOf(vec![
    Guard::Compiles,
    Guard::ValidRustSyntax,
])

Guard::Not(Box::new(Guard::FileExists("/should/not/exist")))
```

### Custom guard

```rust
Guard::Custom(Arc::new(|ctx| {
    if ctx.output.as_ref().map(|o| o.raw.contains("error")).unwrap_or(false) {
        Err(GuardError::Failed {
            guard: "no_error_in_output".into(),
            reason: "output contains 'error'".into(),
        })
    } else {
        Ok(())
    }
}))
```

The closure receives a `&StepContext` and returns `Result<(), GuardError>`.

---

## 5. Verdict — Approval Logic

The `verdict` field decides whether the step's result is **accepted** after `guard_out` passes.

```rust
// Always accept (no verdict needed)
verdict: Verdict::None,

// Accept if the guard passes
verdict: Verdict::Automated(Guard::NonEmptyOutput),

// Require a human to approve in the terminal
verdict: Verdict::UserApproval {
    prompt: "Accept this output?",
    show_diff: true,   // show a diff before asking
},

// All sub-verdicts must pass
verdict: Verdict::AllOf(vec![
    Verdict::Automated(Guard::Compiles),
    Verdict::Automated(Guard::TestsPass),
]),

// Any sub-verdict passing is enough
verdict: Verdict::AnyOf(vec![
    Verdict::Automated(Guard::Compiles),
    Verdict::UserApproval { prompt: "Force accept?", show_diff: false },
]),
```

---

## 6. ToolSet — Scoping Tool Access

`ToolSet` controls which tools the step's action may call.

```rust
ToolSet::None                          // no tools allowed
ToolSet::ReadOnly                      // fs.read, fs.list, search.files, search.grep
ToolSet::ReadWrite                     // all read + write operations
ToolSet::Full                          // all tools
ToolSet::Allow(vec!["word_count".into(), "http_get".into()])  // explicit allowlist
ToolSet::Deny(vec!["shell.exec".into()])   // everything except these
ToolSet::FromSkill("code_review".into())   // inherit from a registered skill
ToolSet::Intersection(                     // only tools allowed by BOTH
    Box::new(ToolSet::ReadOnly),
    Box::new(ToolSet::Allow(vec!["fs.read".into()])),
)
ToolSet::Union(                            // tools allowed by EITHER
    Box::new(ToolSet::Allow(vec!["tool_a".into()])),
    Box::new(ToolSet::Allow(vec!["tool_b".into()])),
)
```

---

## 7. InjectionProtection — Input Safety

Controls whether step inputs are scanned for prompt injection attacks.

```rust
InjectionProtection::None     // no scanning — fast, use for trusted inputs
InjectionProtection::Strict   // scan for injection patterns — use for user-facing inputs
```

Use `Strict` whenever the step's input comes from an untrusted source (user input, web
content, file contents from outside the repo).

---

## 8. Pipeline — Assembling Steps

```rust
let pipeline = Pipeline {
    name: "my_pipeline".into(),
    steps: vec![step_1, step_2, step_3],
    on_failure: FailureMode::Abort,   // what to do when a step fails
    max_retries: 2,                   // retry the whole pipeline up to N times
};
```

### FailureMode

```rust
FailureMode::Abort    // stop immediately on first failure
FailureMode::Retry    // retry the failed step (up to max_retries)
FailureMode::Skip     // skip the failed step and continue
```

---

## 9. PipelineRunner — Running It All

```rust
// Minimal runner (no LLM)
let mut runner = PipelineRunner::new();

// With an LLM client
let provider = OpenAiCompatibleProvider::new(
    "http://localhost:4141/v1".into(),
    "sk-your-api-key".into(),
    "claude-haiku-4-5-20251001".into(),
);
let client = Arc::new(LlmClient::new(Arc::new(provider)));
let mut runner = PipelineRunner::new().with_llm_client(client);

// Run
let result = runner.run(&pipeline, &agent, json!({"input": "hello"})).await?;

// Inspect results
println!("Passed: {:?}", result.steps_passed);
println!("Failed: {:?}", result.steps_failed);
for (name, step_result) in &result.step_results {
    println!("{}: {}", name, step_result.output.raw);
}
```

`LlmClient::from_env()` reads `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`
from the environment and returns `Err(LlmError::NotConfigured)` if the key is absent.

---

## 10. Passing Data Between Steps

The runner does **not** automatically inject previous step outputs into prompts.
Read them from `ctx.step_results` inside a `Custom` action:

```rust
StepAction::Custom(Arc::new(move |ctx| {
    let draft = ctx.step_results
        .get("draft_step")          // name of the earlier step
        .map(|r| r.output.raw.clone())
        .unwrap_or_default();

    Ok(StepOutput::new(format!("Improving: {}", draft)))
}))
```

`StepContext` fields available in a `Custom` closure:

| Field | Type | Description |
|-------|------|-------------|
| `step_results` | `HashMap<String, StepResult>` | All completed steps so far |
| `request` | `Value` | The original input passed to `runner.run()` |
| `input` | `Value` | This step's resolved input |
| `output` | `Option<StepOutput>` | This step's output (set after action runs) |
| `agent_name` | `String` | Name of the running agent |
| `pipeline_name` | `String` | Name of the running pipeline |
| `step_name` | `String` | Name of this step |
| `trace` | `PipelineTrace` | Execution trace entries |
| `budget` | `BudgetState` | Token/cost budget state |
| `filesystem_policy` | `FilesystemPolicy` | Allowed filesystem operations |
| `network_policy` | `NetworkPolicy` | Allowed network operations |

`StepResult` fields:

| Field | Type | Description |
|-------|------|-------------|
| `output.raw` | `String` | The raw text output of the step |
| `output.parsed` | `Option<Value>` | Parsed JSON output, if available |
| `verdict_passed` | `bool` | Whether the verdict passed |
| `error` | `Option<String>` | Error message if the step failed |

---

## 11. LLM Calls with Per-Step Models

Each `LlmCall` step can specify a different model. This enables **difficulty-based
routing** — use a fast cheap model for simple tasks, a powerful model for hard ones.

```rust
use verdict::action::ProviderSpec;

fn model_spec(model: &str) -> Option<ProviderSpec> {
    Some(ProviderSpec {
        model: model.to_string(),
        provider: "openai-compatible".to_string(),
    })
}

// Step 1 — easy task → fast model
AgentStep {
    action: StepAction::LlmCall {
        system: "Write a first draft.".into(),
        user: "Draft a haiku.".into(),
        model: model_spec("claude-haiku-4-5-20251001"),
    },
    ..
}

// Step 2 — hard task → powerful model
AgentStep {
    action: StepAction::LlmCall {
        system: "Analyse in depth.".into(),
        user: "Critique the haiku.".into(),
        model: model_spec("claude-opus-4-7"),
    },
    ..
}
```

The `PipelineRunner` uses `ProviderSpec.model` to override the client's default model
for that step. The `base_url` and API key always come from the configured `LlmClient`.

---

## 12. LoopUntil — Iteration Until a Condition

Use `LoopUntil` when you need to retry work until it succeeds (e.g. TDD loops,
self-correction, retry-until-valid).

```rust
let loop_pipeline = Pipeline {
    name: "fix_loop".into(),
    steps: vec![fix_step, test_step, check_step],  // check_step outputs "SUCCESS" on pass
    on_failure: FailureMode::Abort,
    max_retries: 0,
};

AgentStep {
    name: "tdd_loop".into(),
    guard_in: Guard::None,
    action: StepAction::LoopUntil {
        body: Box::new(StepAction::SubPipeline(Box::new(loop_pipeline))),
        condition: Guard::Custom(Arc::new(|ctx| {
            // Check the last step of the sub-pipeline for "SUCCESS"
            match ctx.step_results.get("check_step") {
                Some(r) if r.output.raw.contains("SUCCESS") => Ok(()),
                _ => Err(GuardError::Failed {
                    guard: "not_done".into(),
                    reason: "check_step has not yet output SUCCESS".into(),
                }),
            }
        })),
        max_iterations: 6,
        on_iteration_failure: IterationFailureMode::Retry,
    },
    guard_out: Guard::NonEmptyOutput,
    verdict: Verdict::None,
    tools: ToolSet::None,
    injection_protection: InjectionProtection::None,
    output_schema: None,
    dependencies: vec!["initial_step".into()],
    parallel: false,
}
```

**Key rules:**
- The condition guard checks `ctx.step_results` from **inside the SubPipeline**
- Put the "success check" step **last** in the loop pipeline so its output is visible
- Return `Ok(())` from the condition to **exit** the loop
- Return `Err(…)` to **continue** looping
- `max_iterations` is a hard safety limit — always set it

---

## 13. Full Worked Example

A two-step pipeline that calls an LLM, validates the output is non-empty JSON, and
requires a human to approve before finishing.

```rust
use std::sync::Arc;
use serde_json::json;
use verdict::prelude::*;

#[tokio::main]
async fn main() {
    // Build LLM client
    let provider = OpenAiCompatibleProvider::new(
        "http://localhost:4141/v1".into(),
        "sk-my-key".into(),
        "claude-sonnet-4-6".into(),
    );
    let client = Arc::new(LlmClient::new(Arc::new(provider)));

    // Step 1: generate a JSON summary
    let step1 = AgentStep {
        name: "summarise".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are a summariser. Output ONLY valid JSON.".into(),
            user: "Summarise the Rust programming language in JSON with keys: name, year, paradigm.".into(),
            model: None,
        },
        guard_out: Guard::AllOf(vec![
            Guard::NonEmptyOutput,
            Guard::ValidJson,
        ]),
        verdict: Verdict::Automated(Guard::ValidJson),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
    };

    // Step 2: human reviews and approves
    let step2 = AgentStep {
        name: "review".into(),
        guard_in: Guard::StepPassed("summarise".into()),
        action: StepAction::Custom(Arc::new(|ctx| {
            let summary = ctx.step_results
                .get("summarise")
                .map(|r| r.output.raw.as_str())
                .unwrap_or("(none)");
            println!("\nGenerated summary:\n{}", summary);
            Ok(StepOutput::new("reviewed".into()))
        })),
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::UserApproval {
            prompt: "Accept this summary?",
            show_diff: false,
        },
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec!["summarise".into()],
        parallel: false,
    };

    // Assemble pipeline
    let pipeline = Pipeline {
        name: "summary_pipeline".into(),
        steps: vec![step1, step2],
        on_failure: FailureMode::Abort,
        max_retries: 1,
    };

    // Build a minimal agent (required by the runner API)
    let agent = Agent {
        name: "summariser_agent".into(),
        description: "Summarises things.".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
    };

    // Run
    let mut runner = PipelineRunner::new().with_llm_client(client);
    match runner.run(&pipeline, &agent, json!({})).await {
        Ok(result) => {
            println!("\nPassed: {:?}", result.steps_passed);
        }
        Err(e) => {
            eprintln!("Pipeline failed: {}", e);
        }
    }
}
```

---

## Quick Reference

### Step template

```rust
AgentStep {
    name: "step_name".into(),
    guard_in: Guard::None,
    action: StepAction::LlmCall {
        system: "…".into(),
        user: "…".into(),
        model: None,
    },
    guard_out: Guard::NonEmptyOutput,
    verdict: Verdict::Automated(Guard::NonEmptyOutput),
    tools: ToolSet::None,
    injection_protection: InjectionProtection::None,
    output_schema: None,
    dependencies: vec![],
    parallel: false,
}
```

### Common guard combos

```rust
// Output is valid, non-empty JSON
Guard::AllOf(vec![Guard::NonEmptyOutput, Guard::ValidJson])

// Code is safe to merge
Guard::AllOf(vec![
    Guard::Compiles,
    Guard::TestsPass,
    Guard::NoSecretsInDiff,
    Guard::DiffTouchesAllowedPaths(vec!["src/".into()]),
])

// Previous step must have worked
Guard::StepPassed("compile".into())
```

### Reading previous step output (Custom action pattern)

```rust
StepAction::Custom(Arc::new(move |ctx| {
    let prev = ctx.step_results
        .get("previous_step_name")
        .map(|r| r.output.raw.clone())
        .unwrap_or_default();
    Ok(StepOutput::new(process(prev)))
}))
```

### Async inside Custom (block_in_place pattern)

```rust
StepAction::Custom(Arc::new(move |_ctx| {
    let client = client.clone();
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async move {
            let resp = client.complete(req).await
                .map_err(|e| StepError::ActionFailed { reason: e.to_string() })?;
            Ok(StepOutput::new(resp.content))
        })
    })
}))
```
