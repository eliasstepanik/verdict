# AGENTS.md — Verdict Project Agent Rules

## The Single Source of Truth

**`architecture.md` is the authoritative plan for this project.**

Every struct, module, enum, trait, function, and design decision implemented in this codebase
must be described in `architecture.md`. If it is not in `architecture.md`, it must not be built.

This is not a suggestion. It is a hard rule.

---

## Rules for All Agents

### 1. Read `architecture.md` First

Before writing, editing, or planning any code, read `architecture.md` in full.
Every implementation decision must trace back to something described there.

### 2. Stay Within the Described Architecture

You may only implement what is explicitly described in `architecture.md`.

- ✅ Implementing `AgentRegistry` as described in the plan → **allowed**
- ✅ Implementing `Guard::Compiles` as listed in the expanded Guard enum → **allowed**
- ❌ Adding a new struct not mentioned in `architecture.md` → **not allowed without permission**
- ❌ Changing a module's responsibility without updating `architecture.md` → **not allowed**
- ❌ Introducing a new concept, crate, or abstraction that is not in the plan → **not allowed without permission**

### 3. Never Silently Deviate

If you discover that the architecture is incomplete, contradictory, or needs to change
to make your implementation work:

**Stop. Do not proceed. Ask the user explicitly.**

Example message:
> "To implement X, I need to introduce Y which is not currently described in `architecture.md`.
> This would be a plan change. Do I have your permission to update the architecture document
> and then proceed?"

You must receive explicit user confirmation before:
- Adding any new module, struct, enum, or trait not in the plan
- Changing the signature of any type described in the plan
- Removing or renaming anything described in the plan
- Changing module layout from what is specified in the plan
- Adding new dependencies to `Cargo.toml` not implied by the plan

### 4. Update `architecture.md` When You Build

Every time you implement something from the plan, you **must** keep `architecture.md`
in sync with the actual implementation. This means:

- If you implement a struct that was described as pseudocode → update the doc to reflect the final Rust signature
- If you added a variant to an enum that was in the plan → confirm it matches exactly or update it
- If a field name changed during implementation for good reason (and user approved) → update `architecture.md`
- If you implement a module → confirm the module path matches the layout in `architecture.md`

`architecture.md` must never fall behind the implementation. It is a living document.

### 5. Do Not Touch Forbidden Paths Without Permission

The following areas are considered architecturally sensitive. Any change here requires
explicit user approval as a plan change:

- The overall module layout (`src/` structure as described in `architecture.md`)
- Core runtime contracts: `Guard`, `Verdict`, `Pipeline`, `StepAction`, `Agent`, `AgentPolicy`
- The `PipelineRunner` execution model (the 10-step execution flow)
- Security guards and their semantics
- The self-update flow and its guard chain
- `ToolSet` scoping semantics (intersection rule)

### 6. Minimal, Targeted Changes

Only change what is needed to implement the described feature.

- Do not refactor unrelated code
- Do not rename things for aesthetics
- Do not restructure modules unless the plan explicitly calls for it
- Do not add dependencies unless required by the plan

---

## Module Layout Contract

The canonical module layout is defined in `architecture.md` under **"Updated Module Layout"**.

```
verdict/
├── src/
│   ├── lib.rs
│   ├── prelude.rs
│   ├── agent.rs
│   ├── registry.rs
│   ├── pipeline.rs
│   ├── runner.rs
│   ├── context.rs
│   ├── guard.rs
│   ├── verdict.rs
│   ├── action.rs
│   ├── toolset.rs
│   ├── tools/
│   ├── mcp/
│   ├── skills/
│   ├── injection.rs
│   ├── audit.rs
│   ├── budget.rs
│   ├── eval.rs
│   ├── self_update.rs
│   ├── llm/
│   └── agents/
```

Any deviation from this layout is a **plan change** and requires user approval.

---

## When to Ask for Permission

Ask the user for explicit permission before doing any of the following:

| Action | Required? |
|--------|-----------|
| Adding a struct not in `architecture.md` | ✅ Yes |
| Adding a new enum variant not in the plan | ✅ Yes |
| Removing or renaming anything from the plan | ✅ Yes |
| Adding a new Cargo dependency | ✅ Yes |
| Changing any module's location | ✅ Yes |
| Changing a public API signature | ✅ Yes |
| Adding a new module file not in the layout | ✅ Yes |
| Implementing exactly what the plan describes | ❌ No — just do it |
| Fixing a typo or formatting in source files | ❌ No |
| Writing tests for plan-described functionality | ❌ No |
| Adding inline doc comments | ❌ No |

---

## How to Handle a Plan Change Request

If you determine a plan change is needed, follow this exact protocol:

1. **Stop all implementation work.**
2. **State clearly:** what you want to change, why it is necessary, and what the impact is.
3. **Wait** for explicit user approval (`yes`, `approved`, `go ahead`, or equivalent).
4. **If approved:** Update `architecture.md` first, then implement.
5. **If not approved:** Find an alternative approach within the existing plan, or stop and report that the task cannot be completed as specified.

Do not interpret silence or ambiguity as approval.

---

## Keeping `architecture.md` Current

`architecture.md` is a living document. It must reflect the current state of the codebase at all times.

### Required Updates After Each Implementation

After completing any implementation task:

1. Open `architecture.md`
2. Find the section describing what you just built
3. Confirm the code matches the description exactly
4. If there are minor discrepancies (a renamed field, a method that was split, etc.) and the user approved it:
   - Update `architecture.md` to match the code
5. If the description is still accurate as pseudocode/design: leave it, but note it is implemented

### Staleness is a Bug

If `architecture.md` describes something that does not match the code, that is a bug —
just like a failing test. Treat it as a defect to be fixed.

---

## Summary Checklist

Before submitting any code change, verify:

- [ ] Everything I implemented is described in `architecture.md`
- [ ] I did not add anything not in the plan without explicit user approval
- [ ] `architecture.md` still accurately reflects the codebase after my changes
- [ ] I did not change any module path, struct name, or public API without approval
- [ ] I did not add any new Cargo dependency without approval
- [ ] If I discovered the plan was incomplete, I stopped and asked the user before proceeding

---

## The Contract

> **The architecture is the plan. The plan is the law.**
> If you want to change the law, ask the legislature (the user).
> If you want to implement the law, just do it — faithfully and completely.
