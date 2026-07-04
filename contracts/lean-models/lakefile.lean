import Lake
open Lake DSL

/-!
  `xpile` MODEL-PROOF lane — the deliberate, walled-off Mathlib exception
  (provable-model-as-code, PMAT-956).

  The core proof lane (`contracts/lean/`) is hermetic and Mathlib-FREE by policy:
  every module there elaborates under bare core Lean, so `lake build` runs in
  ~15s with no cache to fetch (PMAT-903..1146). Those proofs are cheap,
  per-construct structure-extensionality Diamonds.

  THIS lane proves a different KIND of thing: the OLS-minimiser UNIQUENESS
  certificate for a fitted model, which needs real analysis / linear algebra
  that would be a multi-month formalisation to re-derive from core primitives.
  So it — and ONLY it — depends on Mathlib. It is a SEPARATE Lake package with
  its own (identical, v4.15.0) toolchain, its own `lake-manifest.json`, and its
  own advisory CI job (`lean-models`), so the heavy Mathlib cache never touches
  the fast hermetic core lane. The certificate is a LEAF the emit path CITES; no
  codegen builds against Mathlib.

  POLICY: Mathlib is permitted ONLY here. The core lane stays hermetic by rule.
-/

-- Mathlib pinned to the release matching this lane's Lean toolchain (v4.15.0),
-- so the model lane and the core lane share a toolchain. `lake-manifest.json`
-- records the resolved revs; `lake exe cache get` fetches the prebuilt oleans.
require "leanprover-community" / "mathlib" @ git "v4.15.0"

package «xpileModels» where
  -- Same rigor as the core lane: a `sorry` (→ `sorryAx`) becomes an ERROR, so a
  -- green `lake build` is an un-fakeable "no holes" claim for the certificate.
  leanOptions := #[⟨`warningAsError, true⟩]

@[default_target]
lean_lib «Models» where
