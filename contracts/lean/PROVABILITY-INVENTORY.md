# Proof-lane inventory — `contracts/lean/` (PMAT-903, Sprint Day 4)

This file is the **honest, machine-verified** enumeration of what the Lean
proof lane does and does not currently prove. It replaces the `grep sorry`
heuristic that the roadmap had been quoting as ground truth.

Reproduce everything below with:

```sh
cd contracts/lean
lake build                          # builds the PILOT (green ⇔ all 17 elaborate)
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

## PILOT — machine-checked (19 modules, in `lakefile.lean` roots)

These elaborate clean under bare Lean 4 core **with warnings-as-errors** — no
`sorry`, no `axiom`, no Mathlib. `lake build` is green iff all nineteen still do.

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
| `XlateLeanToRust` | `C-XLATE-LEAN-TO-RUST` (PMAT-915: same name-shadowing class — `WarningLineCount.val` body used `w.val` (self-recursion) → positional `.1` projection + explicit `DecidableEq WarningLineCount` instance for the two `deriving DecidableEq` structs) |
| `Notation` | `C-NOTATION-LATEX-MATH-TO-EQUATION` (PMAT-916: two already-established classes — `NonEmptyDefinition.val` body used `n.val` (name-shadowing self-recursion, PMAT-914/915) → positional `.1`; and `cases k <;> tauto` over the decidable `LatexDisplayKind` enum (Mathlib-only, PMAT-904/913) → core `cases k <;> decide`) |
| `Bashrs` | `C-BASHRS-POSIX-IDEMPOTENCE` (PMAT-928: MIXED head — (a) PMAT-914/915/916 name-shadowing `SuccessfulOutcome.val` body used `s.val` (self-recursion) → positional `.1`; (b) NEW genuine Mathlib gap — the Int-sign Diamond's `\|·\|`/`abs_nonneg`/`simp` (no `import Mathlib`) restated over CORE `Int.natAbs` (`Nat.zero_le` non-negativity + `rw`+`rfl` zero-abs) + core `Int.lt_trichotomy`) |
| `XlatePyBoolToRustBool` | `C-XLATE-PY-BOOL-TO-RUST-BOOL` (PMAT-935: NEW R6 contract joins at depth-1 — core-only `PyBool` single-truth-flag STRUCTURE EXTENSIONALITY, same shape as PyFloatArith; closes the last uncited core scalar) |

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

## KNOWN-INCOMPLETE — 4 modules with REAL elaboration errors (excluded)

These do **not** elaborate today. The cause is genuine proof debt — NOT
sorries. The dominant failure is unproved **termination** of recursive
definitions (`fail to show termination`, needing `termination_by` /
`decreasing_by`), with cascading type-mismatch / synthesis / unknown-tactic
errors downstream. Counts are `error:` lines from `lean <file>` on v4.15.0
(the smallest non-termination cases — `XpileBackendTrait` 3 +
`XpileContractFrontendTrait` 2 in PMAT-904, `XpileFrontendTrait` 5 in PMAT-913,
`XlateRustFnToLeanThm` 4 in PMAT-914, `XlateLeanToRust` 7 in PMAT-915,
`Notation` 7 in PMAT-916, `Bashrs` 7 in PMAT-928 — were discharged and are now
in the pilot above):

| Module | `error:` count | Representative first error |
|--------|---------------:|----------------------------|
| `PyIntArith` | 45 | fail to show termination (`:892`) |
| `CompileRustToPtxMma` | 38 | fail to show termination (`:240`) + failed to synthesize |
| `FfiCpythonExt` | 20 | fail to show termination (`:979`) |
| `XlatePyListToVec` | 8 | fail to show termination (`:593`) + type mismatch |

**This is the real provability debt** the machine-checked lane exposes — and it
is honest debt, not hidden `sorry`s. PMAT-904 cleared the two cheapest
(unknown-tactic / synthesis / `rw`-through-`def`), PMAT-913 cleared
`XpileFrontendTrait` (precedence-paren + `tauto`→`decide`), PMAT-914 cleared
`XlateRustFnToLeanThm`, PMAT-915 cleared `XlateLeanToRust`, PMAT-916 cleared
`Notation`, and PMAT-928 cleared `Bashrs` — their `fail to show termination`
first-errors turned out NOT to be genuine missing termination arguments but the
**name-shadowing class**: a `def Subtype.val (x) := x.val` body resolves `x.val`
by dot-notation to *itself* (a non-terminating recursive call, `x` unchanged),
and that broken `.val` poisons every downstream `.val`, cascading into the
`.property` / `Subtype.ext` / derived-`DecidableEq` failures. The fix is the
positional `.1` Subtype projection in the body (PMAT-915 also needed an explicit
`DecidableEq WarningLineCount` instance for two structs `deriving DecidableEq`
over the now-fixed subtype field; PMAT-916's `Notation` ALSO carried a
PMAT-904/913-class Mathlib-only `cases k <;> tauto` over the decidable
`LatexDisplayKind` enum → core `cases k <;> decide`).

**`Bashrs` (PMAT-928) was the FIRST mixed head to also carry a *genuine* Mathlib
gap, not just a name-shadow:** beyond the `:213` `SuccessfulOutcome.val (s) :=
s.val` → `.1` name-shadow, its `outcome_exit_code_int_sign_diamond` used
Mathlib's `|o.exit_code|` absolute-value **notation** (`:683`
`unexpected token '|'` — the symbol is undefined with no `import Mathlib`) plus
`abs_nonneg` and `simp`. The discharge restates those clauses over **core**
`Int.natAbs : Int → Nat`: non-negativity `0 ≤ exit_code.natAbs` is now
*type-level* (`Nat.zero_le`, no lemma — `natAbs` lands in `Nat`), zero-abs
`exit_code = 0 → exit_code.natAbs = 0` is `rw`+`rfl` (`(0:Int).natAbs = 0`
definitionally), and the sign trichotomy uses core `Int.lt_trichotomy` (the bare
`lt_trichotomy` alias is the PMAT-904/913 Mathlib class). Same Int-sign Diamond
content — trichotomy + |·| non-negativity + zero-abs-of-zero + reflexivity — with
zero Mathlib dependency. The lesson: a real Mathlib `abs`/`|·|` use over `Int`
restates cleanly via `Int.natAbs`; you do NOT need to define `|·|` or import
Mathlib for non-negativity (it is the codomain) or zero-abs (it is defeq).

The remaining 4 are genuinely termination-led and are ongoing work —
`XlatePyListToVec` (8) is now the cheapest by `error:` count, then
`FfiCpythonExt` (20), `CompileRustToPtxMma` (38), `PyIntArith` (45). **Lesson
(repeated): a `fail to show termination` first error is not proof the fault is
termination — check for a self-naming `.val`/projection helper first.**

## Relationship to `audit-design.md`

Day 10 (PMAT-909) truths-up `audit-design.md` to state: the Lean lane is now
`lake`-machine-checked over a (now 18-module) pilot, the `grep sorry`/`grep
axiom` debt figures were a measurement artifact, and the real remaining debt is
4 non-elaborating modules (all genuinely termination-led — the name-shadow and
the Mathlib-`abs` heads are now discharged through PMAT-928). No over-claim:
"provable" applies to the pilot contracts, verified by `lake build`, not by
string scan.
