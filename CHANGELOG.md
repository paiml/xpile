# Changelog

All notable changes to xpile are recorded here. The project follows
[Semantic Versioning](https://semver.org/) once it stabilizes; while in
pre-1.0 development each minor version may include breaking changes to
meta-HIR and the trait surfaces.

## [Unreleased]

### Python subset (live, runtime-verified)

This list is the **canonical source of truth** for the supported subset.
The depyler-frontend module docstring points here. When extending the
subset, update this section first.

- Top-level `def name(p: int, q: int) -> int:` with optional type
  annotations for `int` and `bool`
- Multi-statement body: zero or more `let` assignments + final `return`
- Identifiers, integer literals
- Binary arithmetic: `+ - * // %` (floor div / mod use Euclidean
  semantics, matching Python on negative operands — not Rust/Lean's
  default truncate-toward-zero). Rust + Ruchy emission uses
  `.checked_*().expect(...)` so i64 overflow panics with a message
  pointing at the unimplemented bigint promotion slow path in contract
  `C-PY-INT-ARITH` (see `contracts/py-int-arith-v1.yaml`). Lean's `Int`
  is unbounded, so the same contract is satisfied by construction.
- Bitwise: `& | ^ << >>`. `& | ^` lower to plain infix in Rust/Ruchy
  (no overflow risk per-bit). Shifts use `checked_shl` / `checked_shr`
  with `u32::try_from(rhs)` so out-of-range shift amounts panic naming
  the same contract. Lean uses `Int.land` / `Int.lor` / `Int.xor` for
  `& | ^` and `<<<` / `>>>` with `.toNat` coercion for shifts.
- Power: `**`. Rust/Ruchy emit `checked_pow(u32::try_from(rhs).expect(...))`;
  negative exponents (which Python would promote to Float) panic naming
  `C-PY-INT-ARITH`. Lean uses `^` with `.toNat` (same fidelity gap as
  shifts on negative rhs).
- Comparisons: `== != < <= > >=`
- Logical: `and or` (short-circuit, Bool)
- Unary: `-x` (checked_neg, same overflow contract), `not x`
- Ternary: `x if cond else y`
- **Statement-level `if/else`** with single- *or multi-* assignment
  branches. Each assigned name is lifted to its own
  `let name: T = if cond { ... } else { ... }` (PMAT-005). Both
  branches must assign the same *set* of names; assignments can be in
  any order within each branch.
- **`if / elif* / else` chains** recursively lowered to nested
  `IfExpr`; pretty-printed as flat `else if` in Rust / Ruchy
- Function calls: `f(args)` (including self-recursion — `factorial`,
  `fib`-style)
- **`while` loops + mutable rebinding** (PMAT-006). A name that's
  reassigned anywhere in the function (including inside a loop body)
  gets `let mut`; subsequent assignments emit `name = value;`. The
  frontend infers mutability via a pre-walk that takes the max of
  if-branch counts (alternatives) and doubles inside loop bodies
  (repetition). Lean is unsupported for `while` — a follow-up will
  encode it as `partial def` with tail recursion.
- **`for target in range(...)`** desugaring (PMAT-007 + PMAT-008).
  Supports `range(stop)`, `range(start, stop)`, and `range(start, stop, step)`
  where `step` is any non-zero integer literal (positive *or* negative).
  Lowers to a `Let` init + `While target <cmp> stop` + `target = target + step`
  tail. Loop direction is decided at lower time from the literal's
  sign: positive step uses `<`, negative step uses `>`. Non-range
  iterables and non-literal / zero steps still error with a clear message.
- **`assert cond`** (PMAT-009). No-message form only. Rust/Ruchy emit
  `assert!(cond);`. Lean is skipped (requires Decidable instances +
  a propositional formulation; deferred).
- **`BigInt` slow-path scaffold** (PMAT-012). Annotate a function with
  `BigInt` (`def big_sum(a: BigInt, b: BigInt) -> BigInt`) and the
  Rust backend emits `xpile_bigint::BigInt` with plain infix arithmetic
  (no `.checked_*().expect()` — BigInt never overflows). Lean's `Int`
  is unbounded, so the same Python source produces the same Lean
  output regardless of `int` vs `BigInt`. Ruchy defers — emits a
  clear PMAT-012 error pointing at the Rust backend. Bitwise / shift
  / power on BigInt are still a follow-up.
- **Implicit BigInt promotion via return type** (PMAT-013). Annotate
  only the *return* as `BigInt` and the frontend auto-promotes every
  `int`-typed param to BigInt: `def factorial(n: int) -> BigInt:` reads
  naturally and produces a BigInt-mode function end-to-end. Codegen
  appends `.clone()` to BigInt Ident references (BigInt isn't `Copy`)
  so a name referenced in cond + branches + recursive call compiles
  cleanly.

### Backends (real emission)

- Rust target: `pub fn name(...) -> T { ... }`
- Ruchy target: `fun name(...) -> T { ... }`
- Lean 4 target: `def name (...) : T := ...` (uses `Int.fdiv` /
  `Int.fmod` to preserve Python floor semantics). Functions with a
  `while` loop emit a companion `partial def <fn>_loop_0` helper that
  threads loop-state variables as parameters and recurses with their
  updated values (PMAT-010). For-in-range, while + mutable rebinding,
  countdown loops — all transpile cleanly to Lean.

**Contract citations** (PMAT-011): every function whose body uses an
op governed by a Layer-1 contract carries a citation in the emitted
source — `// xpile-contract: C-PY-INT-ARITH` in Rust/Ruchy,
`@[xpile_contract "C-PY-INT-ARITH"]` in Lean. The applicability is
data-driven: comparison- or logical-only functions get no citation;
arithmetic / bitwise / shift / power / unary-neg functions do. The
Lean partial-def helper for a while-loop function carries the same
citation as the outer function.

Same Python source transpiles to all three via `xpile transpile <file.py> --target <t>`.

### Quality gates (on every PR via `.github/workflows/ci.yml`)

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `pv lint contracts/`
- `cargo deny check advisories`
- `cargo test --workspace`

### Lean `assert` via recursive if-then-panic encoding (PMAT-027 / PMAT-009-FOLLOWUP)

Closes one of the two `XPILE-PENDING-UNTIL: v0.3.0` markers. The
Lean codegen now lowers `Stmt::Assert` to a nested
`if cond then <rest> else panic!` chain that preserves Python's
evaluation order (innermost assert runs first because it's
deepest in the AST). Required refactoring `emit_block` into a
recursive `emit_stmts_then_trailing` that wraps each assert
around everything after it.

Sample (`safe_div` from `asserted.py`):

```
@[xpile_contract "C-PY-INT-ARITH"]
def safe_div (a : Int) (b : Int) : Int :=
  if ((b != (0: Int))) then
  if ((a >= (0: Int))) then
  (Int.fdiv a b)
  else panic! "xpile: assertion failed (contract C-PY-INT-ARITH)"
  else panic! "xpile: assertion failed (contract C-PY-INT-ARITH)"
```

Side effect: `xpile audit --target lean` jumps from F1=100% with
1 error (asserted.py) to F1=100% with 0 errors. The full Lean
corpus now compiles. Only one v0.3.0 marker remains (Lean
refinement-proof `sorry` discharge).

### BigInt bitwise / shift / power in Rust + Ruchy backends (PMAT-026 / PMAT-013-FOLLOWUP)

Closes the second of three `XPILE-PENDING-UNTIL: v0.2.0` markers.
Both Rust and Ruchy backends now handle `& | ^ << >> **` on
BigInt operands.

Implementation:
- `xpile-bigint` grows three helper functions: `shl(&BigInt, &BigInt)`,
  `shr(&BigInt, &BigInt)`, `pow(&BigInt, &BigInt)` — each converts
  the rhs from BigInt to the primitive type `num-bigint` wants
  (`usize` for shifts, `u32` for pow) with a contract-named panic
  on out-of-range / negative inputs.
- Rust + Ruchy codegens replace the `Unsupported` deferral with:
  * `& | ^` → plain infix (num-bigint impls these directly on
    BigInt operands)
  * `<< >> **` → calls to `xpile_bigint::{shl, shr, pow}`

After this PR, exactly **two `XPILE-PENDING-UNTIL: v0.2.0` markers
of three are closed** (Ruchy BigInt mode + Rust/Ruchy BigInt
bitwise/shift/power). The Lean v0.3.0 markers (assert + refinement
proofs) remain.

New fixture `bigint_bits.py` exercises the full BigInt-mode
bitwise+shift surface end-to-end.

### Ruchy BigInt mode (PMAT-025 / PMAT-012-FOLLOWUP)

Closes one of the three live `XPILE-PENDING-UNTIL: v0.2.0` markers
from PMAT-014. The Ruchy backend now supports BigInt-typed
functions end-to-end, mirroring the Rust backend's PMAT-012/013
emission. `xpile transpile foo.py --target ruchy` on a fixture
with `BigInt` annotations now produces clean Ruchy source with
`xpile_bigint::BigInt` typed signatures, `.clone()` on Ident
references, plain infix arithmetic, and the contract citation.

Sample:
```
$ xpile transpile crates/xpile/tests/fixtures/big_sum.py --target ruchy
// xpile-contract: C-PY-INT-ARITH
fun big_sum(a: xpile_bigint::BigInt, b: xpile_bigint::BigInt) -> xpile_bigint::BigInt {
    (a.clone() + b.clone())
}
```

Implementation: mechanical mirror of the Rust pattern — added
`function_bigint_mode(f)` + threaded `mode: bool` through every
`emit_*` function. Reused the same `xpile_bigint::div_floor` /
`mod_floor` helpers and the same bitwise/shift/power deferral
(now under a `[XPILE-PENDING-UNTIL: v0.2.0, ticket: PMAT-013-FOLLOWUP]`
marker shared with Rust).

Removed the previous `bigint_ruchy_errors_with_pmat_012_message`
test (bait test that asserted the bail path); replaced with two
positive tests asserting the Ruchy emission shape for explicit
and implicit BigInt promotion.

### Multi-arg fixtures in differential exec gate (PMAT-024 / XPILE-DIFF-002)

`crates/xpile/tests/diff_exec.rs` generalised from 1-arg-only to
support 2-arg fixtures via per-arg input ranges. Three new 2-arg
fixtures: `gcd`, `range_size`, `bits`. **Total: 100 differential
checks across 10 fixtures per CI run** (up from 70 across 7),
all green. Driver synthesis builds the right
`entry(argv[0], argv[1], ...)` call expression at the configured
arity. Still pending: overflow-prone ranges + panic-as-BigInt
interpretation (XPILE-DIFF-003).

### Refine F1 to applicable-contracts denominator + Lean target (PMAT-023 / XPILE-FALSIFY-002)

`xpile audit`'s F1 metric is now computed against only the
functions where `Function::applicable_contracts()` is non-empty —
the *applicable-contracts denominator*. Pre-002 the denominator was
every emitted function, which double-penalised comparison-only
and logical-only functions that correctly emit no citation by
design. With the refinement, F1 on the current corpus jumps from
83.3% [WARN] to 100.0% [OK].

Also added `--target lean`: the audit now recognises Lean's
`@[xpile_contract "..."]` attribute alongside Rust/Ruchy's
`// xpile-contract:` comment form.

New `over_citations` JSON field is a sanity check for the
symmetric failure mode (codegen wrongly cites a comparison-only
function); currently 0.

### Extend deadline scan to proof-lane + Kani harnesses (PMAT-022 / XPILE-EXEMPT-002)

Widens `crates/xpile/tests/exempt_deadlines.rs` from "Rust source
under `crates/*/src/`" to also cover `contracts/lean/*.lean` and
`contracts/kani/*.rs`. The `XPILE-PENDING-UNTIL: v0.3.0` marker
inside `PyIntArith.lean`'s `sorry` proof was effectively
decorative before; now it's gated alongside the codegen markers.
New `scanner_picks_up_proof_lane_markers` test asserts the
widening worked.

### Kani job in CI (PMAT-021 / XPILE-QUORUM-003)

New dedicated `kani` job in `.github/workflows/ci.yml` installs
`kani-verifier`, runs `cargo kani-setup`, and runs the
`kani_verify` workspace test against every harness on every PR.
Kept as a separate job (not bundled with `workspace-test`) so the
~5-minute cold-cache Kani install doesn't slow fast-feedback
gates. Not a required status check yet — flip after Kani has
bedded in for a release cycle. Symbolic stratum is now load-bearing
on every PR, not just locally.

### Run Kani harnesses in workspace tests (PMAT-020 / XPILE-QUORUM-002)

Converts the Symbolic stratum from claim to fact. New
`crates/xpile/tests/kani_verify.rs` walks every `contracts/kani/*.rs`
file, materialises a temp Cargo crate per harness, runs `cargo kani`,
asserts exit-0 AND stdout contains `VERIFICATION:- SUCCESSFUL`
(grep guards against Kani's historical "exit 0 on swallowed solver
error" failure mode). Skip-gracefully if `cargo-kani` is missing
from PATH; local users with Kani installed get the gate
automatically. Still remaining: install Kani in CI so the gate
fires on every PR (XPILE-QUORUM-003).

### Symbolic stratum: Kani harness for C-PY-INT-ARITH (PMAT-019 / XPILE-QUORUM-001)

First **Symbolic stratum** of the N-of-M oracle quorum lands.
`contracts/kani/py_int_arith.rs` carries `#[kani::proof]` functions
for `addition_no_overflow` (and a stub `subtraction_no_overflow`);
Kani 0.67 discharges both via bit-blasted i64 arithmetic in ~27ms.
`contracts/py-int-arith-v1.yaml` grows `kani_harness:` + `kani_file:`
fields wiring the citation; the new
`crates/xpile/tests/kani_harnesses.rs` validates every cited harness
exists in its file with a real `#[kani::proof] fn <name>(...)`.

Combined with PMAT-017's Lean theorem (Semantic stratum) and
PMAT-018's diff_exec runtime check (Semantic stratum), the
`addition_no_overflow` equation now has ≥1 Symbolic + ≥1 Semantic
vote per ruchy 5.0 §14.4 quorum rule.

What this does NOT include yet (XPILE-QUORUM-002+): running
`cargo kani` in CI on every PR; the §14.5 F3 pairwise-correlation
guard; Extrinsic (human review) verdict-recording.

### Differential execution check (PMAT-018 / XPILE-DIFF-001)

New `crates/xpile/tests/diff_exec.rs` runs deterministic LCG-seeded
i64 inputs through both CPython (on the original .py source) and
the rustc-compiled transpiled-Rust binary, asserts their stdout
strings agree. 10 inputs × 7 single-arg fast-path fixtures = 70
differential checks per CI run. Skip-gracefully if `python3` or
`rustc` is missing from PATH. Each fixture's input range is
hardcoded to stay inside the C-PY-INT-ARITH fast-path domain;
widening to overflow-prone ranges + multi-arg fixtures is
XPILE-DIFF-002. Generalises the 11 hand-authored runtime-verified
fixtures into a quantitative gate against fixture overfitting
(audit-design.md §4 caveat).

### Lean refinement proof for C-PY-INT-ARITH (PMAT-017 / XPILE-REFINE-001)

First contract YAML grows `lean_theorem:` + `lean_file:` fields on
its equations. `contracts/py-int-arith-v1.yaml` points at
`contracts/lean/PyIntArith.lean`'s `fast_path_eq_slow_path`
theorem, which states `i64_wrap_add a b = bigint_add a b` when
`fits_i64 (a + b)`. Proof is currently `sorry`-discharged
(XPILE-REFINE-002 follows-up); the *statement* is what the citation
pipeline points at via `@[xpile_contract "C-PY-INT-ARITH"]`.

Enforcement test (`crates/xpile/tests/refinement_proofs.rs`) walks
every contract YAML, asserts every `lean_theorem:` field references
a real file with a real theorem of that name. Closes the
citation-bridge-fragility audit caveat for this contract.

### Quarterly SOTA-gap dossier cadence (PMAT-016 / XPILE-SOTA-001)

`audit-design.md` §0 publishes the quarterly cadence + the next
dossier deadline. Enforcement test (`crates/xpile/tests/sota_dossier_deadline.rs`)
parses the deadline string, compares against wall-clock time, fails
CI when current ≥ deadline. Missing dossier ⇒ falsifier F6 fires
automatically, no manual policing.

Cadence as of v0.1.0: 2026-Q2 (initial — §1..§6 of audit-design.md);
2026-Q3 deadline 2026-08-15; 2026-Q4 deadline 2026-11-15;
2027-Q1 deadline 2027-02-15.

### `xpile audit` (PMAT-015 / XPILE-FALSIFY-001)

New CLI subcommand reports F1 (Layer-1 contract citation coverage)
on a corpus. Walks the given path, runs the transpile pipeline on
every source file the dispatch table recognises, parses the emitted
output for `// xpile-contract: <ID>` citations adjacent to function
declarations, reports % coverage with the §27 roadmap's
OK/WARN/FAIL thresholds (≥95% / ≥50% / <50%). Text + `--json`
modes. Current baseline against `crates/xpile/tests/fixtures/`:
F1 ≈ 83% (WARN — gap is by design; comparison-only functions
correctly don't carry the citation). Lean target is XPILE-FALSIFY-002.

### Time-bounded escape hatches (PMAT-014 / XPILE-EXEMPT-001)

Every "not yet implemented" panic / `Unsupported(...)` error in the
codegen carries an explicit `[XPILE-PENDING-UNTIL: v<semver>, ticket: <ID>]`
marker. A workspace test (`crates/xpile/tests/exempt_deadlines.rs`)
scans every `.rs` file under `crates/*/src/` for the marker and
asserts the current workspace version is strictly less than every
deadline. CI fails the moment a deadline is reached without the
underlying feature shipping — closes the "unimplemented forever"
hole. Adapted from ruchy 5.0 §14.7 (`#[contract_exempt(until)]`).
Current live markers:

- `Ruchy BigInt mode` — until v0.2.0, ticket PMAT-012-FOLLOWUP
- `Rust BigInt bitwise/shift/power` — until v0.2.0, ticket PMAT-013-FOLLOWUP
- `Lean assert` — until v0.3.0, ticket PMAT-009-FOLLOWUP

### Verification milestones

Ten runtime-verified semantic round-trip fixtures (emit → `rustc -O`
→ execute → `assert_eq!`):

- `factorial(n)` — recursive, `factorial(10) == 3628800`
- `fib(n)` — binary recursion, `fib(15) == 610`
- `gcd(a, b)` — tail recursion with `%`, `gcd(12, 18) == 6`
- `abs_val(x)` — statement-level if/else, `abs_val(-100) == 100`
- `sign(x)` — if/elif/else chain, `sign(i64::MIN) == -1`
- `bits(a, b)` — pins `& | ^ << >>` semantics, `bits(5, 3) == 14`
- `square_plus(a, b)` — pins `**` semantics, `square_plus(2, 3) == 10`
- `range_size(a, b)` — multi-assignment if-branches, `range_size(3, 7) == 4`
- `sum_to(n)` — while-loop accumulator, `sum_to(100) == 5050`
- `for_sum(n)` / `range_with_start` / `range_with_step` — for-in-range
  desugaring, all three `range(...)` shapes
- `factorial_iter(n)` — negative-step countdown, `factorial_iter(10) == 3628800`
- `safe_div(a, b)` — assert-precondition fixture, `safe_div(10, 2) == 5`

32 e2e tests across `crates/xpile/tests/transpile_e2e.rs`; ~60
workspace tests total.

## [0.0.1] - 2026-05-15

Initial crates.io name-reservation release. Placeholder binary that
prints a banner pointing at the GitHub repo. The full v0.1.0+ binary
is tracked in this workspace.

Published: <https://crates.io/crates/xpile/0.0.1>.
