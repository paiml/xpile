# Canonical Meta-HIR

**Section 3 of [xpile-spec.md](../xpile-spec.md).**

## v0.1.0 shape

```rust
pub struct Module {
    pub name: String,
    pub source_lang: SourceLang,
    pub items: Vec<Item>,
    pub ffi_boundaries: Vec<FfiBoundary>,
}

pub enum SourceLang {
    Python,
    C,
    Cpp,
    Cuda,
    Ruchy,
}

pub enum Item {
    Function(Function),
}

pub struct Function {
    pub name: String,
    pub signature: String,
}

pub struct FfiBoundary {
    pub from_lang: SourceLang,
    pub to_lang: SourceLang,
    pub symbol: String,
    pub signature: String,
}
```

Intentionally minimal. The federated philosophy is the design choice.

## Federated > unified

Each frontend keeps its own internal HIR — `depyler-hir`, `decy-hir`, the ruchy AST. Meta-HIR is the **coordination layer** that frontends produce and shared infrastructure consumes. It is NOT the type system.

Why:

1. **No good crystal ball.** We don't yet have hybrid demos to validate the right shape of a richer meta-IR. Over-designing now would lock in mistakes.
2. **Language-specific optimization stays local.** depyler-hir can carry Python-specific information (refcount approximations, generator state); decy-hir can carry C-specific information (alias graph, lifetime hints). Neither bleeds into meta-HIR.
3. **Migration path exists.** When a hybrid case demands cross-language type inference, expand meta-HIR to carry types, add `Frontend::infer_types`, and migrate frontends one at a time.

## Determinism requirement

Meta-HIR must serialize canonically (BTreeMap-ordered, no HashMap iteration). Reason: the cache key in [cache-determinism-provenance.md](cache-determinism-provenance.md) hashes serialized meta-HIR; non-deterministic hash inputs would break the determinism contract.

## Growth trajectory

| Trigger | Meta-HIR addition |
|---|---|
| First hybrid Python+C demo lands | Type carrier for FFI boundary inputs/outputs |
| Generators in scope | Coroutine-state representation |
| Async support | Future/Promise canonical form |
| CUDA frontend lands | Device-kernel-launch construct |

Each addition is a contract: `xpile-meta-hir-vN.yaml` versions the IR shape, and `pv diff` detects breaking changes.
