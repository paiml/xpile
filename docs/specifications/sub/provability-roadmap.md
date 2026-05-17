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

**Status (XPILE-FALSIFY-001)**: ✅ shipped — PMAT-015. `xpile audit <path>` walks every source file the dispatch table recognises, transpiles it, parses the emitted output for `// xpile-contract: <ID>` citations adjacent to function declarations, reports F1 (% coverage). Text + `--json` output modes. Current baseline against `crates/xpile/tests/fixtures/`: F1 ≈ 83% — below the 95% target but above the 50% falsifier (status `WARN`). The gap is by design (comparison-only / logical-only functions correctly don't carry the citation under the data-driven `applicable_contracts` rule); refining F1 to "% of functions that *should* have a citation that *do*" is XPILE-FALSIFY-002. Lean's `@[xpile_contract "..."]` attribute uses a different parse and is also XPILE-FALSIFY-002.

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

**Status (XPILE-EXEMPT-001)**: ✅ shipped — PMAT-014. Implementation: every "not yet implemented" `Unsupported(...)` error string in `xpile-rust-codegen`, `xpile-ruchy-codegen`, `xpile-lean-codegen` carries `[XPILE-PENDING-UNTIL: v<semver>, ticket: <ID>]`. Enforced by `crates/xpile/tests/exempt_deadlines.rs` which scans every `.rs` file in `crates/*/src/` and fails CI when current workspace version ≥ any deadline. Three live markers as of v0.1.0 (Ruchy BigInt mode → v0.2.0; Rust BigInt bitwise → v0.2.0; Lean assert → v0.3.0). Remaining work (XPILE-EXEMPT-002+): extend the marker pattern to `expect("...")` panic strings inside emitted Rust (the i64-overflow panics currently document a runtime tradeoff, not an unimplemented feature, so they don't carry deadlines yet — but the implicit-promotion case might want one once we commit to it).

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

**Status (XPILE-QUORUM-002)**: ✅ shipped — PMAT-020. The citation gate from QUORUM-001 confirmed harnesses *exist*; QUORUM-002 confirms they *discharge*. New `crates/xpile/tests/kani_verify.rs` walks every `contracts/kani/*.rs` file, materialises a temp Cargo crate per harness (Cargo.toml + lib.rs), runs `cargo kani`, asserts exit-0 AND stdout contains `VERIFICATION:- SUCCESSFUL` (the grep guards against Kani's historical "exit 0 on swallowed solver error" failure mode). Skip-gracefully if `cargo-kani` is missing from PATH; local users with Kani installed get the gate automatically. Converts the Symbolic stratum from claim to fact. Still remaining: install Kani in CI so the gate fires on every PR rather than only locally (XPILE-QUORUM-003); §14.5 F3 pairwise-correlation guard (XPILE-QUORUM-004 once we have ≥3 oracles); Extrinsic stratum verdict-recording via pmat work-item notes (XPILE-QUORUM-005).

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

**Status (XPILE-DIFF-001)**: ✅ shipped — PMAT-018. `crates/xpile/tests/diff_exec.rs` runs 10 deterministic LCG-seeded i64 inputs per fixture across 7 single-arg fast-path fixtures (factorial, fib, abs_val, sign, sum_to, for_sum, countdown:factorial_iter); for each input it runs both CPython directly on the .py source and the rustc-compiled transpiled-Rust binary, asserts the stdout strings agree. 70 differential checks per CI run, all green at v0.1.0. Skip-gracefully if `python3` or `rustc` is missing from PATH. Each fixture's input range is hardcoded to stay inside the C-PY-INT-ARITH fast-path domain (no overflow panics); widening to overflow-prone ranges + interpreting `.checked_*().expect(...)` panic as "Python promoted to BigInt" is XPILE-DIFF-002. Multi-arg fixtures (gcd, range_size, bits, square_plus, safe_div) also XPILE-DIFF-002.

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

**Status (XPILE-REFINE-001)**: ✅ shipped — PMAT-017. First Layer-1 contract (`C-PY-INT-ARITH`) gets a `lean_theorem:` + `lean_file:` field on its `addition_no_overflow` equation. The file `contracts/lean/PyIntArith.lean` carries the theorem statement `fast_path_eq_slow_path` (proves `i64_wrap_add a b = bigint_add a b` when `fits_i64 (a + b)`). The proof is currently `sorry` — discharging it is XPILE-REFINE-002 (under `XPILE-PENDING-UNTIL: v0.3.0` per PMAT-014). The *statement* IS the load-bearing artefact: it's what `@[xpile_contract "C-PY-INT-ARITH"]` citations point at. Enforcement: `crates/xpile/tests/refinement_proofs.rs` walks every contract YAML, validates every `lean_theorem:` field points at a real file with a real theorem-of-that-name (closes the citation-bridge fragility caveat for this contract). Three stub theorems for mul/floor_div/mod listed in the file too, also under XPILE-PENDING-UNTIL gates.

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
