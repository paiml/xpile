# Polyglot Backend Trait

**Section 5b of [xpile-spec.md](../xpile-spec.md).** Parallel to [frontend-trait.md](frontend-trait.md); refines [rust-codegen.md](rust-codegen.md) from "the Rust backend" into "one of several backends".

## Definition

```rust
pub trait Backend: Send + Sync {
    fn name(&self) -> &'static str;
    fn targets(&self) -> &[Target];
    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError>;
}
```

Three methods. The trait is intentionally narrow because everything else (agent, oracle, contracts, MCP, FFI manifest) is shared.

```rust
pub enum Target {
    Rust,        // xpile-rust-codegen
    Ruchy,       // xpile-ruchy-codegen
    Ptx,         // xpile-ptx-codegen
    Wgsl,        // xpile-wgsl-codegen
    Spirv,       // xpile-spirv-codegen
}

pub struct BackendConfig {
    pub target: Target,
    pub profile: Profile,           // see bidirectional-ruchy.md
    pub hardware: Option<HwProfile>, // e.g., sm_89 for PTX, webgpu_v1 for WGSL
}

pub struct Artifact {
    pub primary: String,            // emitted source (Rust, Ruchy, WGSL) or IR (PTX, SPIR-V text)
    pub sidecars: Vec<(String, Vec<u8>)>, // optional binaries, manifests, debug maps
    pub citations: Vec<ContractId>, // every emitted construct cites a Layer-2 or Layer-5 contract
}
```

## Invariants

Encoded in [`contracts/xpile-backend-trait-v1.yaml`](../../../contracts/xpile-backend-trait-v1.yaml) (to author next):

| Invariant | What it asserts |
|---|---|
| `target_ownership` | No two backends declare overlapping `Target` enum variants (each Target has exactly one Backend) |
| `lower_idempotency` | `hash(lower(m, c)) == hash(lower(m, c))` — deterministic emission, no mutable state, canonical artifact serialization |
| `target_consistency` | Every artifact emitted by `lower` parses/links under the declared `target` (Rust compiles; PTX assembles via `ptxas`; WGSL validates via `naga`) |
| `compile_contract_citation` | Every IR-level construct in `Artifact.primary` (mma.sync, cp.async, @workgroup_size, etc.) cites a Layer-5 compile contract by `ContractId` |

The fourth invariant is what makes Layer 5 load-bearing: a backend cannot emit a hardware instruction without a contract sanctioning it. This closes the gap that `xpile-spec.md` audit-design.md §4 flags as "Oracle blind spots" — UB and hardware misuse in emitted code now have a contract chain to point to.

## Implementations at v0.1.0 and planned

| Crate | Backend struct | Target | Status |
|---|---|---|---|
| `xpile-rust-codegen` | `RustBackend` | `Target::Rust` | **Real** (PR #6 MVP; expanded #11/#12/#13/#15/#19/#20/#21) |
| `xpile-ruchy-codegen` | `RuchyBackend` | `Target::Ruchy` | **Real** (PR #7); same Python subset emits `fun … -> T { … }` |
| `xpile-lean-codegen` | `LeanBackend` | `Target::Lean` | **Real** (PR #14); emits `def name (…) : T :=` with `Int.fdiv` / `Int.fmod` |
| `bashrs-backend` | `BashrsBackend` | `Target::Shell` | **Real** (PMAT-039 MVP; expanded across PMAT-039..058 + PMAT-085..092 polish); emits POSIX `sh` for every Layer B IR variant |
| `xpile-ptx-codegen` | `PtxBackend` | `Target::Ptx` | Scaffold + Layer-5 compile contract at full §14.4 QUORUM (PMAT-074/075) |
| `xpile-wgsl-codegen` | `WgslBackend` | `Target::Wgsl` | Scaffold |
| `xpile-spirv-codegen` | `SpirvBackend` | `Target::Spirv` | Not yet scaffolded |

Four backends are real and share a common construct surface. Same Python source through four different `--target` values produces four different language outputs (Rust / Ruchy / Lean for the code-lane fixtures; Shell via the cross-domain `subprocess.run` recognition path in PMAT-040). PTX / WGSL / SPIR-V remain scaffold; the Layer-5 compile contract for PTX (`contracts/compile-rust-to-ptx-mma-v1.yaml`) is at full §14.4 QUORUM with paired Lean theorem + Kani harness (PMAT-074/075), but the codegen body is not wired up to it yet — that's XPILE-COMPILE-PTX-RUNTIME-001 future work.

## Why object-safe

The trait uses `&dyn Backend` in `xpile-core::TranspileSession::backends` to allow dynamic dispatch by target. That requires:

- No associated types (so `lower` returns the concrete `Artifact`, not `Self::Output`)
- All methods take `&self` (so the trait can be a trait object)
- `Send + Sync` (so sessions are usable across threads)

Same constraints as `Frontend`. Both traits compose at the session level: a session picks one frontend per source file + one backend per target.

## Profile and the two-mHIR decision

`BackendConfig::profile` carries the asymmetric-direction marker from the `bidirectional-ruchy.md` sub-spec:

```rust
pub enum Profile {
    RustOut,    // meta-HIR normalized for Rust emission (default)
    RuchyOut,   // meta-HIR normalized for Ruchy emission (pipelines reconstructed)
}
```

The profile is selected by `xpile-core` based on the chosen backend and applied during a normalization pass *before* `lower()` runs. Backends do not branch on profile internally — they see meta-HIR already normalized for their target. This keeps each `Backend` implementation single-purpose.

## Hardware profile

`BackendConfig::hardware` is `None` for backends emitting target-independent source (Rust, Ruchy), `Some(...)` for backends whose emission depends on hardware capabilities (PTX, WGSL):

```rust
pub enum HwProfile {
    Ptx { compute_capability: String },  // e.g., "sm_89"
    Wgsl { features: Vec<String> },      // e.g., ["timestamp-query", "f16"]
    Spirv { version: (u32, u32) },       // e.g., (1, 6)
}
```

A Layer 5 compile contract pins the `HwProfile` range it applies to (see `compile_targets:` in [contract-taxonomy.md](contract-taxonomy.md)). At session start, `xpile-core` resolves the user-requested target + hardware against the contract corpus and selects the matching backend implementation path.

## Adding a new backend

Mirrors the [frontend-onboarding.md](frontend-onboarding.md) seven-step checklist:

1. Add a variant to `xpile_meta_hir::Target`
2. Create `crates/xpile-<target>-codegen/`
3. Implement `Backend`
4. Wire its `lower` over an example meta-HIR module
5. Author a Layer-5 (compile) contract for one emitted IR construct
6. Author the backend's architectural contract obligations (citation, idempotency)
7. Add a corpus regression test that round-trips an example meta-HIR module through the new backend and asserts the four invariants

A future sub-spec `backend-onboarding.md` will replace this section with the full procedure once the second backend lands.
