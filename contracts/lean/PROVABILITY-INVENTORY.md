# Proof-lane inventory — `contracts/lean/` (PMAT-903, Sprint Day 4)

This file is the **honest, machine-verified** enumeration of what the Lean
proof lane does and does not currently prove. It replaces the `grep sorry`
heuristic that the roadmap had been quoting as ground truth.

Reproduce everything below with:

```sh
cd contracts/lean
lake build                          # builds the PILOT (green ⇔ all 9 elaborate)
for f in *.lean; do lean "$f"; echo "$f rc=$?"; done   # full per-file status
```

The pinned toolchain is `lean-toolchain` → `leanprover/lean4:v4.15.0`.

## The `grep sorry` / `grep axiom` myth (corrected)

The sprint plan's ground truth quoted **"6 `sorry` across 5 files"** and
**"~34 axiom lines"**. PMAT-903 verified this is a naive-`grep` artifact:

- **Zero uses of the `sorry` tactic/term.** The six historical hits are five
  docstring occurrences of the phrase *"…genuinely provable, sorry-free…"*
  (in `XlatePyClassToStruct`, `XlatePyOptionalToOption`, `XlatePySetToHashset`,
  `XlatePyTupleToRustTuple`) plus one inductive **constructor named `sorry`** in
  `Notation.lean`'s `ProofStubReason` enum (`| sorry`) — a *model* of a stub
  reason, not the proof-hole tactic.
- **Zero `axiom` declarations.** Every `axiom` hit is prose inside a docstring
  (e.g. "captures the monoid **axiom** for the underlying composition").
  `grep -nE '^[[:space:]]*axiom [A-Za-z_][A-Za-z0-9_]*[[:space:]]*[:({]' *.lean`
  returns nothing.
- **Zero `import` statements** of any kind — no module imports Mathlib (or
  anything else). The frequent "Mathlib's `List.reverse_reverse`…" lines are
  docstrings naming lemmas, not dependencies. The advisory `lake build` job
  therefore needs **no** Mathlib cache.

`lakefile.lean` sets `warningAsError := true`, so a real `sorry` (which
elaborates to `sorryAx`) **cannot survive a green `lake build`**. That is the
check that makes "provable" un-falsifiable by `grep sorry` for the pilot
contracts — the actual machine-checked guarantee, not a string scan.

## PILOT — machine-checked (9 modules, in `lakefile.lean` roots)

These elaborate clean under bare Lean 4 core **with warnings-as-errors** — no
`sorry`, no `axiom`, no Mathlib. `lake build` is green iff all nine still do.

| Module | Contract |
|--------|----------|
| `CIntArith` | `C-C-INT-ARITH` |
| `PyFloatArith` | `C-PY-FLOAT-ARITH` (PMAT-903 fixed a nested-comment bug — see below) |
| `XlatePyDictToHashmap` | `C-XLATE-PY-DICT-TO-HASHMAP` |
| `XlatePyStrToRustString` | `C-XLATE-PY-STR-TO-RUST-STRING` |
| `XpileContractBackendTrait` | `C-XPILE-CONTRACT-BACKEND` |
| `XlatePyClassToStruct` | `C-XLATE-PY-CLASS-TO-STRUCT` (was mis-flagged "sorry") |
| `XlatePyOptionalToOption` | `C-XLATE-PY-OPTIONAL-TO-OPTION` (was mis-flagged "sorry") |
| `XlatePySetToHashset` | `C-XLATE-PY-SET-TO-HASHSET` (was mis-flagged "sorry") |
| `XlatePyTupleToRustTuple` | `C-XLATE-PY-TUPLE-TO-RUST-TUPLE` (was mis-flagged "sorry") |

**PyFloatArith bug fixed by PMAT-903:** a header docstring contained the literal
`NaN/-0.0`. Lean treats `/-` as a *nested* block-comment opener even inside a
`/- … -/` block, so it swallowed the rest of the file — meaning
`py_float_structure_extensionality_diamond` was **never actually elaborated**.
Rewording to "NaN and signed-zero" restores elaboration; the theorem is now
genuinely machine-checked.

## KNOWN-INCOMPLETE — 11 modules with REAL elaboration errors (excluded)

These do **not** elaborate today. The cause is genuine proof debt — NOT
sorries. The dominant failure is unproved **termination** of recursive
definitions (`fail to show termination`, needing `termination_by` /
`decreasing_by`), with cascading type-mismatch / synthesis / unknown-tactic
errors downstream. Counts are `error:` lines from `lean <file>` on v4.15.0:

| Module | `error:` count | Representative first error |
|--------|---------------:|----------------------------|
| `PyIntArith` | 45 | fail to show termination (`:892`) |
| `CompileRustToPtxMma` | 38 | fail to show termination (`:240`) + failed to synthesize |
| `FfiCpythonExt` | 20 | fail to show termination (`:979`) |
| `XlatePyListToVec` | 8 | fail to show termination (`:593`) + type mismatch |
| `Bashrs` | 7 | fail to show termination (`:213`) |
| `Notation` | 7 | fail to show termination (`:816`) — *not* the `\| sorry` ctor |
| `XlateLeanToRust` | 7 | fail to show termination (`:1014`) |
| `XpileFrontendTrait` | 5 | invalid constructor `⟨…⟩` (`:530`) + unknown tactic |
| `XlateRustFnToLeanThm` | 4 | fail to show termination (`:627`) + type mismatch |
| `XpileBackendTrait` | 3 | unknown tactic (`:741`) |
| `XpileContractFrontendTrait` | 2 | rewrite failed (`:268`) + failed to synthesize |

**This is the real provability debt** the machine-checked lane exposes — and it
is honest debt, not hidden `sorry`s. Discharging it (starting with the
termination obligations) is Day 5+ work; PMAT-903 is "no discharge today".

## Relationship to `audit-design.md`

Day 10 (PMAT-909) truths-up `audit-design.md` to state: the Lean lane is now
`lake`-machine-checked over a 9-module pilot, the `grep sorry`/`grep axiom`
debt figures were a measurement artifact, and the real remaining debt is 11
non-elaborating modules (termination-led). No over-claim: "provable" applies to
the pilot contracts, verified by `lake build`, not by string scan.
