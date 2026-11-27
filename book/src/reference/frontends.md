# Frontends

> **Governing contract:** [`C-XPILE-FRONTEND-TRAIT`](contracts.md#c-xpile-frontend-trait)
> — Layer 3 (architectural), code lane, kind: pattern. Every frontend
> implements this trait. The invariants it pins: `extension_ownership`,
> `parse_idempotency`, `source_lang_consistency`,
> `ffi_boundaries_are_outgoing_only` (plus Gold/Platinum/Diamond
> refinements over the same records).

A frontend reads a source file and lowers it to xpile's canonical
**meta-HIR**. Frontends never see other frontends; they all funnel
through meta-HIR.

## Status

`xpile info` prints this table live from the registry the CLI actually
dispatches through — prefer it to this page.

The **Name** column is the key `xpile info` prints, not a display label.

| Frontend | Name | Extensions that LOWER | Routed → REFUSED | Status | Crate |
|---|---|---|---|---|---|
| Python | `python` | `.py`, `.pyi` | — | ✅ **Real parser** | `depyler-frontend` |
| C      | `c` | `.c`, `.h` | — | ✅ **Real parser** | `decy-frontend` |
| Shell  | `bashrs` | `.sh`, `.bash`, `.zsh` | `*.mk`, `Makefile`, `Dockerfile` | ✅ **Real POSIX parser** | `bashrs-frontend` |
| WASM   | `wasm` | `.wat` | — | ✅ **Real parser** (lossy lift) | `xpile-wasm-frontend` |
| Ruchy  | `ruchy` | — | `*.ruchy` | ⛔ **Routing only — refuses every input** | `ruchy-frontend` |

The two path columns are DIFFERENT claims and PMAT-1433 exists because this
table used to have only one. A frontend can be routed a path spelling and
still refuse it: `bashrs-frontend` is claimed for `*.mk`, `Makefile` and
`Dockerfile` so the refusal can name the dialect that is missing (PMAT-1420)
instead of degrading to a generic "no frontend handles `.mk`" — but it has no
Makefile dialect and no Dockerfile dialect, and every such input exits
non-zero. Until PMAT-1433 the Extensions column read `.sh, .bash, .zsh, .mk`
under status "Real POSIX parser", and `xpile info` printed the same four
extensions unannotated, because `Frontend::lowers_input()` is one boolean for
the whole frontend and `bashrs` earns it on `.sh`. Both columns are now
derived from the registry and checked by
`crates/xpile/tests/frontend_claim_disposition_witness.rs`, which drives every
claimed spelling through the frontend and asserts set equality in BOTH
directions — so implementing the Makefile dialect reds this page until the
row moves.

### "Lowers" is a claim about the GRAMMAR, not about the file format

A claimed extension means the registry routes that spelling to that frontend
and the frontend applies **its own grammar** to the bytes. It does **not**
mean the file format the extension normally denotes is supported — and for
`.pyi` and `.h` those are different things, because the canonical content of
each format is precisely what the grammar rejects:

<!-- XPILE-FORMATFORM-001:BEGIN -->
| spelling | a file in the format's CANONICAL form | why |
|---|---|---|
| `*.pyi` | ✅ REFUSES | a stub is bodiless (`def add(...) -> int: ...`); the frontend requires `return expr` |
| `*.h` | ✅ REFUSES | a header is an include guard plus prototypes; there is no preprocessor, and a prototype has no body |
<!-- XPILE-FORMATFORM-001:END -->

Both still appear under **Extensions that LOWER** above, and that is correct
rather than a contradiction: put a `.py`-shaped *definition* in a `.pyi`, or a
`.c`-shaped definition in a `.h`, and it lowers at exit 0.
`crates/xpile/tests/format_canonical_form_witness.rs`
(XPILE-FORMATFORM-001) measures both halves — the canonical form refuses AND
the definition form lowers at the same path — so the table above cannot drift
in either direction, and neither claim can be satisfied by a probe that was
simply malformed.

Through v0.1.617 nothing said this. `.pyi` and `.h` were published as
extensions that LOWER, with an empty **Routed → REFUSED** cell, on all three
surfaces — this table, `xpile info`, and the dispatch-failure message — and no
file in the canonical form of either format lowered at any of them
(PMAT-1442).

The disposition gate could not have caught it, and says so itself: its subject
is *"does the answer depend on the path spelling"*, so `PROBES` carries one
program per FRONTEND and writes those same bytes to every spelling that
frontend claims. PMAT-1433 generalised the **paths** a probe reaches and left
the **content** fixed; that is [[PMAT-1433]]'s own "one probe per subject
samples one of its N claims", one dimension over.

⚠️ `xpile info` still prints `- python (py, pyi)` and `- c (c, h)`
unannotated. Saying more there needs a per-INPUT granularity the
`refused_claims()` mechanism does not have — it is per-CLAIM — so this page
carries the measured table and `xpile info` does not. That is a disclosed gap,
not a fixed one.

The proof lane registers one contract frontend, LaTeX math
(`latex-contract-frontend`), which reads contract sources rather than
programs.

**Ruchy is registered but has no parser.** It exists so that a
`.ruchy` input gets a named refusal instead of a generic "no frontend
handles this extension" — `xpile transpile x.ruchy --target rust`
exits non-zero with a reason. It does *not* mean Ruchy input works;
Ruchy is a fully supported **output** target (see
[backends](backends.md)). Nothing here silently returns an empty
module: `crates/xpile/tests/claims_drift.rs` runs every registered
frontend against a real program in its own language and fails if one
answers with `Ok(Module { items: [] })`.

**There is no C++, Rust, or Lean 4 frontend.** This page claimed all
three as "planned"/"scaffold" workspace members; no such crate exists
and none is registered. Lean 4 and LaTeX appear in the *proof* lane and
Rust appears as a *backend*, which is where the confusion came from.

## Python frontend — what's supported

The depyler frontend is the deepest at v0.1.0. The supported subset
includes:

- typed `def` functions
- multi-statement function bodies
- ternary expressions
- `if`/`elif`/`else` chains
- function calls including self-recursion

An emitted function carries a `xpile-contract:` citation **only when its
body uses a construct some contract governs** — a minority of the
functions in a typical corpus, and by design.
`Function::applicable_contracts()` returns nothing for comparison-only,
logical-only, constant-only and call-only bodies, and the backends emit
one line per returned ID, so an empty list emits no line at all.
Measured: `def ident(a: int) -> int: return a` and a comparison-driven
`pick` emit **zero** citations from `--target rust`, `--target ruchy`
and `--target lean`, at exit 0. `xpile audit`'s F1 denominator excludes
exactly those functions (XPILE-FALSIFY-002), and
`crates/xpile/tests/citation_surface_witness.rs` (XPILE-CITESURFACE-001)
pins it.

When a citation *is* emitted, two things vary independently, and both
are measured by `crates/xpile/tests/citation_id_matrix_witness.rs`
(XPILE-CITEMATRIX-001) rather than asserted here:

**Which contract, by the function's type** — the same ID on every code
lane:

<!-- XPILE-CITEMATRIX-001:IDS:BEGIN -->
| Python type | contract cited |
|---|---|
| `int` | `C-PY-INT-ARITH` |
| `float` | `C-PY-FLOAT-ARITH` |
| `str` | `C-XLATE-PY-STR-TO-RUST-STRING` |
| `bool` | `C-XLATE-PY-BOOL-TO-RUST-BOOL` |
<!-- XPILE-CITEMATRIX-001:IDS:END -->

**Which comment form, by lane** — the same form for every type:

<!-- XPILE-CITEMATRIX-001:SYNTAX:BEGIN -->
| `--target` | citation form |
|---|---|
| `rust` | `// xpile-contract: <ID>` |
| `ruchy` | `// xpile-contract: <ID>` |
| `lean` | `/-- xpile-contract: <ID> -/` |
<!-- XPILE-CITEMATRIX-001:SYNTAX:END -->

That the ID is lane-independent and the form is type-independent is
itself checked, so a lane that started citing something different would
red rather than quietly disagree with the other two.

Through v0.1.617 this section said *"Each emitted Rust/Ruchy/Lean
function carries a `// xpile-contract: C-PY-INT-ARITH` citation for the
arithmetic contract"* — one sentence, false **three** times. The ID is
**type-directed**, so `C-PY-INT-ARITH` is right only for `int`
functions; `//` is not the Lean form, because PMAT-1405 changed that
lane to a `/-- … -/` docstring **deliberately** (a file `lean` must
actually parse cannot carry the old attribute) and this page went on
naming the Rust comment syntax for it; and the citation is not
universal at all. PMAT-1445 corrected the first two and, in replacing
the sentence, restated the third as *"Each emitted function carries a
`xpile-contract:` citation"*. PMAT-1447 removed it and gated the class
across every surface that states it.


For the full list, see the
[CHANGELOG `Python subset (live, runtime-verified)`](https://github.com/paiml/xpile/blob/main/CHANGELOG.md)
section — that's the canonical list to avoid duplication-and-drift.

### Operator surface

Until PMAT-1441 this section claimed the *whole* of Python's binary and
unary operator sets, without listing either. Both claims were false —
`@`, `is`, `is not` and unary `+` refuse — and the frontend's own refusal
message says so in the same breath (`unsupported binary operator:
MatMult — supported: + - * / // % & | ^ << >> **`). The canonical
CHANGELOG list linked above is enumerative and was correct; this page had
paraphrased it into a universal it never states.

The block below is DERIVED: one probe per Python operator, driven through
the live `PythonFrontend`, compared to this page by equality in
`crates/xpile/tests/frontend_operator_surface_witness.rs`
(XPILE-PYOPSURFACE-001). A `REFUSES` row is recorded only when the
frontend's own error names the operator *and* the same program with a
reference operator in that slot lowers — so a mis-typed probe reds as a
corpus bug instead of publishing a false refusal. Implementing one of
these reds this page until the row moves.

<!-- XPILE-PYOPSURFACE-001:BEGIN -->
```text
class     variant   probe       disposition
BinOp     Add       a + b       lowers
BinOp     Sub       a - b       lowers
BinOp     Mult      a * b       lowers
BinOp     MatMult   a @ b       REFUSES
BinOp     Div       a / b       lowers
BinOp     Mod       a % b       lowers
BinOp     Pow       a ** b      lowers
BinOp     LShift    a << b      lowers
BinOp     RShift    a >> b      lowers
BinOp     BitOr     a | b       lowers
BinOp     BitXor    a ^ b       lowers
BinOp     BitAnd    a & b       lowers
BinOp     FloorDiv  a // b      lowers
Compare   Eq        a == b      lowers
Compare   NotEq     a != b      lowers
Compare   Lt        a < b       lowers
Compare   LtE       a <= b      lowers
Compare   Gt        a > b       lowers
Compare   GtE       a >= b      lowers
Compare   Is        a is b      REFUSES
Compare   IsNot     a is not b  REFUSES
Compare   In        a in b      lowers
Compare   NotIn     a not in b  lowers
UnaryOp   USub      -a          lowers
UnaryOp   UAdd      +a          REFUSES
UnaryOp   Invert    ~a          lowers
UnaryOp   Not       not a       lowers

ast.BinOp: 13 in Python, 12 lower, 1 REFUSE
ast.Compare: 10 in Python, 8 lower, 2 REFUSE
ast.UnaryOp: 4 in Python, 3 lower, 1 REFUSE
```
<!-- XPILE-PYOPSURFACE-001:END -->

`in` / `not in` lower against a `list[...]` right operand (`.contains`),
not against a scalar. Chained comparisons (`0 < a < b`) desugar to the
`and` of adjacent pairs.

## Shell frontend — what's supported

The bashrs frontend parses a POSIX-shell subset sufficient for realistic
build scripts:

- shebang normalisation
- variable assignment + expansion
- conditional file creation (`mkdir -p`, `> file`, `>> file`)
- pipelines
- `if`/`elif`/`else`
- `for` loops over expansions
- subprocess invocation (`cmd arg1 arg2`)

The supported set is locked in by the
[`C-BASHRS-POSIX-IDEMPOTENCE`](contracts.md#c-bashrs-posix-idempotence)
contract. See the [shell-roundtrip tutorial](../tutorials/shell-roundtrip.md)
for an end-to-end example.

## Calling a frontend as a library

```rust
use depyler_frontend::PythonFrontend;
use xpile_frontend::Frontend;

let frontend = PythonFrontend;
let module = frontend.parse_and_lower(path, source)?;
// `module` is a `xpile_meta_hir::Module`
```

The `Frontend` trait surface is intentionally minimal — see
[Adding a frontend](../contributing/adding-a-frontend.md) for the full
implementation guide.
