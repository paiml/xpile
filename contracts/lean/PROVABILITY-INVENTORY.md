# Proof-lane inventory — `contracts/lean/` (PMAT-903, Sprint Day 4)

This file is the **honest, machine-verified** enumeration of what the Lean
proof lane does and does not currently prove. It replaces the `grep sorry`
heuristic that the roadmap had been quoting as ground truth.

Reproduce everything below with:

```sh
cd contracts/lean
lake build                          # builds the PILOT (green ⇔ all 23 elaborate)
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

## PILOT — machine-checked (23 modules, in `lakefile.lean` roots)

These elaborate clean under bare Lean 4 core **with warnings-as-errors** — no
`sorry`, no `axiom`, no Mathlib. `lake build` is green iff all of them still do.

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
| `XlatePyListToVec` | `C-XLATE-PY-LIST-TO-VEC` (PMAT-936: MIXED head, 8 errors, four sound classes — (a) PMAT-914/915/916/928 name-shadowing `NonEmptyHomogeneousList.val` body used `n.val` (self-recursion) → positional `.1`, clearing the `:593` termination + `:632` `.property` + `:1257` `Subtype.ext` cascade; (b) `:796` `simp [List.length_append]` w/o `unfold` → reuse Platinum `lower_length_homomorphism_platinum`; (c) `:980` core `List.length_reverse l.elems` now needs the explicit arg; (d) `:1359` non-existent `Array.toList_length` → core `Array.length_toList`) |
| `FfiCpythonExt` | `C-FFI-CPYTHON-EXT` (PMAT-937: Layer-4 hybrid CPython-extension head, 20 errors, FOUR sound classes, no new termination territory — (a) PMAT-914/915/916/928/936 name-shadowing `BoundedRefcountDelta.val` body used `b.val` (self-recursion) → positional `.1` + an explicit `DecidableEq BoundedRefcountDelta` instance, clearing the `:979` termination + `:987`/`:993` `deriving DecidableEq` + `:1037` `.property` + `:1750` `Subtype.ext` + `:1786`-`:1788` canonical cascade; (b) `:1235`/`:1236` Mathlib-only `use` tactic → core `refine ⟨_, ?_⟩`; (c) `:1466` Mathlib `\|·\|`/`lt_trichotomy`/`abs_of_pos`/`Int.sign_mul_abs` → CORE `Int.natAbs`/`Int.lt_trichotomy`/`Int.natAbs_of_nonneg`/`Int.sign_mul_natAbs` (PMAT-928 lesson); (d) `:1808`+ `lift_ffi_call_bronze_to_silver` annotated the wrong structure `FfiCallSilver` (no `symbol` field) → retargeted lift+projection to `FfiCallStructuredSilver`) |
| `CompileRustToPtxMma` | `C-COMPILE-RUST-TO-PTX-MMA` (PMAT-938: the deepest module — 20 stacked Diamond categories, depth-3..20 — and the inventory's "38 errors" was almost ONE cascading root fault plus bare-core lemma-name gaps; FIVE sound classes, no new termination territory — (a) PMAT-914/915/916/928/936/937 name-shadowing `BoundedSmem.val` body used `b.val` (self-recursion) → positional `.1` + an explicit `DecidableEq BoundedSmem` instance, which poisoned EVERY downstream `.val`/`.property`/`Subtype.ext`/derived `DecidableEq` across all 20 Diamonds (the bulk of the 38); (b) two `omega` heads on NAMESPACED `Nat.min`/`Nat.max` (opaque to omega v4.15.0) — +/max·min distributivity & max/min monotonicity → core `Nat.add_{max,min}_add_{left,right}` + `Nat.{max_le,le_min,le_max_*,min_le_*,le_trans}`; (c) Mathlib-only absorption `Nat.max_min_self`/`Nat.min_max_self` → `Nat.le_antisymm` over core lattice primitives; (d) Mathlib-name gaps — `Nat.eq_or_ne` → `omega`, `pow_zero`/`pow_succ`/`pow_add`/`one_pow` → `Nat.`-namespaced, `Nat.one_le_pow` → `Nat.pow_le_pow_left` + `Nat.one_pow`; (e) a LATENT STATEMENT bug — the `mod is *` homomorphism clause wrote `a%2 * b%2 % 2` (parses `((a%2)*b)%2`), reparenthesized to the genuine ring-hom `((a%2)*(b%2))%2` that `Nat.mul_mod` proves) |
| `PyIntArith` | `C-PY-INT-ARITH` (PMAT-948 **CAPSTONE** — the LAST non-elaborating module; 45 errors, the highest count, but NO genuine missing termination measure. The `:892 fail to show termination` first-error was — for the NINTH+ time — the NAME-SHADOW class: `def PyIntFast.val (p) := p.val` self-resolved by dot-notation to itself, cascading into the `:923`/`:945` `rfl`s and the `:931` `.property` mismatch → positional `.1` clears all four. The remaining ~41 errors were ALL cheap classes already exhausted across PMAT-904..938: (a) Mathlib lemma-name gaps restated over core — `pow_zero`/`pow_one`/`pow_add` → `Int.pow_zero`/`Int.pow_succ` + a hand-rolled `int_pow_add` (Nat-induction over `Int.pow_succ`); `Nat.land_comm` → core `Nat.and_comm` under `Int.ofNat`; `Int.lt_asymm`/`Int.one_ne_zero`/`Int.max_min_distrib_left`/`min_max_distrib_left`/`dvd_trans`/`Nat.cast_*`/`Int.toNat_natCast`/`Int.toNat_eq_zero` → `omega`/`decide`/`Int.dvd_trans`/`Int.toNat_ofNat`/`Int.ofNat_*`; (b) Mathlib-only TACTICS `ring`/`nlinarith` → explicit core `Int.sub_mul`/`Int.mul_sub`/`Int.mul_assoc` + `Int.mul_nonneg`/`Int.mul_pos`/`Int.neg_mul_neg`; (c) the `\|·\|` abs NOTATION (undefined with no `import Mathlib`) restated over core `Int.natAbs` — the PMAT-928/937 lesson — using `Int.natAbs_add_le`/`Int.natAbs_mul`; (d) a LATENT PARENTHESIZATION bug (PMAT-938 class) — the emod `*`-homomorphism clause wrote `a%2 * b%2 % 2`, which parses left-assoc as `((a%2)*b)%2`, reparenthesized to the genuine ring-hom `((a%2)*(b%2))%2` that `Int.mul_emod` proves. The ONE genuinely new piece is the **Bézout identity**: `Int.gcdA`/`gcdB`/`gcd_eq_gcd_ab` are Mathlib-only (verified ABSENT from the entire Lean 4.15.0 toolchain `src`), so conjunct (d) of `gcd_monoid_bezout_diamond` was restated as the EXISTENTIAL Bézout `∃ x y, gcd a b = a*x + b*y` — the genuine mathematical content, NOT a weakening (the Mathlib `gcdA`/`gcdB` are merely ONE constructive witness for this existential) — and PROVED core-only via `Nat.gcd.induction` (the real extended-Euclidean structural recursion, decreasing on `Nat.gcd`'s own well-founded measure), with universality (c) proved core via `Int.natAbs`/`Nat.dvd_gcd`. No `sorry`, no `axiom`, no `import Mathlib`) |

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

## KNOWN-INCOMPLETE — 0 modules (the ENTIRE substrate is machine-checked)

**As of PMAT-948 (CAPSTONE), there are ZERO non-elaborating modules.** Every
`contracts/lean/*.lean` file is now a `lakefile.lean` root and elaborates clean
under bare core Lean 4.15.0 with `warningAsError := true` — no `sorry`, no
`axiom`, no `import Mathlib`. `lake build` is green ⇔ all 23 modules still do.
The historical per-module discharge counts (`error:` lines from `lean <file>`
on v4.15.0, all now in the pilot above) were: `XpileBackendTrait` 3 +
`XpileContractFrontendTrait` 2 (PMAT-904), `XpileFrontendTrait` 5 (PMAT-913),
`XlateRustFnToLeanThm` 4 (PMAT-914), `XlateLeanToRust` 7 (PMAT-915),
`Notation` 7 (PMAT-916), `Bashrs` 7 (PMAT-928), `XlatePyListToVec` 8 (PMAT-936),
`FfiCpythonExt` 20 (PMAT-937), `CompileRustToPtxMma` 38 (PMAT-938), and the
final **`PyIntArith` 45 (PMAT-948 — the capstone)**.

**The capstone — `PyIntArith` (45 errors, the highest count):** confirmed for
the NINTH+ time that a `fail to show termination` first-error is NOT a genuine
missing measure. The `:892` head was the SAME name-shadow class (`def
PyIntFast.val (p) := p.val` self-recursion → positional `.1`, clearing the
`:923`/`:931`/`:945` cascade). The remaining ~41 were ALL previously-exhausted
cheap classes — Mathlib lemma-name gaps → core (`pow_*` → `Int.pow_*` + a
hand-rolled `int_pow_add`; `Nat.land_comm` → core `Nat.and_comm`; a dozen
`Int.*`/`Nat.cast_*` aliases → `omega`/`decide`/core), Mathlib-only tactics
(`ring`/`nlinarith` → explicit core distributivity + `Int.mul_nonneg`/
`Int.mul_pos`), the `|·|` abs notation restated over `Int.natAbs` (PMAT-928/937),
and a latent parenthesization bug `a%2*b%2%2` → `(a%2)*(b%2)%2` (PMAT-938 class).
**The ONE genuinely new piece** was the Bézout identity in
`gcd_monoid_bezout_diamond`: `Int.gcdA`/`gcdB`/`gcd_eq_gcd_ab` are Mathlib-only
(verified absent from the entire toolchain `src`). Rather than weaken or fake it,
conjunct (d) was restated as the EXISTENTIAL Bézout `∃ x y, gcd a b = a*x + b*y`
(the genuine mathematical content; the Mathlib `gcdA`/`gcdB` are merely one
witness) and PROVED core-only via `Nat.gcd.induction` — the real
extended-Euclidean structural recursion, decreasing on `Nat.gcd`'s own
well-founded measure — with universality (c) via `Int.natAbs`/`Nat.dvd_gcd`.
So `PyIntArith` carried NO genuine missing `termination_by`/`decreasing_by`
obligation in its DEFINITIONS; the only real structural-recursion content was the
new Bézout *proof*, discharged honestly.

**This was the real provability debt** the machine-checked lane exposed — honest
debt, not hidden `sorry`s — and it is now fully discharged. PMAT-904 cleared the
two cheapest
(unknown-tactic / synthesis / `rw`-through-`def`), PMAT-913 cleared
`XpileFrontendTrait` (precedence-paren + `tauto`→`decide`), PMAT-914 cleared
`XlateRustFnToLeanThm`, PMAT-915 cleared `XlateLeanToRust`, PMAT-916 cleared
`Notation`, PMAT-928 cleared `Bashrs`, PMAT-936 cleared `XlatePyListToVec`
(MIXED head: name-shadow `.val`→`.1` clearing the `:593`/`:632`/`:1257`
cascade + three core-lemma fixes — `simp`-through-`def` → Platinum reuse,
`List.length_reverse l.elems` explicit arg, `Array.toList_length` →
`Array.length_toList`), and PMAT-937 cleared `FfiCpythonExt` (MIXED head, 20
errors, four sound classes: the name-shadow `BoundedRefcountDelta.val`→`.1` +
explicit `DecidableEq BoundedRefcountDelta` clearing the
`:979`/`:987`/`:993`/`:1037`/`:1750`/`:1786`-`:1788` cascade; Mathlib-only `use`
→ core `refine ⟨_, ?_⟩`; the Mathlib `|·|`/`abs`/`Int.sign_mul_abs` sign-decomp
→ core `Int.natAbs`/`Int.sign_mul_natAbs`; and a wrong-structure annotation
`FfiCallSilver`→`FfiCallStructuredSilver` on the Bronze→Silver lift/projection),
and PMAT-938 cleared `CompileRustToPtxMma` (the deepest module — 20 stacked
Diamonds, depth-3..20 — whose 38 errors were almost ALL the ONE name-shadow
`BoundedSmem.val`→`.1` + explicit `DecidableEq BoundedSmem` cascade poisoning
every `.val`/`.property`/`Subtype.ext`/derived-`DecidableEq` across all 20
Diamonds, plus `omega`-opaque namespaced `Nat.min`/`Nat.max` heads → core lattice
lemmas, Mathlib-name gaps (`Nat.max_min_self`/`pow_*`/`Nat.one_le_pow`/`Nat.eq_or_ne`),
and a latent `*`-homomorphism parenthesization bug `a%2*b%2%2`→`(a%2)*(b%2)%2`)
— their `fail to show termination`
first-errors turned out NOT to be genuine missing termination arguments but the
**name-shadowing class**: a `def Subtype.val (x) := x.val` body resolves `x.val`
by dot-notation to *itself* (a non-terminating recursive call, `x` unchanged),
and that broken `.val` poisons every downstream `.val`, cascading into the
`.property` / `Subtype.ext` / derived-`DecidableEq` failures. The fix is the
positional `.1` Subtype projection in the body (PMAT-915/937 also needed an
explicit `DecidableEq` instance for structs `deriving DecidableEq`
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

**ZERO modules remain.** **Lesson (now confirmed TEN times across
PMAT-914/915/916/928/936/937/938/948): a `fail to show termination` first-error
is not proof the fault is termination — check for a self-naming `.val`/projection
helper first.** The capstone `PyIntArith` was the final confirmation: its `:892`
termination head was the same `def PyIntFast.val (p) := p.val` name-shadow as the
prior nine, NOT a genuine missing measure. Across the whole 23-module substrate,
exactly ZERO definitions needed a hand-written `termination_by`/`decreasing_by` —
the only real structural-recursion *content* anywhere was the new core Bézout
existence proof (`int_gcd_bezout_exists`), which rides on core's own
`Nat.gcd.induction` well-founded recursor rather than a bespoke measure. The
"PyIntArith may finally carry genuine termination debt" hypothesis is now
falsified: it did not.

## Relationship to `audit-design.md`

Day 10 (PMAT-909) truths-up `audit-design.md` to state: the Lean lane is now
`lake`-machine-checked over a (now 23-module) pilot, the `grep sorry`/`grep
axiom` debt figures were a measurement artifact, and the real remaining debt is
**ZERO non-elaborating modules** — PMAT-948 discharged the last one
(`PyIntArith`, the capstone: name-shadow `.val`→`.1`, Mathlib lemma-name/tactic
gaps → core, `|·|`→`Int.natAbs`, a latent parenthesization bug, and a core
EXISTENTIAL Bézout proved via `Nat.gcd.induction` replacing the Mathlib-only
`gcdA`/`gcdB` witnesses). The whole `contracts/lean/` substrate is machine-checked.
No over-claim: "provable" applies to the pilot contracts, verified by `lake
build`, not by string scan.
