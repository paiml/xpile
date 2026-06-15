<p align="center">
  <img src="docs/assets/hero.svg" alt="xpile architecture diagram: a code lane (Python, C, C++, Rust, Ruchy, Lean 4, Shell → meta-HIR → Rust, Ruchy, PTX, WGSL, SPIR-V, Lean 4, Shell) and a proof lane (LaTeX, Lean theorems, mdBook ↔ contracts)" width="100%"/>
</p>

# xpile

[![ci](https://github.com/paiml/xpile/actions/workflows/ci.yml/badge.svg)](https://github.com/paiml/xpile/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/xpile.svg)](https://crates.io/crates/xpile)
[![license](https://img.shields.io/crates/l/xpile.svg)](#license)

**A polyglot transpile workbench with provable contracts at every layer.** Seven language frontends (Python, C, C++, Rust, Ruchy, Lean 4, Shell) share one canonical meta-HIR and dispatch through seven backends (Rust, Ruchy, PTX, WGSL, SPIR-V, Lean 4, Shell), all alongside a **proof lane** that round-trips between LaTeX, Lean 4 theorems, and mdBook through a shared YAML contract substrate. Built to solve **hybrid transpilation** — single artifacts that cross language boundaries (CPython + C extensions, Python + CUDA kernels, Python + shell scripts) — which separate per-language repos cannot.

## Status — v0.1.0

**It transpiles, semantic round-trip verified in CI.** A non-trivial recursive Python function transpiles to Rust that compiles _and computes the right values_:

```python
# factorial.py
def factorial(n: int) -> int:
    return 1 if n <= 1 else n * factorial(n - 1)
```

```bash
$ xpile transpile factorial.py
// xpile-generated from Python module factorial

pub fn factorial(n: i64) -> i64 {
    if (n <= 1i64) { 1i64 } else {
        (n).checked_mul(factorial(
            (n).checked_sub(1i64).expect("xpile: i64 subtraction overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented")
        )).expect("xpile: i64 multiplication overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented")
    }
}
```

Note the `.checked_*().expect(...)` wrappers — every arithmetic op enforces the Layer-1 contract [`py-int-arith-v1.yaml`](contracts/py-int-arith-v1.yaml): i64 overflow panics with a pointer to the unimplemented bigint slow path instead of silently wrapping. (Lean's `Int` is unbounded, so the same contract is satisfied by construction.)

CI runs `rustc -O` on the output and asserts `factorial(10) == 3628800` — the test is `factorial_emitted_rust_computes_correct_values`.

Same source, three different targets:

```bash
$ xpile transpile factorial.py --target ruchy
fun factorial(n: i64) -> i64 {
    if (n <= 1i64) { 1i64 } else {
        (n).checked_mul(factorial((n).checked_sub(1i64).expect("..."))).expect("...")
    }
}

$ xpile transpile factorial.py --target lean
def factorial (n : Int) : Int :=
  if (n <= (1: Int)) then (1: Int) else (n * (factorial (n - (1: Int))))
```

**By the numbers (live, not aspirational):**

- 27 workspace crates · all compile clean (`cargo check --workspace`)
- 12 contracts · `pv lint` PASS with **0 errors and 0 warnings** (full-clean substrate since PMAT-138)
- **100% QUORUM + UNIVERSAL 5-TIER + UNIVERSAL Diamond depth-3..13 across all 12 contracts — Diamond CI gate enforced** — every contract has paired Lean refinement theorem + Kani BMC harness; **638 stratum-vote artifacts** (285 Semantic + 53 Symbolic + 15 Runtime + 285 Extrinsic) across all 5 taxonomy layers. 42/42 equations at Silver; 12/12 contracts at Gold/Platinum; **eleven UNIVERSAL Diamond milestones** depth-3..13 (PMAT-336..442) with **171 wired Diamond theorems** across 12 contracts; deepest contracts at depth-21 (PyIntArith L1) and depth-20 (CompileRustToPtxMma L5). Thirteen recurring algebraic templates discovered: structure-extensionality (32+ contracts), enum completeness, Gold-tier subtype-ext, tier-projection homomorphism, canonical identity, Bronze↔Silver round-trip. Diamond coverage CI-enforced via `diamond_coverage.rs` (22 integration tests, depth-1..13 UNIVERSAL gates) — regressions fail builds. Reporter: `xpile diamond --json`
- **297 workspace tests** · 11+ Python fixtures runtime-verified via `rustc -O` + `assert_eq!` (canonical list in `CHANGELOG.md` §"Python subset"); plus 54 bashrs-frontend tests covering POSIX shell idioms; 22 `diamond_coverage.rs` integration tests gate depth-1..13 UNIVERSAL invariants
- **`pmat tdg .` score 95.1 / 100 (Grade A-)** — meets the originally-planned XPILE-CI-PMAT-TDG-001 ≥ A- threshold without explicit CI enforcement (slight dip from 95.7 reflects the +600 lines of Diamond-program documentation; still solidly A-)
- Python subset shipped: see [`CHANGELOG.md`](CHANGELOG.md) §"Python subset (live, runtime-verified)" — typed `def`, multi-statement bodies, all binary + unary ops, ternary, if/else, elif chains, function calls including self-recursion (canonical source — this README intentionally does not duplicate the list to avoid the staleness it kept accumulating)
- **Four real backends**: Rust (`pub fn`, Python-floor semantics via `checked_div_euclid` / `checked_rem_euclid`, all arithmetic checked for the `C-PY-INT-ARITH` contract), Ruchy (`fun ... -> T`, same overflow semantics — compiles to Rust), Lean 4 (`def`, `Int.fdiv` / `Int.fmod`; `Int` is unbounded so the contract holds by construction), bashrs (POSIX shell — see [`sub/bashrs-merger.md`](docs/specifications/sub/bashrs-merger.md))
- CI: `gate` + `kani` + `workspace-test` all run on every PR; `gate` is the load-bearing required status check (org-level ruleset rule); `kani` + `workspace-test` are not yet required but in practice green on every merged PR. Branch protection: `non_fast_forward` + PR required + `gate` status check (`gh api repos/paiml/xpile/rules/branches/main`).
- Latest tag: **`v0.1.383`** — capability: `enumerate(xs, start)` / `enumerate(xs, start=N)` inside a list comprehension (`[(i, x) for i, x in enumerate(xs, 1)]`) now transpiles (it was rejected) — the index is offset by `start` like Python. Recent: v0.1.382 a non-literal chained assignment over a Copy scalar (`a = b = n + 1`, `x = y = z = n*2`) now transpiles (it was rejected) — bound once to a temp and copied to each target, v0.1.381 a width-only format spec on a string (`f"{s:5}"`, `"{:8}".format(s)`) now transpiles (it was rejected) — left-aligned to the width like Python, via Rust's `{:N}`, v0.1.380 `bool(x)` over a float now transpiles to `x != 0.0` like Python (it was rejected) — 0.0/-0.0 falsy, NaN/inf truthy, v0.1.379 a float `.Nf` format spec over NaN via `.format()` / `%` now prints `nan` like Python (it printed Rust's `NaN`) — routed through the same NaN-guarded path the f-strings use, v0.1.378 `sum()` over a list of floats containing `inf` now returns `inf` like Python (it returned `NaN` — the Neumaier compensation computed `inf - inf`); finite catastrophic-cancellation accuracy preserved, v0.1.377 an identity comprehension `[w for w in words]` over `list[str]` now keeps its element type, so `sorted([w for w in words])` / `max(...)` transpile (they were rejected as `List(I64)`), v0.1.376 float format specs combining width/zero-pad/sign/align WITH precision (`f"{x:8.3f}"`, `{:06.2f}`, `{:>8.2f}`) now transpile (they were rejected) — the most common numeric-table idiom; matches Python via Rust's `{:8.3}`, v0.1.375 a bare-radix `str.format` of a negative int (`"{:x}".format(-255)`) now emits sign-magnitude (`-ff`) like Python, not Rust's two's-complement (`ffffffffffffff01`), v0.1.374 str `.find(sub, start[, end])` / `.count(sub, start[, end])` now transpile (they were rejected) — searching within the char-slice `s[start:end]` with Python clamping, returning correct char indices for non-ASCII, v0.1.373 `len(s.encode())` now transpiles to the UTF-8 byte length (`s.len()`) — it was rejected (`.encode()` is an unsupported method call); distinct from `len(s)` which counts Unicode code points, v0.1.372 `a + b` over two tuples now transpiles as concatenation (`(1, 2) + (3, 4)` → `(1, 2, 3, 4)`) — it was rejected (Rust tuples have no `+`); lowered to a fresh tuple of all fields of both operands, v0.1.371 a chained comparison (`a < b < c`) now SHORT-CIRCUITS like Python — it stops at the first false sub-comparison and never evaluates the trailing operands, so a panic-prone or side-effecting trailing operand (`10 < n < (100 // dv)` with `dv == 0`) no longer runs when an earlier compare is false, v0.1.370 `x in t` / `x not in t` over a fixed-arity tuple now works (it was rejected as "unsupported comparison operator: In") — lowered to a chained-OR of equalities `x == t.0 || x == t.1 || …`, v0.1.369 a negative constant tuple index `t[-1]` now resolves to the field access `t.(len-1)` at compile time (Python from-the-end) instead of emitting list-style `.len()` indexing (E0599 — Rust tuples have no `.len()`); a runtime-variable tuple index now rejects cleanly, v0.1.368 `sum(t)` / `min(t)` / `max(t)` over a fixed-arity tuple now work (e.g. `sum((3, 7, 2))` → 12) instead of emitting an undefined `sum(t)` free call (E0425); a tuple is materialized to a list of its elements, like the other iterables, v0.1.367 `sep.join(d)` over a dict now joins its keys (like Python) instead of emitting `d.join(...)` on a HashMap (E0599); the join argument is materialized to the dict's keys, the same as `sep.join(d.keys())`, v0.1.366 a bool-result `and`/`or` with a container/int operand in a boolean context — `if xs and xs[0] > i:` — now works (it was rejected as "operands must be Bool"); each operand is coerced to its truthiness and folded with `&&`/`||`, while the value-returning `x or 5` form is unchanged, v0.1.365 `str.zfill`/`center`/`ljust`/`rjust` with a **negative width** now return the string unchanged like Python, instead of panicking (a bare `as usize` cast underflowed the negative width to a huge value → capacity-overflow); the width is clamped with `.max(0)`, v0.1.364 `any()` / `all()` over a `list[int]` / `list[float]` / `list[str]` now apply Python per-element truthiness (e.g. `any([0, 0, 3])` → True) instead of emitting an undefined `any(xs)` free call (E0425); each element is mapped to a bool (int → `!= 0`, float → `!= 0.0`, str → non-empty) before the reduce, v0.1.363 `round(x)` now guards inf/nan and out-of-i64 range — `round(float("inf"))`/`round(float("nan"))` raise like Python (OverflowError/ValueError) instead of silently saturating to `i64::MAX`/garbage; normal banker's rounding is unchanged, v0.1.362 `not <int>` / `not <float>` / `not len(xs)` now work (they were rejected as "not requires Bool operand") — `not n` lowers to `n == 0` like Python, completing the int/float truthiness support for the `not` operator, v0.1.361 a `_` discard in a tuple-unpack is never `mut` — `a, _, _ = t` and `_, _, _, _ = (…)` used to emit invalid `mut _` ("`mut` must be followed by a named binding") when the discard repeated; named bindings still get `mut` when mutated, v0.1.360 int/float truthiness in conditions — `if n:`, `if len(xs):`, `while n:`, and `n if c else d` now work (they were rejected as "no int-truthiness"); a nonzero int/float is truthy like Python (the float form matches the edges: `-0.0` falsy, `nan` truthy), v0.1.359 `list.extend()` now accepts any iterable — `xs.extend(range(n))`, `xs.extend((a, b, c))`, and `xs.extend(xs)` (self) all work (they used to fail to compile: E0425/E0599/E0502); the arg is materialized like the other builtins (range→Vec, set→list, tuple→list) and self-extend clones first, v0.1.358 an f-string float-precision spec (`f"{x:.2f}"`, `f"{x:.0%}"`) of a NaN now prints "nan" like Python instead of Rust's "NaN" (inf already matched), v0.1.357 a format-spec fill char before an alignment (`"{:->10}"`, `"{:*<8}"`, `"{:.^9}"`) now works — a `-` fill used to be mistaken for a sign flag and dropped (→ space padding), and `*`/`.` fills were rejected; Rust's `{:fill<width}` syntax is identical to Python's, v0.1.356 `divmod(a, b)` with float operands now lowers to a `(a // b, a % b)` float tuple instead of an undefined `divmod(...)` free call (E0425); the float floor-div/mod ops already match CPython (sign follows the divisor), v0.1.355 `max(d)` / `min(d)` / `sum(d)` over a dict now iterate its keys (like Python) instead of emitting an undefined `max(d)` free call (E0425); the builtin's argument-materializer now treats a bare dict as its keys, the same as `max(d.keys())`, v0.1.354 `int(s, base)` now accepts a base-matching radix prefix (`0x`/`0o`/`0b`) and PEP-515 underscore digit grouping like Python — `int("0xff", 16)` → 255, `int("1_000", 16)` → 4096 (these used to panic, since Rust's `from_str_radix` accepts neither), v0.1.353 `len(tuple)` now folds to the tuple's length (a compile-time constant) instead of emitting `t.len()` — Rust tuples have no `.len()` method, so this used to fail with E0599, v0.1.352 `max(xs, key=…)` / `min(xs, key=…)` with a **float-returning key** (e.g. `max(items, key=lambda x: x / total)`) now compiles — it emitted `max_by_key`/`min_by_key`, which need `f64: Ord` (E0277); a float key now uses `max_by`/`min_by` with `partial_cmp` (ties still resolve to the first element, like Python), v0.1.351 set relational predicates (`a <= b`, `>=`, `<`, `>`, `a.issubset(b)`, `issuperset`, `isdisjoint`) no longer move their operands — comparing a set then reusing it (`a <= b; … len(a)`) or self-comparing (`a <= a`, `a.isdisjoint(a)`) used to fail with E0382; the predicates now bind their operands by reference, v0.1.350 float floor-division now preserves the sign of a zero result — `-0.0 // 1.0` yields `-0.0` like Python, instead of `+0.0` (the fmod-based formula's `floor(0.0)` dropped the sign; now mirrors CPython's `copysign(0.0, a/b)` zero-quotient branch), v0.1.349 unary negation of a float variable now preserves the sign of a zero — `-x` with `x == 0.0` yields `-0.0` (printed "-0.0") like Python, instead of `+0.0` (the old `0.0 - x` lowering lost the sign because `0.0 - 0.0 == +0.0` in IEEE-754; now emits `x * -1.0`, bit-exact with `-x`), v0.1.348 `str.split()` with no argument now splits on the C0 separators U+001C-1F (FS/GS/RS/US) like Python's `str.isspace()`-based split, not just Rust's narrower `split_whitespace` set (was a silent miscompile — fewer parts when those control chars were present), v0.1.347 `@dataclass(order=True)` now derives `PartialOrd`, so instance comparisons (`Point(1,2) < Point(1,3)`, `<=`/`>`/`>=`) compile (was an E0369 — the derive lacked `PartialOrd`); ordering is lexicographic by field, matching Python's tuple comparison, and works for float fields, v0.1.346 a starred-unpack binding (`a, *rest = xs`) is now `let mut` when later mutated (`rest.append(...)`), fixing an E0596 compile error in the new starred-unpack feature, v0.1.345 starred unpacking at any position (`*init, last = xs`, `first, *mid, last = xs`) now transpiles like Python, extending the star-last form, v0.1.344 starred unpacking `a, *rest = xs` (head/tail destructuring, star last) now transpiles like Python (was rejected), v0.1.343 `str.rsplit(sep, maxsplit)` is now supported (split from the right capping at `maxsplit` splits, e.g. `name.rsplit(".", 1)`), v0.1.342 `str.isnumeric()` is now supported (a 0-arg classification predicate; Unicode-numeric via `char::is_numeric()`, so `½`/`²` are numeric but not digits), v0.1.341 `enumerate(xs, start=-1)` / `enumerate(xs, -5)` (negative literal start) now transpiles like Python; was rejected as a "non-literal start", v0.1.340 a runtime-negative index at any nesting level of a list-index WRITE `grid[i][j] = v` now wraps like Python (`grid[-1][-1] = v`), completing negative-indexing (read + single write + nested write), v0.1.339 a runtime-negative list-index WRITE `xs[i] = v` / `xs[i] += v` (i<0) now wraps like Python (`xs[-1] = v` targets the last element) instead of panicking — the assign-side companion to v0.1.338's read fix, v0.1.338 a runtime-negative list index `xs[i]` (i<0 at runtime) now wraps like Python (`xs[-1]` is the last element) instead of panicking via `usize` underflow, v0.1.337 short-circuit chains `a or b or c` / `a and b and c` (e.g. `name or env or "default"`) now return the first decisive operand by truthiness like Python, extending the 2-operand form, v0.1.336 `x or default` / `x and y` now return the operand by truthiness like Python (`0 or 5` → `5`, `"" or "d"` → `"d"`, `xs or [9,9]`); the common default-value idiom was previously rejected (operands had to be Bool), v0.1.335 `int ** <negative int literal>` now yields a float like Python (`2 ** -1` → `0.5`); was rejected (the integer power path can't represent a negative exponent), v0.1.334 correctness: range-comprehension variables (`[i for i in range(n)]`, dict/set forms) no longer leak into / clobber an enclosing same-named binding — they're scoped to the comprehension like Python (the counter is renamed to a fresh synthetic name), v0.1.333 correctness: for-range loop-variable semantics — nested same-name loops (`for i: for i:`), the post-loop leaked value, and empty-range no-clobber now match Python (the desugar drives a fresh synthetic counter instead of the user variable), v0.1.332 stepped string slices `s[a:b:step]` (`"abcdef"[::2]` → `"ace"`, `[::-2]` → `"fdb"`) now transpile, giving str full step parity with list (was refused), v0.1.331 `str.rjust`/`ljust`/`center` accept the optional fill-char arg (`"ab".rjust(5, "*")` → `"***ab"`); the 2-arg form was previously refused, v0.1.330 correctness: `raise E(msg)` emits a typed panic payload (`xpile: ValueError: neg`) so distinct exception types are distinguishable (was an identical `"neg"` payload) — the first sub-slice of the typed-exceptions epic, v0.1.329 a `bool` `and`/`or`/`not`/ternary call argument (`g(5, c and d)`) is lowered context-aware instead of rejected, v0.1.328 `s *= n` (str) / `xs *= n` (list) are repetition not numeric multiply (was E0599 / rejected), v0.1.327 a non-Copy variable reused in a list literal (`[inner, inner]`) or appended twice is cloned instead of move-then-used → E0382, v0.1.326 a default-using function called in argument position (`f(g(x))`) fills the nested call's defaults too (was E0061), v0.1.325 `str(list)`/`str(tuple)` and `print(list)`/`print(tuple)` render the Python repr — completing the list/tuple repr surface. (v0.1.13 PTX gate.) A sustained run of correctness fixes comes from multi-pass adversarial differential python3-vs-rustc hunts (str Unicode, bool-as-int, tie/stability, modpow, keyword identifiers, mut-receiver-in-condition, the i64-overflow contract trio, chained-compare double-eval, sorted-over-float, bool-bitwise type, float div-by-zero, repr(), float scientific notation, float-sum compensation, the ownership clones field-read + call-arg-reuse, int-cast non-finite + out-of-range, prelude-type-name reject, list.insert index clamp, float-mod fmod parity, frozen-dataclass Eq+Hash, PEP 584 dict union, enumerate start= keyword, int sum/enumerate overflow contract, reversed(str), format() builtin, empty-set element inference, dict-comp key clone, C0-separator whitespace, float max/min first-arg-wins, annotation/Optional mismatch reject, sort float-key partial_cmp, subscript list-concat aug-assign, pow negative-modulus sign, math.floor/ceil/trunc range guard, pow bool-base coercion, float max/min empty→ValueError, pop runtime-negative index, int(str) PEP 515 underscores).
- Published on crates.io: [`xpile 0.1.37`](https://crates.io/crates/xpile) (`cargo install xpile`) — the full v0.1.14→v0.1.37 line shipped in the 2026-06-12 Friday batch (all 27 workspace crates). Per the release cadence, crates.io publishes **once per week on Fridays**; GitHub tags ship per slice — so the crates.io line catches up to the latest tag each Friday (next window 2026-06-19, which will pick up v0.1.38+).

### Contract substrate at QUORUM

The ruchy 5.0 §14.4 N-of-M oracle quorum rule requires ≥1 vote in ≥3 strata
(Semantic / Symbolic / Runtime / Extrinsic) for a contract to be considered
discharged. As of v0.1.0, every contract clears this bar:

```text
$ xpile quorum
  ... (12 contracts, all at QUORUM)
  totals: 12 QUORUM, 0 PARTIAL, 0 UNVERIFIED (12 contracts total)
```

**285 Semantic + 53 Symbolic + 15 Runtime + 285 Extrinsic = 638 stratum-vote artifacts** across all 5 layers of the contract taxonomy, via [`contracts/lean/*.lean`](contracts/lean/) and [`contracts/kani/*.rs`](contracts/kani/). **42/42 equations at Silver** + **12/12 contracts at Gold/Platinum** + **eleven UNIVERSAL Diamond milestones depth-3..13** (PMAT-336..442) totalling **171 wired Diamond theorems**; deepest 21 (PyIntArith L1), 20 (CompileRustToPtxMma L5). Diamond coverage is CI-enforced via the `diamond_coverage.rs` gate (22 integration tests, depth-1..13 UNIVERSAL) — regressions fail builds.
Every equation in every contract has both its own Bronze-tier Lean theorem
(`rfl` by construction) AND its own Kani symbolic harness exploring 256^4 ≈
4.3B configurations per harness. Silver/Gold/Platinum refinement is
incremental from here as concrete impl pressure arrives.

> **Canonical spec:** [`docs/specifications/xpile-spec.md`](docs/specifications/xpile-spec.md) — TOC + 25 sections, each linking to a `sub/<topic>.md`.
>
> **Adversarial audit:** [`docs/specifications/audit-design.md`](docs/specifications/audit-design.md) — Popperian falsification record (4 hypotheses).

## Two lanes, one substrate

xpile has two parallel pipelines that share the YAML contract substrate. Trait-level detail in [`sub/frontend-trait.md`](docs/specifications/sub/frontend-trait.md), [`sub/backend-trait.md`](docs/specifications/sub/backend-trait.md), [`sub/contract-frontend-trait.md`](docs/specifications/sub/contract-frontend-trait.md), [`sub/contract-backend-trait.md`](docs/specifications/sub/contract-backend-trait.md).

### Code lane (executable code)

```
Frontends                      Backends
─────────                      ─────────
Python   ─┐               ┌─→ Rust        ✅ real emission
C        ─┤               ├─→ Ruchy       ✅ real emission
Shell    ─┤               ├─→ Shell       ✅ real emission (POSIX, PMAT-037..058)
C++      ─┼→ meta-HIR ─→ ─┼─→ PTX         🚧 scaffold + Layer-5 contract (QUORUM)
Rust     ─┤               ├─→ WGSL        🚧 scaffold
Ruchy    ─┤               ├─→ SPIR-V      🚧 planned
Lean 4   ─┘               └─→ Lean 4      🚧 scaffold
```

### Proof lane (notation + proofs)

```
ContractFrontends             ContractBackends
─────────────────             ─────────────────
LaTeX       ─┐                  ┌─→ LaTeX (papers)
Lean 4 thm  ─┼─→ contracts ←──←─┼─→ Lean 4 theorems
mdBook      ─┘                  └─→ mdBook
```

Lean 4 spans both lanes. LaTeX is proof-lane-only. Citation bridge uses **format-native structured constructs** (`@[xpile_contract "..."]` attribute in Lean, `\xpileContract{...}{...}` macro in LaTeX, structured comment in mdBook) — never regex over body text. Revised post-audit; see [`sub/contract-backend-trait.md`](docs/specifications/sub/contract-backend-trait.md) §"Citation bridge".

## Quick orientation

| Question | Section |
|---|---|
| What is xpile and why does it exist? | [§1 Vision and Architecture](docs/specifications/sub/vision.md) |
| How do I add a new language? | [§17 Frontend Onboarding](docs/specifications/sub/frontend-onboarding.md) |
| Lean 4 in both lanes? | [§24 Lean 4 Bidirectional](docs/specifications/sub/lean-bidirectional.md) |
| LaTeX in the proof lane? | [§25 LaTeX Bidirectional](docs/specifications/sub/latex-bidirectional.md) |
| What is hybrid transpilation? | [§16 Hybrid Transpile Flow](docs/specifications/sub/hybrid-transpile-flow.md) |
| How does the agent loop work? | [§7 Bounded Agent Repair Loop](docs/specifications/sub/agent-loop.md) |
| How are contracts validated? | [§11 Provable Contracts (`pv`)](docs/specifications/sub/pv-integration.md) |
| What's the contract taxonomy? | [§13 Contract Taxonomy](docs/specifications/sub/contract-taxonomy.md) (5 layers × 2 lanes) |
| What are the quality gates? | [§12 `pmat`](docs/specifications/sub/pmat-integration.md) + [§18 CI Pipeline](docs/specifications/sub/ci-gates.md) |

## Contracts at v0.1.0 (12, all at QUORUM)

| Contract | `pv` kind | Layer × Lane | What it pins down | Refinements |
|---|---|---|---|---|
| `xpile-frontend-trait-v1.yaml` | pattern | 3 architectural / code | Frontend trait invariants | [Lean](contracts/lean/XpileFrontendTrait.lean) · [Kani](contracts/kani/xpile_frontend_trait.rs) |
| `xpile-backend-trait-v1.yaml` | pattern | 3 / code | Backend trait + structural compile-contract citation | [Lean](contracts/lean/XpileBackendTrait.lean) · [Kani](contracts/kani/xpile_backend_trait.rs) |
| `xpile-contract-frontend-trait-v1.yaml` | pattern | 3 / proof | ContractFrontend trait invariants | [Lean](contracts/lean/XpileContractFrontendTrait.lean) · [Kani](contracts/kani/xpile_contract_frontend_trait.rs) |
| `xpile-contract-backend-trait-v1.yaml` | pattern | 3 / proof | ContractBackend + citation bridge via structured attrs | [Lean](contracts/lean/XpileContractBackendTrait.lean) · [Kani](contracts/kani/xpile_contract_backend_trait.rs) |
| `py-int-arith-v1.yaml` | kernel | 1 semantics / code | Python `int` arithmetic with bigint promotion | [Lean](contracts/lean/PyIntArith.lean) · [Kani](contracts/kani/py_int_arith.rs) |
| `bashrs-posix-idempotence-v1.yaml` | pattern | 1 semantics / code | POSIX shell idempotence, Python↔bashrs cross-domain | [Lean](contracts/lean/Bashrs.lean) · [Kani](contracts/kani/bashrs.rs) |
| `xlate-py-list-to-vec-v1.yaml` | kernel | 2 translation / code | Python list → Rust Vec, alias-preserving | [Lean](contracts/lean/XlatePyListToVec.lean) · [Kani](contracts/kani/xlate_py_list_to_vec.rs) |
| `xlate-lean-to-rust-v1.yaml` | kernel | 2 / code | All Lean 4 constructs (def, partial, inductive, instance, axiom, ...) → Rust | [Lean](contracts/lean/XlateLeanToRust.lean) · [Kani](contracts/kani/xlate_lean_to_rust.rs) |
| `xlate-rust-fn-to-lean-thm-v1.yaml` | kernel | 2 / proof | Rust fn + contract → Lean 4 theorem with `@[xpile_contract]` attr | [Lean](contracts/lean/XlateRustFnToLeanThm.lean) · [Kani](contracts/kani/xlate_rust_fn_to_lean_thm.rs) |
| `notation-latex-math-to-equation-v1.yaml` | kernel | 2 / proof | LaTeX math + theorem envs → contract equations | [Lean](contracts/lean/Notation.lean) · [Kani](contracts/kani/notation.rs) |
| `ffi-cpython-ext-v1.yaml` | pattern | 4 hybrid / code | CPython C-extension boundary semantics | [Lean](contracts/lean/FfiCpythonExt.lean) · [Kani](contracts/kani/ffi_cpython_ext.rs) |
| `compile-rust-to-ptx-mma-v1.yaml` | pattern | **5 compile / code** | PTX emission: `mma.sync`, `cp.async` pipelining, SMEM budget | [Lean](contracts/lean/CompileRustToPtxMma.lean) · [Kani](contracts/kani/compile_rust_to_ptx_mma.rs) |

`pv lint contracts/` → PASS, **0 errors and 0 warnings**. `xpile quorum` → 12 QUORUM, 0 PARTIAL, 0 UNVERIFIED. Every equation carries domain-grounded pre/postconditions; every equation is anchored to a Lean refinement theorem; every contract declares a `qa_gate`.

## Workspace (27 crates)

```
crates/
├── xpile/                           CLI binary
├── xpile-core/                      session orchestration + default_session()
├── xpile-agent/                     bounded agent loop (from alchemize)
├── xpile-oracle/                    Oracle trait — capture & compare execution
├── xpile-llm/                       model invocation + content-addressed cache
├── xpile-mcp/                       MCP server
├── xpile-contracts/                 re-export of provable-contracts (pv)
├── xpile-meta-hir/                  canonical IR (incl. Layer-B shell variants)
├── xpile-ffi-manifest/              cross-language boundary registry
├── xpile-bigint/                    BigInt promotion lane (slow path)
│
├── xpile-frontend/                  Frontend trait (code lane)
├── xpile-backend/                   Backend trait (code lane)
├── xpile-contract-frontend/         ContractFrontend trait (proof lane)
├── xpile-contract-backend/          ContractBackend trait (proof lane)
│
├── depyler-frontend/                Python   (.py, .pyi) — REAL parser
├── decy-frontend/                   C        (.c, .h)    — scaffold
├── ruchy-frontend/                  Ruchy    (.ruchy)    — scaffold
├── bashrs-frontend/                 Shell    (.sh)       — REAL parser (POSIX subset)
│
├── xpile-rust-codegen/              Rust    — REAL emission
├── xpile-ruchy-codegen/             Ruchy   — REAL emission
├── xpile-ptx-codegen/               PTX     — scaffold + Layer-5 contract
├── xpile-wgsl-codegen/              WGSL    — scaffold
├── xpile-lean-codegen/              Lean 4  — scaffold
├── bashrs-backend/                  Shell   — REAL emission (POSIX subset)
│
├── latex-contract-frontend/         LaTeX   — scaffold
├── xpile-lean-contract-backend/     Lean theorems — scaffold (attr citation)
└── xpile-latex-contract-backend/    LaTeX papers  — scaffold (macro citation)
```

`depyler` / `decy` / `ruchy` are also exposed as workspace **aliases** so the original `cargo install depyler` / `cargo install decy` / `cargo install ruchy` consumers keep working when the merge plan in [`sub/migration.md`](docs/specifications/sub/migration.md) lands.

## CI gates (live)

Every PR runs:

| Step | Command |
|---|---|
| Formatting | `cargo fmt --all -- --check` |
| Type check | `cargo check --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Provable contracts | `pv lint contracts/` (via `aprender-contracts-cli`) |
| Security advisories | `cargo deny check advisories` |
| Tests | `cargo test --workspace` (incl. e2e rustc round-trip and `every_kani_harness_discharges`) |
| Kani BMC | dedicated `kani` job runs `cargo kani` over all Kani harnesses in `contracts/kani/` |

Workflow: [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

## Family

| Repo | Role |
|---|---|
| `paiml/xpile` (this) | Polyglot transpile workbench |
| `paiml/aprender` | ML framework; source of `aprender-contracts` (`pv`) |
| `paiml/depyler` | Python→Rust transpiler — folds into xpile per [§19](docs/specifications/sub/migration.md) |
| `paiml/decy` | C→Rust transpiler — folds in |
| `paiml/ruchy` | Modern data science language; xpile's third frontend |
| `paiml/paiml-mcp-agent-toolkit` | `pmat` |
| `pymc-labs/alchemize` | Source of the four-tool agent loop pattern |

## Install

```bash
cargo install xpile
```

Requires Rust 1.93+. All 27 workspace crates are published on crates.io
at v0.1.0. For source-based installs and the optional dev tooling (`pv`,
`pmat`, `cargo kani`), see the
[book's Installation chapter](https://paiml.github.io/xpile/installation.html).

## Usage

```bash
$ xpile info                                  # list registered frontends/backends
$ xpile transpile factorial.py                # Python → Rust (default)
$ xpile transpile factorial.py --target ruchy # Python → Ruchy
$ xpile transpile factorial.py --target lean  # Python → Lean 4
$ xpile transpile script.sh --target shell    # POSIX shell round-trip
$ xpile diamond --contracts-dir ./contracts   # Diamond-tier coverage report
$ xpile quorum  --contracts-dir ./contracts   # 4-stratum quorum report
```

End-to-end tutorials and the full CLI reference live in the book:
**<https://paiml.github.io/xpile/>**.

## License

MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
