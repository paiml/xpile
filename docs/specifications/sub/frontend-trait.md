# Polyglot Frontend Trait

**Section 2 of [xpile-spec.md](../xpile-spec.md).**

## Definition

```rust
pub trait Frontend: Send + Sync {
    fn name(&self) -> &'static str;
    fn extensions(&self) -> &[&'static str];
    fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError>;
}
```

Three methods. The trait is intentionally narrow because everything else (agent, oracle, codegen, MCP, contracts) is shared.

## Invariants

Encoded in [`contracts/xpile-frontend-trait-v1.yaml`](../../../contracts/xpile-frontend-trait-v1.yaml):

| Invariant | What it asserts |
|---|---|
| `extension_ownership` | No two frontends declare the same extension |
| `parse_idempotency` | `hash(parse(p, s)) == hash(parse(p, s))` — no mutable state, canonical serialization |
| `source_lang_consistency` | Module's `source_lang` matches the producing frontend's declared language |
| `ffi_boundaries_are_outgoing_only` | Frontends record outgoing calls only; incoming reconciliation is the FFI manifest's job |

## Implementations at v0.1.0

| Crate | Type | Extensions (per `Frontend::extensions` + `matches_path`) | Status |
|---|---|---|---|
| `depyler-frontend` | `PythonFrontend` | `py`, `pyi` | **Real** — parses via `rustpython-parser 0.4`; subset in `CHANGELOG.md`; includes cross-domain `subprocess.run` → `Stmt::Cmd` lowering (PMAT-040) |
| `bashrs-frontend` | `BashrsFrontend` | `sh`, `bash`, `zsh`, `mk` + canonical filenames `Makefile`, `Dockerfile` via `matches_path` | **Real** — tokenizer handles realistic POSIX shell (quoting, $NAME / ${NAME}, $(cmd), backtick, NAME=value, pipelines, ShellLoop, special params $1-9/$@/$#, escape sequences, line continuation, redirections, short-circuit `&&`/`||`, test brackets, arith expansion `$((...))`, subshells). 54 tests across PMAT-039..058 + PMAT-085..092. |
| `decy-frontend` | `CFrontend` | `c`, `h` | Scaffold (returns empty Module) |
| `ruchy-frontend` | `RuchyFrontend` | `ruchy` | Scaffold (returns empty Module) |

Phase-2 parser integration plan for the still-stub frontends:

- `decy-frontend` will adopt clang / tree-sitter parsing + the existing decy HIR-lowering
- `ruchy-frontend` will depend on the `ruchy` crate from crates.io and reuse its parser + AST

The Python frontend's real implementation shipped in PR #6 MVP and grew through PRs #11/#12/#13/#15/#19/#20 and PMAT-002…PMAT-008 to cover the full v0.1.0 subset (all binary + unary ops including bitwise / power, multi-assignment if-branches, while loops with mutable rebinding, for-in-range with positive *or negative* literal steps, recursive function calls). Verified end-to-end by 11+ runtime-executed fixtures — see [`CHANGELOG.md`](../../../CHANGELOG.md) §"Python subset (live, runtime-verified)" for the canonical inventory.

The bashrs frontend's real implementation arrived in the PMAT-037..058 substrate-completion run alongside its sibling crate `bashrs-backend`. Both halves of the bashrs round-trip are tested by `bashrs_realistic_demo.sh` (PMAT-052) which exercises every Layer B IR variant byte-identically through frontend → meta-HIR → backend. The PMAT-085..092 polish run added 7 invariant lock-in tests covering POSIX param expansion, line continuation, redirections, short-circuit operators, test brackets, arithmetic expansion, and subshells. The trait determinism invariant for both frontends is covered by `C-XPILE-FRONTEND-TRAIT` at full §14.4 QUORUM (PMAT-062/063).

## Why object-safe

The trait uses `&dyn Frontend` in `xpile-core::TranspileSession::frontends` to allow dynamic dispatch by file extension. That requires:

- No associated types (so `parse_and_lower` returns the concrete `Module` type, not `Self::Hir`)
- All methods take `&self` (so the trait can be a trait object)
- `Send + Sync` (so sessions are usable across threads)

## Adding a new frontend

See [frontend-onboarding.md](frontend-onboarding.md). The seven steps are:

1. Add a variant to `xpile_meta_hir::SourceLang`
2. Create `crates/<lang>-frontend/`
3. Implement `Frontend`
4. Wire its parse/lower
5. Author a Layer-1 contract for one construct
6. Author a Layer-2 contract for that construct → Rust
7. Add a corpus regression test
