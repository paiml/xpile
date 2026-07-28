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
- all binary operators (with Python-floor semantics for `//` and `%`)
- all unary operators
- ternary expressions
- `if`/`elif`/`else` chains
- function calls including self-recursion

Each emitted Rust/Ruchy/Lean function carries a
`// xpile-contract: C-PY-INT-ARITH` citation for the arithmetic
contract.

For the full list, see the
[CHANGELOG `Python subset (live, runtime-verified)`](https://github.com/paiml/xpile/blob/main/CHANGELOG.md)
section — that's the canonical list to avoid duplication-and-drift.

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
