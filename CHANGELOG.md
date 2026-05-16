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
- **`for target in range(...)`** desugaring (PMAT-007). Supports
  `range(stop)`, `range(start, stop)`, and `range(start, stop, step)`
  where `step` is a positive integer literal. Lowers to a `Let` init
  + `While target < stop` + `target = target + step` tail. Non-range
  iterables and negative/non-literal steps still error with a clear
  message.

### Backends (real emission)

- Rust target: `pub fn name(...) -> T { ... }`
- Ruchy target: `fun name(...) -> T { ... }`
- Lean 4 target: `def name (...) : T := ...` (uses `Int.fdiv` /
  `Int.fmod` to preserve Python floor semantics)

Same Python source transpiles to all three via `xpile transpile <file.py> --target <t>`.

### Quality gates (on every PR via `.github/workflows/ci.yml`)

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `pv lint contracts/`
- `cargo deny check advisories`
- `cargo test --workspace`

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

30 e2e tests across `crates/xpile/tests/transpile_e2e.rs`; ~58
workspace tests total.

## [0.0.1] - 2026-05-15

Initial crates.io name-reservation release. Placeholder binary that
prints a banner pointing at the GitHub repo. The full v0.1.0+ binary
is tracked in this workspace.

Published: <https://crates.io/crates/xpile/0.0.1>.
