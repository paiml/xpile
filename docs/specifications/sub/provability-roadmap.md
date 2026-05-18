# Provability Roadmap — ruchy 5.0 alignment

**Section 27 of [xpile-spec.md](../xpile-spec.md).**

**Source document**:
[`/home/noah/src/ruchy/docs/specifications/ruchy-5.0-sovereign-platform.md`](../../../../ruchy/docs/specifications/ruchy-5.0-sovereign-platform.md)
(1051 lines, dated 2026-04-03). The "provability mandate" lives in
ruchy's §14; that section is the model this document tracks against.

**Bounded claim** (ruchy §14.1 echoes this for xpile too): "becoming
one of the most provable polyglot transpile workbenches" is a *niche*
claim, not a universal one. We are not competing with Lean-mathlib on
mathematical depth, CompCert on verified compilation, or seL4 on
end-to-end refinement to assembly. We're competing within
{contract-driven transpilers}, a smaller field. Within that niche the
ruchy-5.0 §14 commitments are the bar; this document is xpile's plan
to clear it.

## 1. Planned for adoption

Each row below is a one-PR-sized chunk that adopts a ruchy-§14
mechanism into xpile. Ordering reflects shipping risk (top is
smallest); items are independent, can be parallelised.

### 1.1 Pre-committed falsifier thresholds (`XPILE-FALSIFY-XXX`)

**Status (XPILE-FALSIFY-001)**: ✅ shipped — PMAT-015. `xpile audit <path>` walks every source file the dispatch table recognises, transpiles it, parses the emitted output for `// xpile-contract: <ID>` citations adjacent to function declarations, reports F1 (% coverage). Text + `--json` output modes.

**Status (XPILE-FALSIFY-002)**: ✅ shipped — PMAT-023. F1 is now computed against the *applicable-contracts denominator* — only functions where `Function::applicable_contracts()` is non-empty count in the denominator. Pre-002 the denominator was every emitted function, which double-penalised comparison-only and logical-only functions (`cmp.py::le`, `pick.py::pick`) that correctly emit *no* citation by design. With the refinement, F1 on the current corpus jumps from 83.3% [WARN] to **100.0% [OK]**. Also added `--target lean`: Lean's `@[xpile_contract "..."]` attribute is now parsed; F1 reports 100.0% [OK] there too (19 / 19 cited; the `asserted.py` fixture errors on Lean per the live `XPILE-PENDING-UNTIL: v0.3.0` marker, which is reported under `errors:` rather than counted as a citation miss). New `over_citations` JSON field is a sanity check for the symmetric failure mode (codegen wrongly cites a comparison-only function); currently 0. PTX/WGSL/SPIR-V citations are XPILE-FALSIFY-003+.

**Ruchy reference**: §14.5 (F1–F12 metrics with pre-committed
falsifier thresholds).

**xpile analog**: Each Layer-1 contract publishes a quantitative
threshold below which the contract's enforcement claim is falsified.

Initial proposed metrics:

| # | Metric | Initial target | Falsified if... |
|---|---|---|---|
| F1 | % of transpiled functions carrying at least one `// xpile-contract: <ID>` citation | ≥ 95% on a fixed corpus | < 50% — the citation pipeline is performative |
| F2 | Density of `expect("... not yet implemented ...")` panics per KLoC of emitted Rust | ≤ 1 / KLoC | > 5 / KLoC — the slow-path scaffold is the wrong default |
| F3 | Oracle-vs-transpile divergence rate on the runtime-verified fixture set | 0 | ≥ 1 — semantic equivalence claim has a hole |
| F4 | Count of Layer-1 contracts with zero `falsification_tests` | 0 | ≥ 1 — a contract that can't be falsified is a tautology |
| F5 | Backends with `Unsupported` errors emitted on the runtime-verified fixture set | 0 (Rust), 0 (Lean) | ≥ 1 on a previously-passing fixture — regression |

Implementation lands as new fields in `contracts/*.yaml` and a
companion `xpile audit` subcommand that scans + reports.

### 1.2 Time-bounded escape hatches (`XPILE-EXEMPT-XXX`)

**Status (XPILE-EXEMPT-001)**: ✅ shipped — PMAT-014. Implementation: every "not yet implemented" `Unsupported(...)` error string in `xpile-rust-codegen`, `xpile-ruchy-codegen`, `xpile-lean-codegen` carries `[XPILE-PENDING-UNTIL: v<semver>, ticket: <ID>]`. Enforced by `crates/xpile/tests/exempt_deadlines.rs` which scans every `.rs` file in `crates/*/src/` and fails CI when current workspace version ≥ any deadline. Three live markers as of v0.1.0 (Ruchy BigInt mode → v0.2.0; Rust BigInt bitwise → v0.2.0; Lean assert → v0.3.0).

**Status (XPILE-EXEMPT-002)**: ✅ shipped — PMAT-022. Widens the deadline scanner from "Rust source under `crates/*/src/`" to also cover proof-lane and Symbolic-stratum artefacts: `contracts/lean/*.lean` and `contracts/kani/*.rs`. The `XPILE-PENDING-UNTIL: v0.3.0` marker in `PyIntArith.lean`'s `sorry` proof (PMAT-017) was effectively decorative before this PR — the scanner walked past `contracts/` entirely. Now it's gated alongside the codegen markers. New `scanner_picks_up_proof_lane_markers` test asserts the widening worked (catches future regressions that narrow the scan back). What's still pending under XPILE-EXEMPT: extending the marker pattern to `expect("...")` panic strings inside emitted Rust (the i64-overflow panics document a runtime tradeoff, not an unimplemented feature — adding deadlines there means committing to a specific implicit-promotion timeline — XPILE-EXEMPT-003+).

**Ruchy reference**: §14.7 (`#[contract_exempt(reason, until,
ticket)]` with `build.rs` enforcement).

**xpile analog**: every `expect("... slow path not yet
implemented")` and every `LeanCodegenError::Unsupported(...)` /
`RuchyCodegenError::Unsupported(...)` string carries a deadline.

Proposed shape:

```rust
// In emitted Rust (extension of PMAT-002's panic strings):
.expect(
    "xpile: i64 addition overflow; \
     bigint promotion (contract C-PY-INT-ARITH slow path) \
     not yet implemented (until: v0.3.0, ticket: PMAT-014)"
)
```

```rust
// In codegen Unsupported errors:
RuchyCodegenError::Unsupported {
    msg: "BigInt mode not yet implemented in Ruchy backend",
    until_version: "0.2.0",
    ticket: "PMAT-015",
}
```

`build.rs` (or `pv lint`) compares `CARGO_PKG_VERSION` against each
`until` and hard-fails when current ≥ until. This closes the
"unimplemented forever" hole — every promise of a slow-path has a
date attached.

Backwards-compat: existing PMAT-002 panic messages get re-emitted
with `until` strings; old fixture-test assertions that match on
fragments of the message still pass because the contract-ID prefix
is unchanged.

### 1.3 N-of-M stratified oracle quorum (`XPILE-QUORUM-XXX`)

**Status (XPILE-QUORUM-001)**: ✅ shipped — PMAT-019. First **Symbolic stratum** harness lands in `contracts/kani/py_int_arith.rs`. The `addition_no_overflow` `#[kani::proof]` function discharges `C-PY-INT-ARITH`'s addition equation via Kani BMC (bit-blasted i64 arithmetic, ~27ms on Kani 0.67). YAML wiring: `py-int-arith-v1.yaml`'s `addition_no_overflow` equation now carries `kani_harness:` + `kani_file:` fields alongside the Lean theorem from PMAT-017. Together with PMAT-018's diff_exec runtime check (Semantic stratum) and PMAT-017's Lean theorem statement (Semantic stratum), the equation now has **≥1 Symbolic + ≥1 Semantic vote** per the ruchy 5.0 §14.4 quorum rule. Citation gate (`crates/xpile/tests/kani_harnesses.rs`) validates every `kani_harness:` field references a real file with a real `#[kani::proof] fn <name>(...)` — symmetric with the Lean gate in `refinement_proofs.rs`.

**Status (XPILE-QUORUM-002)**: ✅ shipped — PMAT-020. The citation gate from QUORUM-001 confirmed harnesses *exist*; QUORUM-002 confirms they *discharge*. New `crates/xpile/tests/kani_verify.rs` walks every `contracts/kani/*.rs` file, materialises a temp Cargo crate per harness (Cargo.toml + lib.rs), runs `cargo kani`, asserts exit-0 AND stdout contains `VERIFICATION:- SUCCESSFUL` (the grep guards against Kani's historical "exit 0 on swallowed solver error" failure mode). Skip-gracefully if `cargo-kani` is missing from PATH; local users with Kani installed get the gate automatically. Converts the Symbolic stratum from claim to fact.

**Status (XPILE-QUORUM-003)**: ✅ shipped — PMAT-021. New dedicated `kani` job in `.github/workflows/ci.yml` installs `kani-verifier` + runs `cargo kani-setup` + runs the `kani_verify` test against every harness on every PR. Kept as a *separate* job from `workspace-test` (Kani install is ~5 min on cold cache; bundling would slow fast-feedback gates) and not yet a *required* status check (flip after Kani has bedded in for a release cycle).

**Status (XPILE-QUORUM-005)**: ✅ shipped — PMAT-032. Closes the Extrinsic stratum side of the §14.4 quorum. New `xpile attestations` subcommand walks `contracts/*.yaml` for the contract-ID universe (via lightweight `metadata.id:` line scan) and counts mentions in `docs/roadmaps/roadmap.yaml`; each occurrence is one human attestation, attributed to the enclosing work item's `id:`. As of substrate completion (PMAT-058..077): **12 contracts scanned, all 12 reach ≥1 Extrinsic attestation** via the roadmap entries shipped alongside each refinement PR. Integration test (`crates/xpile/tests/attestations.rs`) asserts the Extrinsic count for `C-PY-INT-ARITH` is ≥1 in the live roadmap and that the text-mode output carries its landmarks.

**Status (PMAT-033)**: ✅ shipped — unified `xpile quorum` reporter consolidates all four strata into a single per-contract table. It's a reporter (not a gate); the constituent gates remain authoritative. Sources: Semantic = `lean_theorem:` refs in the contract's own YAML; Symbolic = `kani_harness:` refs in the contract's own YAML; Runtime = fixture files mentioning the contract ID; Extrinsic = roadmap mentions (reuses PMAT-032 scanner). Quorum thresholds per §14.4: ≥1 vote in ≥3 strata = QUORUM, 1-2 strata = PARTIAL, 0 = UNVERIFIED. **As of PMAT-058..077 substrate completion and PMAT-127..138 quality sweep: 12 QUORUM / 0 PARTIAL / 0 UNVERIFIED. 100% of the 12-contract substrate at QUORUM, all 12 at 4-stratum minimum (Sem + Sym + Run + Ext).** Two contracts reach rich four-stratum coverage with multi-vote Runtime witnesses (C-PY-INT-ARITH: 9/1/4/7; C-BASHRS-POSIX-IDEMPOTENCE: 1/1/1/12); the remaining 10 reach 4-stratum QUORUM with a single demo Runtime fixture each (deeper Runtime witnesses awaiting Bronze→Gold tier refinement per each contract's `XPILE-REFINE-*-001+` follow-on). Tests: `tests/quorum.rs` asserts C-PY-INT-ARITH has full quorum and the reporter walks every contracts/*.yaml without missing any. Still pending under XPILE-QUORUM: §14.5 F3 pairwise-correlation guard (XPILE-QUORUM-004 — needs ≥3 oracles per contract; C-PY-INT-ARITH qualifies today with 4 distinct stratum sources, and the remaining 11 substrate contracts now provide 4 distinct stratum sources each — single-vote demo fixtures count toward source diversity).

**Ruchy reference**: §14.4 (Symbolic + Semantic + Extrinsic
oracle strata, ≥1 vote from each).

**xpile analog**: today the Oracle is a single stratum (behavioral
capture of CPython output). Adding parallel oracles per Layer-1
contract:

| Stratum | Oracle | Verdict |
|---|---|---|
| Symbolic | Kani BMC on the emitted Rust | no counter-example ≤ bound |
| Semantic | probar (1000 fuzzed inputs) on the Python source vs the transpiled Rust | no falsifier found |
| Semantic | Lean theorem on the Layer-1 contract's equation | `impl ≡ spec` proved |
| Extrinsic | Human review (`pmat work complete` with `--note`) | LGTM with reason string |

Anti-correlation guard (Ruchy §14.5 F3): pairwise verdict
correlation tracked over a 100-fixture sample; any pair ≥ 0.95
collapses to one vote (no triple-counting the same evidence).

Discharge rule: Layer-1 contracts require ≥1 Symbolic + ≥1 Semantic
vote; safety-critical contracts (e.g. `C-FFI-CPYTHON-REFCOUNT`)
add ≥1 Extrinsic.

### 1.4 Differential execution check (`XPILE-DIFF-XXX`)

**Status (XPILE-DIFF-001)**: ✅ shipped — PMAT-018. `crates/xpile/tests/diff_exec.rs` runs 10 deterministic LCG-seeded i64 inputs per fixture across 7 single-arg fast-path fixtures (factorial, fib, abs_val, sign, sum_to, for_sum, countdown:factorial_iter); for each input it runs both CPython directly on the .py source and the rustc-compiled transpiled-Rust binary, asserts the stdout strings agree. 70 differential checks per CI run, all green at v0.1.0.

**Status (XPILE-DIFF-002)**: ✅ shipped — PMAT-024. Generalises the runner from 1-arg-only to support 2-arg fixtures via per-arg input ranges (struct `FixtureCfg { args: &[(i64, i64)] }`); driver synthesis builds the right `entry(argv[0], argv[1], ...)` call expression at the arity. Three new 2-arg fixtures: `gcd(a, b)` over [0, 1M]², `multi_branch::range_size(a, b)` over [-1B, 1B]², `bits::bits(a, b)` over [-2^61, 2^61)². Total: **100 differential checks across 10 fixtures per CI run**, all green.

**Status (XPILE-DIFF-003)**: ✅ shipped — PMAT-031. Adds optional per-fixture `overflow_args` ranges plus a three-way outcome classifier (`DocumentedGap` / `Promoted` / `OffContractCrash`). Inputs from the overflow domain are run on both CPython (which always promotes to BigInt) and the rustc-compiled Rust binary; the runner interprets a Rust panic with `C-PY-INT-ARITH` in the message as a *documented gap*, not a failure. Hard-fail conditions: (a) Rust exits zero with a value that diverges from Python's BigInt result — silent miscompile; (b) Rust panics without naming `C-PY-INT-ARITH` — citation regression. Two fixtures wired up (`factorial.py` and `countdown.py::factorial_iter` on n∈[21, 30]). Still pending under XPILE-DIFF: more tricky-semantics fixtures (`square_plus`, `safe_div`); arity > 2.

**Status (PMAT-036)**: ✅ shipped — converts the 20 documented promotion gaps from DIFF-003 into 20 promoted-and-agreed successes. Headline metric: `XPILE-DIFF-003: 20 overflow-phase checks across 2 fixture(s) — 0 documented promotion gaps, 20 promoted-and-agreed.` Mechanism: (i) `factorial.py` + `countdown.py` annotated `-> BigInt` so PMAT-013's implicit promotion lifts the whole body to BigInt mode; (ii) `depyler-frontend` extended to propagate BigInt-mode through the for-range loop-target binding (was hard-coded I64); (iii) `depyler-frontend` skips `from __future__ import annotations` preamble (needed for CPython to `exec` the fixture); (iv) `diff_exec.rs` dual-mode build pipeline — uses cargo with a real `xpile-bigint` path-dep when the transpile output uses BigInt, falls back to standalone rustc otherwise. Architectural payoff: stress-tests the §27 type lattice through frontend lowering + codegen + differential-exec all participating in the BigInt-mode path.

**Ruchy reference**: §14.10.4 (interpreter vs transpiled binary
on N probar-generated inputs per function).

**xpile analog**: today we have 11 hand-authored runtime-verified
fixtures (`factorial`, `fib`, `gcd`, …). Each was hand-picked.
Generalise:

```
D1. For every transpiled fn `f(args: T...)`:
D2.   probar generates 100 inputs satisfying `requires` (from contract
      YAML); or, lacking contracts, generates inputs sampling each
      arg's type domain
D3.   CPython-3.x evaluates f on each input → reference[i]
D4.   rustc -O builds the emitted Rust + runs f on each input → observed[i]
D5.   reference[i] == observed[i] for all i, OR the function ships
      with a `#[xpile_diff_exempt(reason, until, ticket)]` (same
      hatch surface as §1.2)
```

Closes the "fixture overfitting" caveat from `audit-design.md` §4
quantitatively, not just by adding more fixtures.

### 1.5 Refinement proofs via Lean (`XPILE-REFINE-XXX`)

**Status (XPILE-REFINE-001)**: ✅ shipped — PMAT-017. First Layer-1 contract (`C-PY-INT-ARITH`) gets a `lean_theorem:` + `lean_file:` field on its `addition_no_overflow` equation. The file `contracts/lean/PyIntArith.lean` carries the theorem statement `fast_path_eq_slow_path` (proves `i64_wrap_add a b = bigint_add a b` when `fits_i64 (a + b)`). The *statement* IS the load-bearing artefact: it's what `@[xpile_contract "C-PY-INT-ARITH"]` citations point at. Enforcement: `crates/xpile/tests/refinement_proofs.rs` walks every contract YAML, validates every `lean_theorem:` field points at a real file with a real theorem-of-that-name (closes the citation-bridge fragility caveat for this contract). Three stub theorems for mul/floor_div/mod listed in the file too, also under XPILE-PENDING-UNTIL gates.

**Status (XPILE-REFINE-002)**: ✅ shipped — PMAT-028. The `sorry` in `fast_path_eq_slow_path` is discharged via Lean core's `Int.bmod` (balanced mod, returns values in `[-N/2, N/2)`). Refactored `i64_wrap_add a b := Int.bmod (a + b) (2 ^ 64)`; the proof is then `unfold + rw [Int.bmod_def] + split <;> omega` — no mathlib dependency. Closes the second of the two `XPILE-PENDING-UNTIL: v0.3.0` markers (Lean assert was the first, PMAT-027). `refinement_proofs.rs` was updated to assert the positive landmark `Int.bmod_def` is present and the negative landmark `sorry` is absent from proof code (docstrings excluded); a regression that reintroduces `sorry` will fire that test.

**Status (XPILE-REFINE-003)**: ✅ shipped — PMAT-029. All three stubs (`mul`, `floor_div`, `mod`) are now real theorems with discharged proofs. Closes the *last* `XPILE-PENDING-UNTIL` marker anywhere in the workspace. Approach: factored `bmod_fits_i64 : Int.bmod n (2^64) = n when fits_i64 n` out of the additive proof; `mul_fast_path_eq_slow_path` reuses it directly via `i64_wrap_mul a b := Int.bmod (a * b) (2^64)`. `floor_div` and `mod` both model fast and slow path as the same `Int.fdiv` / `Int.fmod` operation under their `fits_i64`-of-result preconditions, so the theorems reduce to `rfl` — the load-bearing observation is that the *identity* relation is recorded next to the equation rather than implicit in prose. Three more equations in `py-int-arith-v1.yaml` now carry `lean_theorem` + `lean_file` refs (`multiplication_quadratic_promotion`, `division_floor_semantics`, new `modulo_floor_semantics`); refinement_proofs.rs validates all four theorems by name on every test run.

**Status (XPILE-REFINE-004)**: ✅ shipped — PMAT-030. Completes the C-PY-INT-ARITH refinement corpus with `shl` / `shr` / `pow` theorems. `shl` and `pow` reuse the shared `bmod_fits_i64` lemma; `shr` is `rfl` (both paths are `Int.fdiv a (2^b)`). Why model shifts as `a * 2^b` rather than `a <<< b`: core Lean 4.15 doesn't auto-synthesise the `HShiftLeft Int Nat` instance, and `a * 2^b` is semantically identical for the non-negative shift amounts that Rust's `checked_shl(b: u32)` accepts — avoids a mathlib import. Three more equations now in `py-int-arith-v1.yaml`: `shift_left_signed_semantics`, `shift_right_signed_semantics`, `power_signed_semantics`. **Bitwise** (`&` / `|` / `^`) remains uncovered (XPILE-REFINE-005) because core Lean lacks `Int.land/lor/xor`.

**Status (XPILE-REFINE-006)**: ✅ shipped — PMAT-034. Slow-path soundness theorem `add_slow_path_eq_python` for `addition_overflow_promotion`. The proof is `rfl` — both Python's `int.__add__` and our model of `xpile_bigint::BigInt::add` (defined as `Int.add`) are unbounded mathematical addition, so the equation is a definitional equality. Documentary value: any future change to `bigint_add`'s Lean definition would have to retain `rfl`-equality with `+` or invalidate the theorem (and fail `refinement_proofs.rs`'s citation gate). Quorum impact: C-PY-INT-ARITH Semantic count rose 7 → 8. **Bitwise (XPILE-REFINE-005) is now the only refinement gap left** on the contract — design decision required on mathlib dep vs. hand-rolled `Int.land/lor/xor`.

**Ruchy reference**: §14.10.5 (Platinum functions have `lean_theorem`
fields proving `impl ≡ spec` within bound).

**xpile analog**: contracts like `C-PY-INT-ARITH` already have a
"fast path" and "slow path" equation. Today we enforce them
operationally (panic on overflow). Refine to a Lean theorem:

```lean
-- contracts/lean/PyIntArith.lean (generated by pv)
theorem fast_path_eq_slow_path
    (a b : Int)
    (h : (a + b) ≥ Int.neg (2^63) ∧ (a + b) < 2^63) :
    i64_wrap_add a b = bigint_add a b := by
  -- proof discharged by `pv` codegen + Lean's `decide` tactic
  ...
```

`pv lint` Gate 5 checks that every Layer-1 contract with both fast
and slow path has a non-`sorry` theorem file. We already emit
`@[xpile_contract "C-PY-INT-ARITH"]` (PMAT-011); the proof file is
the missing half.

### 1.6 Quarterly SOTA-gap dossier (`XPILE-SOTA-XXX`)

**Status (XPILE-SOTA-001)**: ✅ shipped — PMAT-016. `audit-design.md` §0 now publishes the quarterly cadence + the next-dossier deadline (2026-08-15). Enforcement: `crates/xpile/tests/sota_dossier_deadline.rs` parses the deadline string, compares against wall-clock time, fails CI when current ≥ deadline with an explicit "publish dossier + bump date" remediation message. Pure-Rust date arithmetic (no chrono dep). Three sub-tests: live gate, cadence-table integrity, date-arithmetic self-tests against known Unix epoch points.

**Ruchy reference**: §14.F-Audit-8 + F6 (recurring "what beats us
where" publication).

**xpile analog**: `audit-design.md` is currently a single snapshot
(2026-05-15). Convert it to a quarterly cadence:

- 2026-Q2 audit (initial, already exists at `audit-design.md`)
- 2026-Q3 dossier deadline: 2026-08-15
- 2026-Q4 dossier deadline: 2026-11-15
- 2027-Q1 dossier deadline: 2027-02-15

Each dossier enumerates: (a) transpilers that beat xpile on at
least one axis since the previous dossier, (b) which of xpile's
hypotheses (audit §5) is newly stressed by external work, (c) any
falsifier from §1.1 that has entered the falsified range.

Missing dossier = falsifier F6 fires automatically.

## 2. In-spirit, scope-deferred

These are mechanisms whose value is real but whose meta-HIR cost is
too large for v0.1.0. They are recorded so the boundary is explicit.

### 2.1 `Secret<T>` / `Public<T>` information-flow types

**Ruchy reference**: §14.10.1 (from HACL* / F* IntTypes).

**Why deferred for xpile**: our current users (Python → Rust for
numerical workloads) don't bring secret data through the
transpiler. The day a cryptographic-Python user shows up, the
absence of info-flow becomes a real gap; until then, adding
`Type::Secret(Box<Type>)` to meta-HIR would create downstream
work in every backend for zero observable benefit.

**Re-evaluation trigger**: first time a fixture or contract asks
"is this value safe to log / branch on".

### 2.2 Capability types for effects

**Ruchy reference**: §14.10.2 (from Austral).

**Why deferred for xpile**: capabilities solve "ambient authority"
in *runtimes*. xpile is a compile-time tool — the emitted Rust
runs in some host's runtime, and that host (not xpile) is the
right place to enforce capability constraints. We do, however,
have one capability-shaped contract already: `C-FFI-CPYTHON-REFCOUNT`
(refcount balance is *exactly* a linear-capability obligation).
Future Layer-2 FFI contracts SHOULD carry capability annotations,
even if the meta-HIR type lattice doesn't enforce them.

**Re-evaluation trigger**: a second Layer-2 FFI contract that needs
to express "this function consumes a resource that must be released".

### 2.3 Totality markers (`@total` / `decreases`)

**Ruchy reference**: §14.10.3 (from Idris / ATS).

**Why deferred for xpile**: meta-HIR has no termination story.
PMAT-010 (Lean while via `partial def`) sidesteps this by always
emitting `partial def`. A future PMAT could:

- Add `Function::is_total: Option<bool>` to meta-HIR
- Lean backend emits `def` (not `partial def`) when `is_total = true`
- Frontend infers totality from `decreases`-style annotations OR
  from structural recursion shape

**Re-evaluation trigger**: a Lean-using consumer complaining that
they can't compose `partial def` outputs inside total proofs.

## 3. Explicitly NOT adopted

This subsection exists so future readers don't propose these items
again. Each was considered and rejected for a load-bearing reason,
not by oversight.

### 3.1 The 9 pillars themselves

Ruchy's §2 lists nine pillars (Correctness, Compute, Infrastructure,
Scripting, Learning, Visualization, Simulation, Testing, Embedding).
Each is a *component of ruchy-the-language*, not a transpiler
feature. xpile:

- Already federates with bashrs (Pillar 4) per §19.
- Already consumes `aprender-contracts` (Pillar 5's substrate).
- Other seven pillars are out of scope.

### 3.2 Graduate workflow (interpret → embed → compile)

xpile has no interpreter and does not aspire to be a runtime.
ruchy's "three execution modes from one source" is a property of
*ruchy-the-language*; xpile's "many target languages from one
source" is a property of *xpile-the-transpiler*. They are
orthogonal claims.

### 3.3 Language-level new keywords

ruchy §4 introduces 7 new reserved words. xpile transpiles
*existing* languages; it does not invent syntax. Layer-1 contracts
are how xpile expresses semantic constraints, not new keywords.

## 4. Cross-references

- `xpile-spec.md` §27 — index pointing at this document.
- `xpile-spec.md` §26 — audit-acknowledged caveats this roadmap closes.
- `audit-design.md` §4 — negative-feedback bullets that informed
  this roadmap (fixture overfitting → §1.4, citation-bridge
  fragility → §1.5, single-snapshot audit → §1.6).
- ruchy §14 — the model we're tracking against.
- ruchy §14.F-Audit-8 — niche-bounded claim framing.
