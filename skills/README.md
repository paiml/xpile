# xpile skills

Markdown files that encode "when you see X, do Y" for the xpile repair agent. Borrowed from [alchemize's skills/](https://github.com/pymc-labs/alchemize/tree/main/alchemize/skills) pattern.

## How skills work

When the agent loop runs and encounters a particular diagnostic class or source idiom, it calls `apply_skill(name)` to pull the relevant markdown into context. Skills are:

- **Short** — a page or less, prescriptive
- **Tagged by language and boundary type** — `lang: python`, `boundary: ffi-c`, etc.
- **Composable** — the agent can apply several skills in one session

## A skill is a holding pen, not a permanent backstop

A skill that fires often enough across the corpus signals a recurring failure pattern that should be lifted into the static transpiler (`xpile-rust-codegen` or a frontend's lowering). When promoted:

- A deterministic rule lands in the appropriate crate
- The corresponding `skills/<name>.md` is **deleted in the same PR**
- A corpus regression test guards the promoted rule

Skill *graduation rate* is the success signal for the agent loop: skills should trend toward zero over time as the static side gets smarter.

## Planned starter set (scaffold stage — all TODO)

- `lifetimes.md` — common Rust borrow/lifetime resolution patterns
- `stdlib_import_resolution.md` — when consult_stdlib_map returns nothing for a Python or C symbol
- `generators.md` — Python generators → Rust iterators
- `context_managers.md` — Python `with` blocks → RAII / scope guards
- `c_pointer_aliasing.md` — C pointer aliasing → safe Rust references
- `cuda_kernel_launch.md` — Python `@cuda.jit` kernel launches → cudarc / wgpu shim patterns
- `ruchy_pipeline_op.md` — Ruchy `|>` pipelines → idiomatic Rust method chaining

## Skill template

```markdown
---
name: foo-pattern
lang: python
boundary: none
status: draft
---

# When you see X

Brief description of the source idiom (3-5 lines).

## Generate

The canonical Rust pattern, with an example.

## Pitfalls

Common mistakes the agent should avoid.

## Promotion criterion

Fires N+ times across M+ distinct files in a quarter → graduate to a static rule.
```
