# Skills System

**Section 10 of [xpile-spec.md](../xpile-spec.md).**

## Purpose

A skill is a short, prescriptive markdown file the agent loads when it hits a recurring failure idiom. Borrowed from alchemize's `skills/` pattern.

```
crates/xpile-agent/skills/
├── lifetimes.md
├── stdlib_import_resolution.md
├── generators.md
├── context_managers.md
├── decorators.md
├── c_pointer_aliasing.md
├── cuda_kernel_launch.md
└── ruchy_pipeline_op.md
```

## Skill frontmatter

```markdown
---
name: stdlib-import-resolution
lang: python
boundary: none
status: draft
fires_per_quarter_threshold: 50
distinct_files_threshold: 10
---

# When you see X

Brief description of the source idiom.

## Generate

The canonical Rust pattern.

## Pitfalls

Common mistakes the agent should avoid.
```

## Loading

The agent calls `apply_skill(name)` which pulls the markdown body into the LLM context with a `<skill name="...">` wrapper. Multiple skills can be applied in one session.

## Skills are a holding pen, not a permanent backstop

**Critical design rule.** A skill that fires often enough across the corpus signals a recurring failure pattern in the static transpiler. The fix is to lift the skill's logic into `xpile-rust-codegen` as a deterministic rule and **delete** the skill markdown.

⚠️ This is **not** encoded anywhere in this repository. The line previously read
"encoded in `contracts/skill-graduation-v1.yaml` (ported from depyler to xpile in
Phase 1)"; that file does not exist here, and Phase 1 shipped the four trait
contracts instead (`sub/phased-rollout.md`). The rule lives in depyler's
`skill-graduation-v1.yaml` and is a porting obligation, not a local invariant
(PMAT-1502).

## Graduation pipeline

```
Quarter ends → pv kaizen rollup --skills
            → report each skill's (firings, distinct_files)
            → skills meeting threshold (≥50 fires, ≥10 distinct files) are candidates
Engineer picks candidate → writes deterministic rule in xpile-rust-codegen
                       → adds corpus regression test tagged with skill ID
                       → PR includes: new rule + skill markdown deletion + test
                       → cache entries referencing the deleted skill auto-invalidate (skills_hash changes)
```

## Success signal

**Repair-invocation rate trends *down* per corpus over time.** Quarter-over-quarter rate should be flat or decreasing (≤10% growth tolerance for noise). Sustained growth means skills are accumulating instead of graduating — process failure, not code failure.

Two consecutive quarters of >10% growth in any tier triggers a graduation audit.

## Telemetry

Every skill invocation is logged:

```json
{
  "session_id": "...",
  "cache_key": "<hex>",
  "skill_name": "stdlib_import_resolution",
  "applied_at_iteration": 2,
  "session_exit": "Match"
}
```

`pv kaizen rollup --skills` aggregates these by quarter and produces the graduation candidate list.

## Skill quality requirements

A skill PR must:

1. Have frontmatter with all required fields
2. Include at least one *positive* and one *negative* example
3. Cite the diagnostic class (e.g., `E0433`, `E0502`) it targets
4. Pass `xpile-agent` ingestion: `cargo test -p xpile-agent --test skill_loader`

Reviewers reject skills that are too vague to be applied mechanically.
