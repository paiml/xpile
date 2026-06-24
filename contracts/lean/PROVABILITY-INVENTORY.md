# Proof-lane inventory — `contracts/lean/` (PMAT-903, Sprint Day 4)

This file is the **honest, machine-verified** enumeration of what the Lean
proof lane does and does not currently prove. It replaces the `grep sorry`
heuristic that the roadmap had been quoting as ground truth.

Reproduce everything below with:

```sh
cd contracts/lean
lake build                          # builds the PILOT (green ⇔ all 12 elaborate)
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

## PILOT — machine-checked (15 modules, in `lakefile.lean` roots)

These elaborate clean under bare Lean 4 core **with warnings-as-errors** — no
`sorry`, no `axiom`, no Mathlib. `lake build` is green iff all fifteen still do.

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
| `XpileBackendTrait` | `C-XPILE-BACKEND-TRAIT` (PMAT-904: `tauto`→`decide`) |
| `XpileContractFrontendTrait` | `C-XPILE-CONTRACT-FRONTEND-TRAIT` (PMAT-904: `Inhabited` + defeq `calc`) |
| `FfiShellSubprocess` | `C-FFI-SHELL-SUBPROCESS` (PMAT-907: depth-1 `ShellInvocation` STRUCTURE EXTENSIONALITY) |
| `CFloatArith` | `C-C-FLOAT-ARITH` (PMAT-912: depth-1 `CFloat32`/`CFloat64` STRUCTURE EXTENSIONALITY + ABI-width-distinctness) |
| `XpileFrontendTrait` | `C-XPILE-FRONTEND-TRAIT` (PMAT-913: precedence-paren on `parse_and_lower_function_diamond` clause (c) + `tauto`→`decide` on `source_lang_enum_completeness_diamond`) |
| `XlateRustFnToLeanThm` | `C-XLATE-RUST-FN-TO-LEAN-THM` (PMAT-914: name-shadowing fix — `NonEmptyPreconditionList.val` body used `n.val` (self-recursion) → positional `.1` Subtype projection) |

**PMAT-904 (Sprint Day 5) discharged the two cheapest non-elaborating files** —
both with *real* errors, not sorries, confirming the reframed debt model:

- **`XpileBackendTrait`** (was 3 errors) — `target_enum_completeness_diamond`
  used the **Mathlib-only `tauto`** tactic (`cases t <;> tauto`); with no
  `import Mathlib` it was an *unknown tactic*. Replaced with core **`decide`**:
  after `cases t` each goal is a decidable disjunction over the `Target` enum.
- **`XpileContractFrontendTrait`** (was 2 errors) — (a) `[0]!` on an
  `Array EquationsBlock` needed `Inhabited EquationsBlock`; added `Inhabited` to
  the `deriving` clause. (b) `frame_safety_transitive_platinum` used
  `rw [t1.property, …]`, but `before`/`after` are `def`s so `rw` couldn't see
  the `.val.fst/.snd` pattern syntactically; re-proved as a defeq `calc`
  (mirroring how `frame_safety_witness_gold` discharges `f.property`).

**PyFloatArith bug fixed by PMAT-903:** a header docstring contained the literal
`NaN/-0.0`. Lean treats `/-` as a *nested* block-comment opener even inside a
`/- … -/` block, so it swallowed the rest of the file — meaning
`py_float_structure_extensionality_diamond` was **never actually elaborated**.
Rewording to "NaN and signed-zero" restores elaboration; the theorem is now
genuinely machine-checked.

## KNOWN-INCOMPLETE — 7 modules with REAL elaboration errors (excluded)

These do **not** elaborate today. The cause is genuine proof debt — NOT
sorries. The dominant failure is unproved **termination** of recursive
definitions (`fail to show termination`, needing `termination_by` /
`decreasing_by`), with cascading type-mismatch / synthesis / unknown-tactic
errors downstream. Counts are `error:` lines from `lean <file>` on v4.15.0
(the smallest non-termination cases — `XpileBackendTrait` 3 +
`XpileContractFrontendTrait` 2 in PMAT-904, `XpileFrontendTrait` 5 in PMAT-913,
`XlateRustFnToLeanThm` 4 in PMAT-914 — were discharged and are now in the pilot
above):

| Module | `error:` count | Representative first error |
|--------|---------------:|----------------------------|
| `PyIntArith` | 45 | fail to show termination (`:892`) |
| `CompileRustToPtxMma` | 38 | fail to show termination (`:240`) + failed to synthesize |
| `FfiCpythonExt` | 20 | fail to show termination (`:979`) |
| `XlatePyListToVec` | 8 | fail to show termination (`:593`) + type mismatch |
| `Bashrs` | 7 | fail to show termination (`:213`) |
| `Notation` | 7 | fail to show termination (`:816`) — *not* the `\| sorry` ctor |
| `XlateLeanToRust` | 7 | fail to show termination (`:1014`) |

**This is the real provability debt** the machine-checked lane exposes — and it
is honest debt, not hidden `sorry`s. PMAT-904 cleared the two cheapest
(unknown-tactic / synthesis / `rw`-through-`def`), PMAT-913 cleared
`XpileFrontendTrait` (precedence-paren + `tauto`→`decide`), and PMAT-914 cleared
`XlateRustFnToLeanThm` — whose "fail to show termination (`:627`) + type
mismatch" turned out NOT to be a genuine missing termination argument but a
**name-shadowing bug**: `def NonEmptyPreconditionList.val (n) := n.val` resolved
`n.val` by dot-notation to *itself* (a non-terminating recursive call, `n`
unchanged), and that broken `.val` poisoned every downstream `n.val`, cascading
into the `n.property` and `Subtype.ext` mismatches. The one-line fix is the
positional `.1` Subtype projection in the body. The remaining 7 are genuinely
termination-led and are ongoing work — `Bashrs`/`Notation`/`XlateLeanToRust`
(7 each) are now the cheapest heads.

## Relationship to `audit-design.md`

Day 10 (PMAT-909) truths-up `audit-design.md` to state: the Lean lane is now
`lake`-machine-checked over a (now 15-module) pilot, the `grep sorry`/`grep
axiom` debt figures were a measurement artifact, and the real remaining debt is
7 non-elaborating modules (termination-led). No over-claim: "provable" applies
to the pilot contracts, verified by `lake build`, not by string scan.
