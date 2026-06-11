# Diamond-Tier Refinement Taxonomy

**Section 28 of [xpile-spec.md](../xpile-spec.md).** Catalogs every Diamond-tier algebraic category demonstrated across xpile's contract substrate.

> **⛔ DEPTH RATCHET FROZEN AT depth-13 (PMAT-465, 2026-06-11).** Do **not** run further depth-14+ UNIVERSAL broadening sweeps as default or background work. Per [xpile-spec.md §30 "Autonomous execution priority"](../xpile-spec.md#autonomous-execution-priority--ev-ranked-pmat-465-2026-06-11), that capacity is redirected to higher-EV work (dict lane, decy C frontend, and paying down the placeholder Runtime witnesses on the 10 contracts flagged in [`audit-design.md`](../audit-design.md) §4). Depth broadening resumes **only** when a new contract must reach the existing UNIVERSAL floor, or on explicit user request. The taxonomy below remains the authoritative record of what *has* been demonstrated through depth-13.

## What is a Diamond?

Per [ruchy 5.0 §14.10.5](../sub/provability-roadmap.md), the refinement-tier progression is:

| Tier | Captures | Example |
|---|---|---|
| **Bronze** | byte-array placeholders, `rfl`-by-construction | "modules array preserved at byte level" |
| **Silver** | typed structural model | "PyListSilver α preserved with element type" |
| **Gold** | refinement subtypes encoding preconditions | `NonEmptyHomogeneousList α := { l // l.elements ≠ [] }` |
| **Platinum** | single compositional algebraic property | commutativity, associativity, idempotence |
| **Diamond** | **COMBINED algebraic axiomatizations** | full monoid (closure + assoc + identity), full lattice (max + min + absorption), full equivalence relation (refl + symm + trans) |

A Diamond theorem combines multiple Platinum properties into a **single algebraic category** that captures a named mathematical structure as a 4-conjunction at the type level.

## Coverage milestones

As of v0.1.0+ (PMAT-214..442), the substrate has:

- **Diamond depth-1 UNIVERSAL** (12/12 contracts): every contract has at least one Diamond at one algebraic category.
- **Diamond depth-2 UNIVERSAL** (12/12 contracts): every contract has at least two **distinct** Diamond categories. CI-enforced via PMAT-251.
- **Diamond depth-3 UNIVERSAL** (12/12 contracts, post-PMAT-336): every contract has ≥3 distinct Diamond categories. CI-enforced via tightened gate. Achieved through PMAT-241..245 (Layer coverage) + PMAT-289 + PMAT-331..336 broadening sweep.
- **Diamond depth-4 UNIVERSAL** (12/12 contracts, post-PMAT-344): every contract has ≥4 distinct Diamond categories. Achieved through PMAT-247/248/288/329/330 (ALL 5 LAYERS milestone at PMAT-330) followed by 7-PR broadening sweep (PMAT-338..344) using five recurring algebraic templates.
- **Diamond depth-5 UNIVERSAL** (12/12 contracts, post-PMAT-354): every contract has ≥5 distinct Diamond categories. Achieved through PMAT-286/287/328 (initial L1/L4/L5 opens) followed by a 9-PR broadening sweep (PMAT-346..354), with **depth-5 ACROSS ALL 5 TAXONOMY LAYERS** intermediate milestone at PMAT-347 and **depth-5 UNIVERSAL** finale at PMAT-354. The broadening leaned heavily on the structure-extensionality template (PMAT-349, 352, 353, 354) and introduced the String.length Nat-structure as a sixth recurring template (PMAT-346, 350).
- **Diamond depth-6 UNIVERSAL** (12/12 contracts, post-PMAT-365): every contract has ≥6 distinct Diamond categories. Achieved through PMAT-290/291 (initial L1/L5 opens) followed by a 10-PR broadening sweep (PMAT-356..365), with **depth-6 ACROSS ALL 5 TAXONOMY LAYERS** intermediate milestone at PMAT-358 and **depth-6 UNIVERSAL** finale at PMAT-365. The wave was dominated by the structure-extensionality template (PMAT-356/359/360/361/362/363/364) — and closed Rust↔Lean Array.size invariant on both sides (PMAT-344 Rust, PMAT-365 Lean) and ContractFrontend↔ContractBackend inner/outer record extensionality on the trait pair (PMAT-353/354/361/364).
- **Diamond depth-7 UNIVERSAL** (12/12 contracts, post-PMAT-376): every contract has ≥7 distinct Diamond categories. Achieved through PMAT-292/293 (initial L1/L5 opens) followed by a 10-PR broadening sweep (PMAT-367..376), with **depth-7 ACROSS ALL 5 TAXONOMY LAYERS** intermediate milestone at PMAT-369 and **depth-7 UNIVERSAL** finale at PMAT-376. The wave continued the structure-extensionality template (PMAT-367/368/371/373/374) and Array.size template (PMAT-375/376), and **introduced the enum completeness template as a 7th recurring algebraic family** (PMAT-370 Target, PMAT-372 LatexDisplayKind).
- **Diamond depth-8 UNIVERSAL** (12/12 contracts, post-PMAT-387): every contract has ≥8 distinct Diamond categories. Achieved through PMAT-294/295 (initial L1/L5 opens) followed by a 10-PR broadening sweep (PMAT-378..387), with **depth-8 ACROSS ALL 5 TAXONOMY LAYERS** intermediate milestone at PMAT-380 and **depth-8 UNIVERSAL** finale at PMAT-387. The wave continued the structure-extensionality template (PMAT-378/381..385) and Array.size template (PMAT-386/387). Added Bronze-tier struct-ext demonstrations as a substrate-wide pattern (PMAT-379 Outcome.length on Bronze, PMAT-381 Artifact Bronze, PMAT-368 prior wave) and added a third instance of enum-completeness (PMAT-380 SourceLang).
- **Diamond depth-9 UNIVERSAL** (12/12 contracts, post-PMAT-398): every contract has ≥9 distinct Diamond categories. Achieved through PMAT-296/297 (initial L1/L5 opens) followed by a 10-PR broadening sweep (PMAT-389..398), with **depth-9 UNIVERSAL** finale at PMAT-398. The wave **introduced Template 9 (Gold-tier subtype-extensionality)** as a new recurring algebraic family, captured the Gold-tier refinement subtype on every contract: PMAT-389 (BorrowedRefManifestEntry struct-ext on FfiCpythonExt, transitional), PMAT-390 (SuccessfulOutcome on Bashrs), PMAT-391 (FrameSafeTransition on ContractFrontendTrait), PMAT-392 (ConsistentBackendInput on BackendTrait), PMAT-393 (ConsistentFrontendOutput on FrontendTrait), PMAT-394 (CitationCompleteContract on ContractBackendTrait), PMAT-395 (NonEmptyHomogeneousList α on PyListToVec — first polymorphic subtype-ext), PMAT-396 (WarningLineCount on XlateLeanToRust), PMAT-397 (NonEmptyPreconditionList on XlateRustFnToLeanThm), PMAT-398 (NonEmptyDefinition on Notation — finale). PMAT-311 was the prior solo subtype-ext (BoundedSmem), now joined by 9 more for a total of 10 substrate instances. Closes Frontend↔Backend Gold-tier subtype-ext symmetry pair (PMAT-392/393) and ContractFrontend↔ContractBackend Gold-tier subtype-ext pair (PMAT-391/394).
- **Diamond depth-10 UNIVERSAL** (12/12 contracts, post-PMAT-409): every contract has ≥10 distinct Diamond categories. Achieved through PMAT-300/301 (initial L1/L5 opens) followed by a 10-PR broadening sweep (PMAT-400..409), with **depth-10 UNIVERSAL** finale at PMAT-409. The wave **introduced Template 10 (Tier-projection homomorphism)** as a new recurring algebraic family — canonical forgetful Silver→Bronze maps defined on every contract's tiered model and proven structure-preserving. PMAT-400 (BoundedRefcountDelta subtype-ext on FfiCpythonExt, transitional Template 9), PMAT-401 (silver_to_bronze on Bashrs Outcome — Template 10 introduction), PMAT-402 (ArtifactSilver→Artifact), PMAT-403 (MetaHirModuleSilver→MetaHirModule), PMAT-404 (TranspileSession→Array EquationsBlock), PMAT-405 (RenderedDocSilver→RenderedDoc), PMAT-406 (HomogeneousListSilver α→PyListSilver α — second polymorphic projection), PMAT-407 (LeanDefSilver→LeanDef), PMAT-408 (RustFnSilver→RustFn), PMAT-409 (DefinitionEnvSilver→DefinitionEnv — finale). Closes Frontend↔Backend trait Silver→Bronze tier-projection pair (PMAT-402/403), ContractFrontend↔ContractBackend Silver→Bronze tier-projection pair (PMAT-404/405), and Rust↔Lean Silver→Bronze tier-projection pair (PMAT-407/408).
- **Diamond depth-11 UNIVERSAL** (12/12 contracts, post-PMAT-420): every contract has ≥11 distinct Diamond categories. Achieved through PMAT-302/303 (initial L1/L5 opens) followed by a 10-PR broadening sweep (PMAT-411..420), with **depth-11 UNIVERSAL** finale at PMAT-420. The wave **introduced Template 11 (Canonical identity element)** as a new recurring algebraic family — distinguished identity/zero elements on every contract's Silver/Gold tiered model with structural properties proven. PMAT-411 (balanced_refcount_delta on FfiCpythonExt — Template 11 introduction), PMAT-412 (empty_success_outcome on Bashrs), PMAT-413 (empty_rust_artifact on BackendTrait), PMAT-414 (empty_python_module on FrontendTrait — closes F↔B pair), PMAT-415 (empty_session on ContractFrontendTrait), PMAT-416 (empty_contract on ContractBackendTrait — closes CF↔CB pair), PMAT-417 (empty_py_list_silver α on PyListToVec — third polymorphic canonical), PMAT-418 (empty_lean_def_silver on XlateLeanToRust), PMAT-419 (empty_rust_fn_silver on XlateRustFnToLeanThm — closes Rust↔Lean pair), PMAT-420 (empty_definition_env_silver on Notation — finale).
- **Diamond depth-12 UNIVERSAL** (12/12 contracts, post-PMAT-431): every contract has ≥12 distinct Diamond categories. Achieved through PMAT-305/306 (initial L1/L5 opens) followed by a 10-PR broadening sweep (PMAT-422..431), with **depth-12 UNIVERSAL** finale at PMAT-431. The wave **introduced Template 12 (Bronze→Silver canonical-lift homomorphism)** as a new recurring algebraic family — inverse direction of Template 10 (Silver→Bronze projection). Define canonical lifts from Bronze types to Silver types with default values for the new Silver fields. PMAT-422 (FfiCall→FfiCallSilver — Template 12 introduction), PMAT-423 (Outcome→OutcomeSilver), PMAT-424 (Artifact→ArtifactSilver), PMAT-425 (MetaHirModule→MetaHirModuleSilver — closes F↔B pair), PMAT-426 (EquationsBlock→TranspileSession), PMAT-427 (RenderedDoc→RenderedDocSilver — closes CF↔CB pair), PMAT-428 (PyList→PyListSilver UInt8 — UInt8-specialized), PMAT-429 (LeanDef→LeanDefSilver), PMAT-430 (RustFn→RustFnSilver — closes Rust↔Lean pair), PMAT-431 (DefinitionEnv→DefinitionEnvSilver — finale).
- **Diamond depth-13 UNIVERSAL** (12/12 contracts, post-PMAT-442): every contract has ≥13 distinct Diamond categories. Achieved through PMAT-307/308 (initial L1/L5 opens) followed by a 10-PR broadening sweep (PMAT-433..442), with **depth-13 UNIVERSAL** finale at PMAT-442. The wave **introduced Template 13 (Bronze→Silver→Bronze round-trip identity)** as a new recurring algebraic family — proves the composition of Template 10 projection and Template 12 lift equals identity on Bronze type. Captures correctness relationship between Templates 10 and 12.
- **Diamond depth-7 ACROSS LAYERS** (2/12 contracts): PMAT-292 (order-distributive-lattice on L1) + PMAT-293 (bounded lattice with top+bottom on L5).
- **Diamond depth-8 ACROSS LAYERS** (2/12 contracts): PMAT-294 (divisibility-preorder on L1, FIRST relation-not-operation category) + PMAT-295 (cancellative monoid on L5).
- (Depth-9..13 ACROSS LAYERS milestones subsumed by depth-9..13 UNIVERSAL after PMAT-398/409/420/431/442. Historical anchors: PMAT-298/299 opened depth-9; PMAT-300/301 opened depth-10; PMAT-302/303 opened depth-11; PMAT-305/306 opened depth-12; PMAT-307/308 opened depth-13.)
- **Diamond depth-14 ACROSS LAYERS** (2/12 contracts): PMAT-310 (Nat-cast ring homomorphism on L1, FIRST EXTERNAL claim) + PMAT-311 (subtype extensionality on L5, FIRST SUBTYPE-STRUCTURE claim).
- **Diamond depth-15 ACROSS LAYERS** (2/12 contracts): PMAT-312 (Int-emod quotient ring homomorphism on L1, FIRST QUOTIENT-RING claim) + PMAT-313 (Nat-mod quotient ring homomorphism on L5).
- **Diamond depth-16 ACROSS LAYERS** (2/12 contracts): PMAT-315 (Int gcd-monoid + Bézout / PID on L1, FIRST UNIVERSAL-OBJECT-WITH-CONSTRUCTIVE-WITNESS claim) + PMAT-316 (Nat gcd-monoid on L5).
- **Diamond depth-17 ACROSS LAYERS** (2/12 contracts): PMAT-317 (unit group `{1, -1} ≅ Z/2Z` on L1, FIRST UNIT-GROUP claim) + PMAT-318 (Nat power-monoid on L5).
- **Diamond depth-18 ACROSS LAYERS** (2/12 contracts): PMAT-320 (sign function monoid hom `Int → {-1,0,1}` on L1, third piece of sign×magnitude decomposition) + PMAT-321 (Nat integral domain on L5).
- **Diamond depth-19 ACROSS LAYERS** (2/12 contracts): PMAT-322 (negation-order compatibility / OrderedAddCommGroup on L1) + PMAT-323 (Nat truncated subtraction on L5).
- **Diamond depth-20 ACROSS LAYERS** (2/12 contracts): PMAT-325 (Int.toNat partial inverse on L1) + PMAT-326 (Nat power monotonicity on L5).
- **Diamond depth-21** (1/12 contracts, DEEPEST): PMAT-327 (Nat-cast order embedding on PyIntArith L1, captures Mathlib's `OrderRingHom Nat Int` shape together with PMAT-310).

**Substrate total: 171 wired Diamond equations across 12 contracts.**

## Recurring algebraic templates (substrate-wide)

Thirteen recurring algebraic templates emerged during the depth-3..13 broadening sweeps. Each is mechanically applicable to specific record/subtype/enum patterns, enabling the **depth-3..13 UNIVERSAL** milestones (PMAT-336/344/354/365/376/387/398/409/420/431/442):

### Template 1: Structure-extensionality

Demonstrated on **32 distinct record/subtype contracts** (PMAT-311 + PMAT-329..336 + PMAT-349, 352..354, 356, 359..364, 367, 368, 371, 373, 374, 378, 381..385):

| PMAT | Contract | Target type |
|---|---|---|
| 311 | C-COMPILE-RUST-TO-PTX-MMA | BoundedSmem (subtype) |
| 329 | C-BASHRS-POSIX-IDEMPOTENCE | OutcomeSilver (record) |
| 330 | C-XPILE-FRONTEND-TRAIT | MetaHirModuleSilver (record) |
| 331 | C-XPILE-BACKEND-TRAIT | ArtifactSilver (record) |
| 332 | C-XPILE-CONTRACT-FRONTEND-TRAIT | TranspileSession (record, outer) |
| 333 | C-XPILE-CONTRACT-BACKEND-TRAIT | Contract (record, outer) |
| 334 | C-NOTATION-LATEX-MATH-TO-EQUATION | EquationFormulaSilver (record) |
| 335 | C-XLATE-LEAN-TO-RUST | RustFn (record) |
| 336 | C-XLATE-RUST-FN-TO-LEAN-THM | RustFnSilver (record, Rust side) |
| 349 | C-XLATE-PY-LIST-TO-VEC | PyListSilver α (record, polymorphic) |
| 352 | C-XLATE-RUST-FN-TO-LEAN-THM | LeanDefSilver (record, Lean side — closes Rust↔Lean struct pair) |
| 353 | C-XPILE-CONTRACT-FRONTEND-TRAIT | EquationsBlock (record, inner equations side) |
| 354 | C-XPILE-CONTRACT-BACKEND-TRAIT | ContractId (record, inner) |
| 356 | C-FFI-CPYTHON-EXT | FfiCallSilver (record, L4) |
| 359 | C-XPILE-BACKEND-TRAIT | Backend (record, INPUT side, single-field) |
| 360 | C-XLATE-PY-LIST-TO-VEC | TypedRustVecSilver α (record, Rust Vec side — closes Python↔Rust struct pair) |
| 361 | C-XPILE-CONTRACT-FRONTEND-TRAIT | MetaHirModule (record, inner modules side — closes inner-record pair with PMAT-353) |
| 362 | C-NOTATION-LATEX-MATH-TO-EQUATION | LatexCitationSilver (record, CITATION record) |
| 363 | C-XLATE-LEAN-TO-RUST | RustItemWithCitationSilver (record, 3-field) |
| 364 | C-XPILE-CONTRACT-BACKEND-TRAIT | RenderedDocSilver (record) |
| 367 | C-FFI-CPYTHON-EXT | FfiManifestEntryStructuredSilver (record, 6-field manifest) |
| 368 | C-BASHRS-POSIX-IDEMPOTENCE | Outcome (Bronze, single-field) |
| 369 | C-XPILE-FRONTEND-TRAIT | Frontend (record, INPUT side — closes Frontend↔Backend trait input-record pair with PMAT-359) |
| 371 | C-XLATE-PY-LIST-TO-VEC | HeterogeneousListSilver (rejection record) |
| 373 | C-XLATE-LEAN-TO-RUST | LeanInductive (record, Lean INPUT) |
| 374 | C-XLATE-RUST-FN-TO-LEAN-THM | ContractObligationSilver (record, 3-field input) |
| 378 | C-FFI-CPYTHON-EXT | BorrowedRef (record, ownership Bronze) |
| 381 | C-XPILE-BACKEND-TRAIT | Artifact (Bronze, single-field — Bronze-tier struct-ext) |
| 382 | C-XLATE-PY-LIST-TO-VEC | HomogeneousListSilver (witness record, single-field) |
| 383 | C-NOTATION-LATEX-MATH-TO-EQUATION | LeanTheoremEnvSilver (record, env-name + body) |
| 384 | C-XLATE-LEAN-TO-RUST | RustEnum (record, Rust OUTPUT — closes Lean↔Rust enum struct pair with PMAT-373) |
| 385 | C-XLATE-RUST-FN-TO-LEAN-THM | EmittedLeanTheoremSilver (record, 3-field output — closes Input/Output extensionality pair with PMAT-374) |

Each Diamond combines: field-equality → record-equality (Subtype.ext / record extensionality), record-equality → field-equality (congruence), decidable equality, self-equality.

**Cross-substrate symmetry closures** (depth-5/depth-6 broadening sweep):
- **Rust↔Lean struct pair**: PMAT-336 captured RustFnSilver (Rust side struct), PMAT-352 captured LeanDefSilver (Lean side struct).
- **Rust↔Lean Array.size pair**: PMAT-344 captured RustFnSilver.body/name Array.size, PMAT-365 captured LeanDefSilver.body/name Array.size.
- **Python↔Rust translation pair**: PMAT-349 captured PyListSilver α (Python input), PMAT-360 captured TypedRustVecSilver α (Rust Vec output).
- **ContractFrontend↔ContractBackend trait pair**:
  - Outer record: PMAT-332 (TranspileSession) + PMAT-333 (Contract).
  - Inner record: PMAT-353 (EquationsBlock equations side) + PMAT-354 (ContractId).
  - Plus PMAT-361 (MetaHirModule modules side) + PMAT-364 (RenderedDocSilver).
- **Frontend↔Backend trait pair input records**: PMAT-330 (MetaHirModuleSilver) + PMAT-359 (Backend INPUT record).
- **Lean↔Rust inductive/enum pair**: PMAT-373 (LeanInductive Lean INPUT) + PMAT-384 (RustEnum Rust OUTPUT — closes the inductive/enum struct pair on XlateLeanToRust).
- **Input/Output struct-ext pair on XlateRustFnToLeanThm**: PMAT-374 (ContractObligationSilver INPUT) + PMAT-385 (EmittedLeanTheoremSilver OUTPUT — closes the input/output extensionality pair across the translation contract).
- **Bronze-tier struct-ext emergence (depth-8 broadening wave PMAT-378..387)**: substrate began carrying struct-extensionality on Bronze record sub-types (PMAT-368 Outcome Bronze, PMAT-381 Artifact Bronze) — the same template the substrate uses on Silver/Gold sub-records now also holds on Bronze tier representations, demonstrating tier-independence of the algebraic structure.

### Template 2: Array.size structure

Demonstrated on **11 contracts** (PMAT-340/341/344/348/351/358/365/375/376/386/387) for `Array.size` axioms on record fields:

| PMAT | Contract | Field |
|---|---|---|
| 340 | C-XPILE-CONTRACT-FRONTEND-TRAIT | TranspileSession.modules/equations |
| 341 | C-XPILE-CONTRACT-BACKEND-TRAIT | Contract.depends_on/references |
| 344 | C-XLATE-RUST-FN-TO-LEAN-THM | RustFnSilver.body/name (Rust side) |
| 348 | C-XPILE-BACKEND-TRAIT | ArtifactSilver.bytes |
| 351 | C-XLATE-LEAN-TO-RUST | RustFn.body |
| 358 | C-XPILE-FRONTEND-TRAIT | MetaHirModuleSilver.bytes |
| 365 | C-XLATE-RUST-FN-TO-LEAN-THM | LeanDefSilver.body/name (Lean side — closes Rust↔Lean Array.size pair with PMAT-344) |
| 375 | C-XPILE-CONTRACT-FRONTEND-TRAIT | EquationsBlock.bytes (inner record) |
| 376 | C-XPILE-CONTRACT-BACKEND-TRAIT | ContractId.bytes (inner record — closes ContractFrontend↔ContractBackend Array.size pair with PMAT-375) |
| 386 | C-XPILE-CONTRACT-FRONTEND-TRAIT | MetaHirModule.bytes (inner record, modules side — closes inner-record Array.size symmetry with PMAT-387) |
| 387 | C-XPILE-CONTRACT-BACKEND-TRAIT | RenderedDocSilver.bytes (inner record — closes final inner-record Array.size symmetry pair on ContractFrontend↔ContractBackend trait) |

Each Diamond combines: size non-negativity, empty-record size-0 case, field-replacement preservation, field independence (or reflexivity for single-field records).

### Template 3: Enum distinctness

Demonstrated on 3 contracts (PMAT-339/342/347) for inductive enum types:

| PMAT | Contract | Enum |
|---|---|---|
| 339 | C-XPILE-BACKEND-TRAIT | Target (rust/ruchy/lean/ptx/wgsl/spirv/shell) |
| 342 | C-NOTATION-LATEX-MATH-TO-EQUATION | LatexDisplayKind (displayMath/equation/align) |
| 347 | C-XPILE-FRONTEND-TRAIT | SourceLang (python/c/rust/ruchy/shell/lean) |

Each Diamond combines: pairwise constructor distinctness (proved by `decide`), self-equality, decidable equality.

### Template 4: Nat structure

Demonstrated on PMAT-343 for `Nat`-valued fields:

| PMAT | Contract | Field |
|---|---|---|
| 343 | C-XLATE-LEAN-TO-RUST | LeanInductive.variant_count, RustEnum.variant_count |

Combines: non-negativity (trivially for Nat), successor strict-ordering (proved by `omega`).

### Template 5: Reverse involution

Demonstrated on PMAT-338 for `List`-valued fields:

| PMAT | Contract | Field |
|---|---|---|
| 338 | C-XLATE-PY-LIST-TO-VEC | PyListSilver.elems (List α) |

Combines: double-reverse identity, length preservation, empty-list reverse, singleton reverse.

### Template 6: String.length Nat-structure

Demonstrated on 3 contracts (PMAT-346/350/379) for `String.length` Nat-measure invariants on text-valued record fields:

| PMAT | Contract | Field |
|---|---|---|
| 346 | C-BASHRS-POSIX-IDEMPOTENCE | OutcomeSilver.observable (String) |
| 350 | C-NOTATION-LATEX-MATH-TO-EQUATION | EquationFormulaSilver.ascii_normalised (String) |
| 379 | C-BASHRS-POSIX-IDEMPOTENCE | Outcome.observable (Bronze — closes Silver/Bronze String.length pair on Bashrs with PMAT-346) |

Combines: length non-negativity (trivially for Nat), empty-string length-0, field-replacement preservation, other-field independence. Complements Template 2 (Array.size) — both Nat-measure invariants on container fields, targeting String vs. Array containers respectively.

### Template 7: Int-sign decomposition

Demonstrated on 2 contracts (PMAT-328/357) for sign-trichotomy + absolute-value invariants on Int-valued fields with semantic dichotomy:

| PMAT | Contract | Field |
|---|---|---|
| 328 | C-FFI-CPYTHON-EXT | FfiCallSilver.refcount_delta (Int — balanced/leaked/over-decref) |
| 357 | C-BASHRS-POSIX-IDEMPOTENCE | OutcomeSilver.exit_code (Int — success/failure) |

Combines: sign trichotomy (0 < x ∨ x = 0 ∨ x < 0), absolute-value non-negativity, zero-value identity, reflexivity. Cross-substrate parallel — both contracts host Int fields with semantic success/failure dichotomies that the sign-structure axiomatizes.

### Template 8: Enum completeness

Demonstrated on 3 contracts (PMAT-370/372/380) for total-coverage axiomatization of finite enum types:

| PMAT | Contract | Enum |
|---|---|---|
| 370 | C-XPILE-BACKEND-TRAIT | Target (7 variants — rust/ruchy/lean/ptx/wgsl/spirv/shell) |
| 372 | C-NOTATION-LATEX-MATH-TO-EQUATION | LatexDisplayKind (3 variants — displayMath/equation/align) |
| 380 | C-XPILE-FRONTEND-TRAIT | SourceLang (6 variants — python/c/rust/ruchy/shell/lean) |

Combines: total coverage (every value matches one of N known variants), self-equality, decidable membership, constructor distinctness sample. Complement to Template 3 (enum distinctness) — together they give the full finite-enumeration axiomatization. Introduced during the depth-7 broadening sweep (PMAT-367..376) and extended to a 3rd instance during the depth-8 broadening sweep (PMAT-380), giving full coverage of the three Layer-2 enum families (Frontend SourceLang ↔ Backend Target ↔ Notation LatexDisplayKind).

### Template 9: Gold-tier subtype extensionality

Demonstrated on **10 contracts** (PMAT-311 + PMAT-390/391/392/393/394/395/396/397/398) for **Gold-tier refinement subtypes** — `Subtype` values carrying a Prop-level witness, where the subtype satisfies `Subtype.ext` (val-equality lifts to subtype-equality). Introduced during the depth-9 broadening sweep:

| PMAT | Contract | Refinement subtype |
|---|---|---|
| 311 | C-COMPILE-RUST-TO-PTX-MMA | BoundedSmem := { s : Nat // s ≤ smem_budget_sm80 } (FIRST substrate subtype-ext, prior wave) |
| 390 | C-BASHRS-POSIX-IDEMPOTENCE | SuccessfulOutcome := { o : OutcomeSilver // o.exit_code = 0 } |
| 391 | C-XPILE-CONTRACT-FRONTEND-TRAIT | FrameSafeTransition := { p : TranspileSession × TranspileSession // p.fst.modules = p.snd.modules } |
| 392 | C-XPILE-BACKEND-TRAIT | ConsistentBackendInput := { p : Backend × ArtifactSilver // p.snd.target = p.fst.declared_target } (closes Frontend↔Backend pair with PMAT-393) |
| 393 | C-XPILE-FRONTEND-TRAIT | ConsistentFrontendOutput := { p : Frontend × MetaHirModuleSilver // p.snd.source_lang = p.fst.declared_lang } (closes Frontend↔Backend pair with PMAT-392) |
| 394 | C-XPILE-CONTRACT-BACKEND-TRAIT | CitationCompleteContract := { p : Contract × RenderedDocSilver // p.snd.citations = p.fst.depends_on ++ p.fst.references } (closes ContractFrontend↔ContractBackend pair with PMAT-391) |
| 395 | C-XLATE-PY-LIST-TO-VEC | NonEmptyHomogeneousList α := { l : HomogeneousListSilver α // l.elements ≠ [] } (FIRST polymorphic subtype-ext) |
| 396 | C-XLATE-LEAN-TO-RUST | WarningLineCount := { n : Nat // n ≥ warning_lines_floor } (mirror of PMAT-311 — bounded-Nat with dual-direction predicates) |
| 397 | C-XLATE-RUST-FN-TO-LEAN-THM | NonEmptyPreconditionList := { pl : PreconditionListSilver // pl.source_indices.size > 0 } |
| 398 | C-NOTATION-LATEX-MATH-TO-EQUATION | NonEmptyDefinition := { d : DefinitionEnvSilver // d.all_math_spans.size > 0 } |

Each Diamond combines: val-equality → subtype-equality (Subtype.ext), subtype-equality → val-equality (congruence), decidable equality (when val carrier has DecidableEq), self-equality. Polymorphic variants (PMAT-395, PMAT-397) use the 3-way form without decidable equality.

**Cross-substrate symmetry closures from the depth-9 wave:**
- **Frontend↔Backend Gold-tier subtype-ext pair**: PMAT-392 (ConsistentBackendInput) + PMAT-393 (ConsistentFrontendOutput) — both subtypes lift `Trait × OutputRecord` via a cross-field consistency witness.
- **ContractFrontend↔ContractBackend Gold-tier subtype-ext pair**: PMAT-391 (FrameSafeTransition, frame-preservation) + PMAT-394 (CitationCompleteContract, citation-completeness).
- **Bashrs Silver/Bronze + Gold subtype tier-emergence**: PMAT-329 (OutcomeSilver struct-ext) + PMAT-368 (Outcome Bronze) + PMAT-390 (SuccessfulOutcome Gold subtype) — captures the Bronze/Silver/Gold tier progression on the same contract.

### Template 10: Tier-projection homomorphism

Demonstrated on **9 contracts** (PMAT-401/402/403/404/405/406/407/408/409) for **canonical Silver→Bronze forgetful maps** — define a structure-preserving projection that drops fields added at Silver tier and prove the four projection axioms: field preserved, projection is independent of dropped fields (forgetful), preserves empty/identity element, reflexivity. Introduced during the depth-10 broadening sweep:

| PMAT | Contract | Forgetful map |
|---|---|---|
| 401 | C-BASHRS-POSIX-IDEMPOTENCE | silver_to_bronze : OutcomeSilver → Outcome (drops exit_code — Template 10 introduction) |
| 402 | C-XPILE-BACKEND-TRAIT | artifact_silver_to_bronze : ArtifactSilver → Artifact (drops target) |
| 403 | C-XPILE-FRONTEND-TRAIT | metahir_module_silver_to_bronze : MetaHirModuleSilver → MetaHirModule (drops source_lang — closes Frontend↔Backend pair with PMAT-402) |
| 404 | C-XPILE-CONTRACT-FRONTEND-TRAIT | session_to_equations_view : TranspileSession → Array EquationsBlock (drops modules — proof-lane projection) |
| 405 | C-XPILE-CONTRACT-BACKEND-TRAIT | rendered_doc_silver_to_bronze : RenderedDocSilver → RenderedDoc (drops citations — closes CF↔CB pair with PMAT-404) |
| 406 | C-XLATE-PY-LIST-TO-VEC | homogeneous_to_simple_list : HomogeneousListSilver α → PyListSilver α (drops element_type_tag — polymorphic) |
| 407 | C-XLATE-LEAN-TO-RUST | lean_def_silver_to_bronze : LeanDefSilver → LeanDef (drops name/args/return_type) |
| 408 | C-XLATE-RUST-FN-TO-LEAN-THM | rust_fn_silver_to_bronze : RustFnSilver → RustFn (drops name/generics/args/return_type — closes Rust↔Lean pair with PMAT-407) |
| 409 | C-NOTATION-LATEX-MATH-TO-EQUATION | definition_env_silver_to_bronze : DefinitionEnvSilver → DefinitionEnv (drops all_math_spans/label — depth-10 UNIVERSAL finale) |

Each Diamond combines: (a) primary field preserved by projection, (b) projection is independent of dropped fields (forgetful), (c) empty/identity input maps to empty/identity output, (d) reflexivity.

**Cross-substrate symmetry closures from the depth-10 wave:**
- **Frontend↔Backend Silver→Bronze tier-projection pair**: PMAT-402 (ArtifactSilver→Artifact) + PMAT-403 (MetaHirModuleSilver→MetaHirModule).
- **ContractFrontend↔ContractBackend Silver→Bronze tier-projection pair**: PMAT-404 (TranspileSession proof-lane view) + PMAT-405 (RenderedDocSilver→RenderedDoc).
- **Rust↔Lean Silver→Bronze tier-projection pair**: PMAT-407 (LeanDefSilver→LeanDef) + PMAT-408 (RustFnSilver→RustFn).
- **Polymorphic tier-projection**: PMAT-406 (HomogeneousListSilver α → PyListSilver α) — second polymorphic Template (after PMAT-395 Template 9).

### Template 11: Canonical identity element

Demonstrated on **10 contracts** (PMAT-411/412/413/414/415/416/417/418/419/420) for **distinguished identity/zero elements** within Silver/Gold subtypes — defines a canonical "empty" or "zero" value and proves its structural properties. Introduced during the depth-11 broadening sweep:

| PMAT | Contract | Canonical element |
|---|---|---|
| 411 | C-FFI-CPYTHON-EXT | balanced_refcount_delta : BoundedRefcountDelta (val=0 — Template 11 introduction) |
| 412 | C-BASHRS-POSIX-IDEMPOTENCE | empty_success_outcome : OutcomeSilver (observable="", exit_code=0) |
| 413 | C-XPILE-BACKEND-TRAIT | empty_rust_artifact : ArtifactSilver (bytes=#[], target=Target.rust) |
| 414 | C-XPILE-FRONTEND-TRAIT | empty_python_module : MetaHirModuleSilver (bytes=#[], source_lang=python — closes F↔B pair with PMAT-413) |
| 415 | C-XPILE-CONTRACT-FRONTEND-TRAIT | empty_session : TranspileSession (both arrays empty) |
| 416 | C-XPILE-CONTRACT-BACKEND-TRAIT | empty_contract : Contract (no dependencies, no references — closes CF↔CB pair with PMAT-415) |
| 417 | C-XLATE-PY-LIST-TO-VEC | empty_py_list_silver α : PyListSilver α (polymorphic — third polymorphic canonical) |
| 418 | C-XLATE-LEAN-TO-RUST | empty_lean_def_silver : LeanDefSilver (all 4 fields empty) |
| 419 | C-XLATE-RUST-FN-TO-LEAN-THM | empty_rust_fn_silver : RustFnSilver (all 5 fields empty — closes Rust↔Lean pair with PMAT-418) |
| 420 | C-NOTATION-LATEX-MATH-TO-EQUATION | empty_definition_env_silver : DefinitionEnvSilver (first_math_span="", all_math_spans=#[], label=none — depth-11 UNIVERSAL finale) |

Each Diamond combines: (a) primary field has canonical zero/empty value, (b) auxiliary field has canonical zero/empty value, (c) size/length of canonical field is 0, (d) reflexivity.

**Cross-substrate symmetry closures from the depth-11 wave:**
- **Frontend↔Backend trait canonical-element pair**: PMAT-413 (empty_rust_artifact) + PMAT-414 (empty_python_module).
- **ContractFrontend↔ContractBackend trait canonical-element pair**: PMAT-415 (empty_session) + PMAT-416 (empty_contract).
- **Rust↔Lean canonical-element pair**: PMAT-418 (empty_lean_def_silver) + PMAT-419 (empty_rust_fn_silver).
- **Third polymorphic substrate Template instance**: PMAT-417 (empty_py_list_silver α).

### Template 12: Bronze→Silver canonical-lift homomorphism

Demonstrated on **10 contracts** (PMAT-422..431) for **canonical Bronze→Silver lifts** — inverse direction of Template 10 (Silver→Bronze projection). Define a function from Bronze type to Silver type that preserves Bronze fields and sets default values for the new Silver fields. Introduced during the depth-12 broadening sweep:

| PMAT | Contract | Bronze→Silver lift |
|---|---|---|
| 422 | C-FFI-CPYTHON-EXT | lift_ffi_call_bronze_to_silver : FfiCall → FfiCallSilver (Template 12 introduction) |
| 423 | C-BASHRS-POSIX-IDEMPOTENCE | bronze_to_silver : Outcome → OutcomeSilver (default exit_code=0) |
| 424 | C-XPILE-BACKEND-TRAIT | artifact_bronze_to_silver : Artifact → ArtifactSilver (default Target.rust) |
| 425 | C-XPILE-FRONTEND-TRAIT | metahir_module_bronze_to_silver : MetaHirModule → MetaHirModuleSilver (closes F↔B pair with PMAT-424) |
| 426 | C-XPILE-CONTRACT-FRONTEND-TRAIT | equations_block_to_session : EquationsBlock → TranspileSession (wraps as singleton) |
| 427 | C-XPILE-CONTRACT-BACKEND-TRAIT | rendered_doc_bronze_to_silver : RenderedDoc → RenderedDocSilver (closes CF↔CB pair with PMAT-426) |
| 428 | C-XLATE-PY-LIST-TO-VEC | py_list_bronze_to_silver_u8 : PyList → PyListSilver UInt8 (UInt8-specialized) |
| 429 | C-XLATE-LEAN-TO-RUST | lean_def_bronze_to_silver : LeanDef → LeanDefSilver |
| 430 | C-XLATE-RUST-FN-TO-LEAN-THM | rust_fn_bronze_to_silver : RustFn → RustFnSilver (closes Rust↔Lean pair with PMAT-429) |
| 431 | C-NOTATION-LATEX-MATH-TO-EQUATION | definition_env_bronze_to_silver : DefinitionEnv → DefinitionEnvSilver (finale) |

Each Diamond combines: (a) primary field preserved by lift, (b) auxiliary field has default value, (c) empty/identity Bronze maps to empty/identity Silver, (d) reflexivity.

**Cross-substrate symmetry closures from the depth-12 wave:**
- **Frontend↔Backend Bronze→Silver lift pair**: PMAT-424 (Artifact lift) + PMAT-425 (MetaHirModule lift).
- **ContractFrontend↔ContractBackend Bronze→Silver lift pair**: PMAT-426 (EquationsBlock→TranspileSession) + PMAT-427 (RenderedDoc lift).
- **Rust↔Lean Bronze→Silver lift pair**: PMAT-429 (LeanDef lift) + PMAT-430 (RustFn lift).
- **UInt8-specialized lift**: PMAT-428 (PyList→PyListSilver UInt8) — fourth concrete-instance Template (alongside PMAT-395/406/417 polymorphic Templates).

### Template 13: Bronze→Silver→Bronze round-trip identity

Demonstrated on **10 contracts** (PMAT-433..442) for **round-trip composition correctness** — proves that the composition of Template 10 projection (Silver→Bronze) and Template 12 lift (Bronze→Silver) equals identity on the Bronze type. Captures the correctness relationship between the two directional homomorphisms. Introduced during the depth-13 broadening sweep:

| PMAT | Contract | Round-trip composition |
|---|---|---|
| 433 | C-FFI-CPYTHON-EXT | silver_to_bronze ∘ lift_bronze_to_silver = id on FfiCall (Template 13 introduction) |
| 434 | C-BASHRS-POSIX-IDEMPOTENCE | silver_to_bronze ∘ bronze_to_silver = id on Outcome |
| 435 | C-XPILE-BACKEND-TRAIT | artifact round-trip |
| 436 | C-XPILE-FRONTEND-TRAIT | metahir_module round-trip (closes F↔B pair with PMAT-435) |
| 437 | C-XPILE-CONTRACT-FRONTEND-TRAIT | EquationsBlock singleton round-trip (variant: first_equation_or_empty ∘ to_session) |
| 438 | C-XPILE-CONTRACT-BACKEND-TRAIT | rendered_doc round-trip (closes CF↔CB pair with PMAT-437) |
| 439 | C-XLATE-PY-LIST-TO-VEC | py_list round-trip (UInt8-specialized: toList∘toArray = id) |
| 440 | C-XLATE-LEAN-TO-RUST | lean_def round-trip |
| 441 | C-XLATE-RUST-FN-TO-LEAN-THM | rust_fn round-trip (closes Rust↔Lean pair with PMAT-440) |
| 442 | C-NOTATION-LATEX-MATH-TO-EQUATION | definition_env round-trip (depth-13 UNIVERSAL finale) |

Each Diamond combines: (a) round-trip equals identity, (b) primary field preserved through round-trip, (c) empty/identity input round-trips to empty/identity output, (d) reflexivity.

**Cross-substrate symmetry closures from the depth-13 wave:**
- **Frontend↔Backend round-trip pair**: PMAT-435 + PMAT-436.
- **ContractFrontend↔ContractBackend round-trip pair**: PMAT-437 + PMAT-438.
- **Rust↔Lean round-trip pair**: PMAT-440 + PMAT-441.

These thirteen templates enabled mechanical 3rd..13th-Diamond addition to every contract, driving all eleven UNIVERSAL milestones (depth-3..13).

## Diamond categories by family

### Monoid family

Captures `(S, op, identity)` with closure + associativity + identity laws. Specializations:

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Commutative monoid | PMAT-214 | C-PY-INT-ARITH | `(Int, +, 0)` and `(Int, *, 1)` |
| Semiring | PMAT-214 | C-PY-INT-ARITH | `(Int, +, 0, *, 1)` |
| Bounded monoid | PMAT-218 | C-COMPILE-RUST-TO-PTX-MMA | `(BoundedSmem, +, 0)` with sum-bound precondition |
| String monoid | PMAT-219 | C-NOTATION-LATEX-MATH-TO-EQUATION | `(String, ++, "")` on contract_id |
| Free list-monoid | PMAT-221 | C-XLATE-PY-LIST-TO-VEC | `(PyListSilver α, ++, [])` polymorphic |
| Inductive monoid | PMAT-222 | C-XLATE-LEAN-TO-RUST | `(LeanInductiveSilver, compose, empty)` |
| Precondition-list monoid | PMAT-223 | C-XLATE-RUST-FN-TO-LEAN-THM | `(PreconditionListSilver, ++, empty)` |
| Citation render-monoid | PMAT-226 | C-XPILE-CONTRACT-BACKEND-TRAIT | `(Contract, compose, empty)` on depends_on |
| Citation product-monoid | PMAT-234 | C-NOTATION-LATEX-MATH-TO-EQUATION | `(String × String, ++_componentwise)` |
| Contract product-monoid | PMAT-239 | C-XPILE-CONTRACT-BACKEND-TRAIT | `(Contract.depends_on × Contract.references, ++_componentwise)` |
| Shift-monoid | PMAT-241 | C-PY-INT-ARITH | `(Int × Nat, shl, 0)` Nat-action via powers of 2 |
| Length-monoid homomorphism | PMAT-244 | C-XLATE-PY-LIST-TO-VEC | `length: (PyListSilver α, ++, []) → (Nat, +, 0)` |
| Power-monoid | PMAT-247 | C-PY-INT-ARITH | `(Int × Nat, pow, 0)` arbitrary-base Nat-action |
| Bitwise-AND commutative monoid | PMAT-286 | C-PY-INT-ARITH | `(Int, &, ...)` via Nat.land kernel + 2's-complement |
| Closure / subalgebra | PMAT-287 | C-COMPILE-RUST-TO-PTX-MMA | bounded-sum closure under budget precondition |
| Cancellative monoid | PMAT-295 | C-COMPILE-RUST-TO-PTX-MMA | `(BoundedSmem, +, 0)` with Nat.add_left/right_cancel |
| Ordered monoid | PMAT-299 | C-COMPILE-RUST-TO-PTX-MMA | `(BoundedSmem, +, ≤)` reflexivity + transitivity + monotonicity (Mathlib's `OrderedAddCommMonoid` shape) |

### Group family

Adds inverses to monoid structure:

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Abelian group | PMAT-216 | C-FFI-CPYTHON-EXT | refcount-delta `(Int, +, 0, -)` |
| Constructive inverse | PMAT-288 | C-FFI-CPYTHON-EXT | existential witness for refcount inverse |
| Abelian-group enrichment | PMAT-290 | C-PY-INT-ARITH | `(Int, +, 0, -)` negation-involution + distributivity (enriches additive monoid to abelian group) |

### Ring family

Captures ring-theoretic axioms — bridges between additive group and multiplicative monoid:

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Ring distributivity (neg × mul) | PMAT-300 | C-PY-INT-ARITH | `(Int, +, *, neg)` with `(-a)*b = -(a*b)` — bridges PMAT-214 SEMIRING + PMAT-290 ABELIAN-GROUP into a full RING |
| Integral domain | PMAT-302 | C-PY-INT-ARITH | `(Int, +, *)` with no zero divisors (`a*b = 0 → a = 0 ∨ b = 0`) — Mathlib's `IsDomain` / `NoZeroDivisors` (strengthens PMAT-300 RING; Z/6Z falsifies) |
| Ordered ring (sign rules) | PMAT-305 | C-PY-INT-ARITH | `(Int, +, *, ≤)` sign rules: nonneg × nonneg ≥ 0, nonpos × nonpos ≥ 0, nonneg × nonpos ≤ 0, strictpos × strictpos > 0 — bridges PMAT-298 LINEAR-ORDER + PMAT-300 RING (Mathlib's `OrderedRing`; Z[i] falsifies) |

### Norm family

Captures `|·|`-style "size" / "magnitude" structures:

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Absolute value / norm | PMAT-307 | C-PY-INT-ARITH | `(Int, |·|)` as a NORMED RING: non-negativity + definiteness + triangle inequality + multiplicativity — Mathlib's `AbsoluteValue` typeclass |

### Ring-homomorphism family

Captures external category-theoretic claims about structure-preserving maps BETWEEN rings:

| Category | Example PMAT | Contract | Map |
|---|---|---|---|
| Nat-cast ring hom (INJECTIVE) | PMAT-310 | C-PY-INT-ARITH | `Nat.cast : Nat → Int` preserves 0, 1, +, * — FIRST EXTERNAL/category-theoretic claim in substrate |
| Int-emod quotient hom (SURJECTIVE) | PMAT-312 | C-PY-INT-ARITH | `(· % 2) : Int → Z/2Z` preserves +, *, non-negative, < n — FIRST QUOTIENT-RING claim |
| Nat-mod quotient hom (SURJECTIVE) | PMAT-313 | C-COMPILE-RUST-TO-PTX-MMA | `(· % 2) : Nat → Z/2Z` on BoundedSmem.val carrier — mirror of PMAT-312 |

### GCD-monoid / PID family

Captures the universal-object property of gcd plus constructive Bézout for PIDs:

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Int GCD monoid + Bézout / PID | PMAT-315 | C-PY-INT-ARITH | `Int.gcd` as universal object (divides both, dvd_gcd) plus constructive Bézout pair (`gcd a b = a*x + b*y`) — establishes Int as a `IsPrincipalIdealRing` |
| Nat GCD monoid | PMAT-316 | C-COMPILE-RUST-TO-PTX-MMA | `Nat.gcd` as universal object with commutativity (replaces Bézout since Nat lacks negatives) |

### Unit-group family

Captures the multiplicative-inverse structure within a ring:

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Int unit group `{1, -1} ≅ Z/2Z` | PMAT-317 | C-PY-INT-ARITH | multiplicative-inverse-structure of Int: -1 is self-inverse, negation factors via -1, squares non-negative |

### Sign-function family

Captures the SIGN map as a monoid morphism:

| Category | Example PMAT | Contract | Map |
|---|---|---|---|
| Int sign monoid hom | PMAT-320 | C-PY-INT-ARITH | `Int.sign : (Int, *) → ({-1, 0, 1}, *)` SURJECTIVE multiplicative monoid hom; preserves +, neg, 0, 1. Third piece of the `Int = sign × magnitude` decomposition |

### Ordered-add-comm-group family

Captures how negation interacts with the linear order on an abelian group:

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Int neg-order compatibility | PMAT-322 | C-PY-INT-ARITH | negation reverses `<` and `≤` (Mathlib's `OrderedAddCommGroup` typeclass shape); positivity-negativity duality |

### Truncated-subtraction family

Captures Nat's semiring-minus-like structure (subtraction saturates at 0):

| Category | Example PMAT | Contract | Operation |
|---|---|---|---|
| Nat truncated subtraction | PMAT-323 | C-COMPILE-RUST-TO-PTX-MMA | `Nat.sub` on BoundedSmem.val: truncates at 0 (a - b ≤ a), add-sub roundtrip, self-cancellation, zero-identity |

### Power-monoid family

Captures the Nat-action by exponentiation:

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Int power-monoid | PMAT-247 | C-PY-INT-ARITH | `Int.pow` with `(Int × Nat, pow, 0)` Nat-action |
| Nat power-monoid | PMAT-318 | C-COMPILE-RUST-TO-PTX-MMA | `Nat.pow` axioms on BoundedSmem.val (pow_zero/succ/add/one_pow) |

### Subtype-structure family

Captures claims about subtype carriers (not just operations through the projection):

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Subtype extensionality | PMAT-311 | C-COMPILE-RUST-TO-PTX-MMA | BoundedSmem ↔ Nat .val via `Subtype.ext` — extensionality + congruence + antisymmetric-lift + decidable equality. FIRST SUBTYPE-STRUCTURE claim on BoundedSmem |

### Lattice family

Captures `(S, ⊔, ⊓)` with absorption laws:

| Category | Example PMAT | Contract | Operation |
|---|---|---|---|
| Join-semilattice | PMAT-231 | C-COMPILE-RUST-TO-PTX-MMA | `(BoundedSmem, max)` |
| Meet-semilattice | PMAT-242 | C-COMPILE-RUST-TO-PTX-MMA | `(BoundedSmem, min)` |
| Bounded lattice (absorption) | PMAT-248 | C-COMPILE-RUST-TO-PTX-MMA | `max ↔ min` via absorption |
| Distributive lattice | PMAT-291 | C-COMPILE-RUST-TO-PTX-MMA | cross-distributivity of max/min |
| Order distributive lattice | PMAT-292 | C-PY-INT-ARITH | `(Int, min, max)` Int's natural ordering as a lattice |
| Bounded lattice (top+bottom) | PMAT-293 | C-COMPILE-RUST-TO-PTX-MMA | explicit top (smem_budget) + bottom (0) elements with absorption |
| Additive-lattice distributivity | PMAT-301 | C-COMPILE-RUST-TO-PTX-MMA | `(BoundedSmem, +, max, min)` tropical-semiring axiom: + distributes over max AND min (bridges PMAT-218 monoid + PMAT-291 distributive lattice) |
| Max/min monotonicity | PMAT-306 | C-COMPILE-RUST-TO-PTX-MMA | max and min are MONOTONE in both arguments (a ≤ b → max a c ≤ max b c, etc.) — order preservation distinct from algebra |
| GLB/LUB universal property | PMAT-308 | C-COMPILE-RUST-TO-PTX-MMA | CATEGORICAL definition of meet/join — min is the GREATEST lower bound, max is the LEAST upper bound (extremality, distinct from algebra/absorption/monotonicity) |

### Functor family

Captures functorial / projection-style structures preserving algebraic invariants:

| Category | Example PMAT | Contract | Projection |
|---|---|---|---|
| Cardinality functor | PMAT-237 | C-XLATE-LEAN-TO-RUST | `variant_count → (Nat, +, 0)` monoid hom |
| Constant-projection | PMAT-232 | C-XPILE-FRONTEND-TRAIT | `source_lang ← declared_lang` invariant in inputs |
| Constant-projection | PMAT-235 | C-XPILE-BACKEND-TRAIT | `target ← declared_target` invariant in inputs |
| Exit-code projection | PMAT-238 | C-BASHRS-POSIX-IDEMPOTENCE | `exit_code = 0` invariant on success path |
| Zero-copy pointer-identity | PMAT-243 | C-FFI-CPYTHON-EXT | pointer-preservation under ZeroCopy mode |
| Function-axiom | PMAT-245 | C-XPILE-FRONTEND-TRAIT | parse-and-lower totality + uniqueness + congruence |

### Relation family

Captures equivalence-relation structures (reflexivity + symmetry + transitivity) and preorders:

| Category | Example PMAT | Contract | Relation |
|---|---|---|---|
| Equivalence relation | PMAT-217 | C-XPILE-CONTRACT-FRONTEND-TRAIT | `modules_equiv` on TranspileSession |
| Equivalence-class congruence | PMAT-217 (companion) / PMAT-250 (wired) | C-XPILE-CONTRACT-FRONTEND-TRAIT | parse_to_equations preserves `modules_equiv` |
| Frontend equivalence-class | PMAT-224 | C-XPILE-FRONTEND-TRAIT | `lang_equiv` on Frontend pairs |
| Backend equivalence-class | PMAT-225 | C-XPILE-BACKEND-TRAIT | `target_equiv` on Backend pairs |
| Symmetric purity (cross-domain) | PMAT-289 | C-BASHRS-POSIX-IDEMPOTENCE | python-side purity mirrors bashrs-side purity |
| **Divisibility preorder** | **PMAT-294** | **C-PY-INT-ARITH** | **`(Int, ∣)` preorder — FIRST relation-not-operation category in substrate** |

### Order-topology family

Captures structural properties of orders beyond the algebraic axioms — totality, density, discreteness:

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Linear-order trichotomy | PMAT-298 | C-PY-INT-ARITH | `(Int, <)` trichotomy + irreflexivity + asymmetry + transitivity (totality of strict order) |
| Discrete order | PMAT-303 | C-COMPILE-RUST-TO-PTX-MMA | `(BoundedSmem.val, <)` successor + no-gaps + irreflexivity + successor-iff (distinguishes (Nat, <) from dense (Real, <)) |

### Subtype / section-retraction family

Captures preservation of refinement-subtype invariants through lowering:

| Category | Example PMAT | Contract | Subtype |
|---|---|---|---|
| NonEmpty section-retraction | PMAT-229 | C-XLATE-PY-LIST-TO-VEC | NonEmpty homogeneous list → typed Rust Vec |
| NonEmpty section-retraction | PMAT-236 | C-XLATE-RUST-FN-TO-LEAN-THM | NonEmpty precondition list → emitted Lean hypotheses (proof-lane mirror of PMAT-229) |

### Pure-function family

Captures functional purity (idempotence + cross-domain + determinism):

| Category | Example PMAT | Contract | Function |
|---|---|---|---|
| Pure function | PMAT-215 | C-BASHRS-POSIX-IDEMPOTENCE | bashrs/python cross-domain bridge |
| GIL-invariant preservation | PMAT-230 | C-FFI-CPYTHON-EXT | lock-state preserved across ABI boundary |

## Proof-pattern recipes

The 4-conjunction Diamond pattern decomposes consistently across categories:

### Monoid Diamond recipe

```lean
theorem some_monoid_diamond (a b c : Carrier) :
    -- (a) Homomorphism / closure (often PMAT-XXX lifted)
    lower (op a b) = op (lower a) (lower b)
    -- (b) Associativity
    ∧ op (op a b) c = op a (op b c)
    -- (c) Left identity
    ∧ op identity a = a
    -- (d) Right identity (sometimes omitted if op is commutative)
    ∧ op a identity = a := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · -- discharge homomorphism via existing Platinum
  · exact Op.assoc_lemma a b c
  · exact Op.left_id_lemma a
  · exact Op.right_id_lemma a
```

### Semilattice Diamond recipe

```lean
theorem some_semilattice_diamond (a b c : Carrier) :
    -- (a) Commutativity
    op a b = op b a
    -- (b) Associativity
    ∧ op (op a b) c = op a (op b c)
    -- (c) Identity or absorption (depending on bounded-vs-unbounded)
    ∧ op identity a = a       -- bottom for join, top for meet
    -- (d) Idempotence (semilattice-defining axiom)
    ∧ op a a = a := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Op.comm a b
  · exact Op.assoc a b c
  · exact Op.identity a       -- or Op.absorbs a
  · exact Op.idem a
```

### Equivalence-relation Diamond recipe

```lean
theorem some_equiv_diamond (a b c : Carrier) :
    -- (a) Reflexivity
    R a a
    -- (b) Symmetry
    ∧ (R a b → R b a)
    -- (c) Transitivity
    ∧ (R a b → R b c → R a c)
    -- (d) Function congruence (PMAT-XXX determinism lifted)
    ∧ (R a b → f a = f b) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · intro h; exact h.symm
  · intros h1 h2; exact h1.trans h2
  · -- discharge congruence via existing Platinum
```

### Constant-projection Diamond recipe

```lean
theorem some_const_proj_diamond (f : Frontend) (x y x' y' : I) :
    -- (a) Constant in first input
    (proj f x y) = (proj f x' y)
    -- (b) Constant in second input
    ∧ (proj f x y) = (proj f x y')
    -- (c) Projection equals tag-source
    ∧ (proj f x y) = f.declared_value
    -- (d) Jointly constant across all inputs
    ∧ (proj f x y) = (proj f x' y') := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact determinism_platinum f x y x' y
  · exact determinism_platinum f x y x y'
  · exact consistency_silver f x y
  · exact determinism_platinum f x y x' y'
```

## When to add a new Diamond

Add a new Diamond when **all four** of the following hold:

1. **You have a name for the algebraic structure** ("commutative monoid", "join-semilattice", "Euclidean domain", "equivalence relation").
2. **The structure has at least 3-4 axioms** that can be conjuncted into a single theorem.
3. **At least one axiom is already proven** at Platinum or Silver tier (so the Diamond is a *combination*, not a new bottom-up proof).
4. **The category is distinct** from existing Diamonds on the same contract (use `xpile diamond` to verify depth count + category novelty).

If only 1-2 axioms are available or the structure isn't a named algebraic category, ship a **Platinum** theorem instead and lift to Diamond later when more axioms accrue.

## CI enforcement

The `crates/xpile/tests/diamond_coverage.rs` integration test (PMAT-251) enforces:

- Every contract has ≥1 Diamond (depth-1 UNIVERSAL).
- Every contract has ≥2 Diamonds (depth-2 UNIVERSAL).
- ≥6 contracts have ≥3 Diamonds (depth-3 broadened via PMAT-289).
- ≥3 contracts have ≥4 Diamonds (depth-4 ACROSS LAYERS — L1 + L4 + L5).
- ≥2 contracts have ≥5 Diamonds (depth-5 ACROSS LAYERS, PMAT-286/287).
- ≥2 contracts have ≥6 Diamonds (depth-6 ACROSS LAYERS, PMAT-290/291).
- ≥2 contracts have ≥7 Diamonds (depth-7 ACROSS LAYERS, PMAT-292/293).
- ≥2 contracts have ≥8 Diamonds (depth-8 ACROSS LAYERS, PMAT-294/295).
- ≥2 contracts have ≥9 Diamonds (depth-9 ACROSS LAYERS, PMAT-298/299).
- ≥2 contracts have ≥10 Diamonds (depth-10 ACROSS LAYERS, PMAT-300/301).
- ≥2 contracts have ≥11 Diamonds (depth-11 ACROSS LAYERS, PMAT-302/303).
- ≥2 contracts have ≥12 Diamonds (depth-12 ACROSS LAYERS, PMAT-305/306).
- ≥2 contracts have ≥13 Diamonds (depth-13 ACROSS LAYERS, PMAT-307/308).
- ≥2 contracts have ≥14 Diamonds (depth-14 ACROSS LAYERS, PMAT-310/311).
- ≥2 contracts have ≥15 Diamonds (depth-15 ACROSS LAYERS, PMAT-312/313).
- ≥2 contracts have ≥16 Diamonds (depth-16 ACROSS LAYERS, PMAT-315/316).
- ≥2 contracts have ≥17 Diamonds (depth-17 ACROSS LAYERS, PMAT-317/318).
- ≥2 contracts have ≥18 Diamonds (depth-18 ACROSS LAYERS, PMAT-320/321).
- ≥2 contracts have ≥19 Diamonds (depth-19 ACROSS LAYERS, PMAT-322/323).
- ≥2 contracts have ≥20 Diamonds (depth-20 ACROSS LAYERS, PMAT-325/326).
- ≥1 contract has ≥21 Diamonds (depth-21 deepest, PMAT-327 on PyIntArith).
- 12/12 contracts have ≥3 Diamonds (depth-3 UNIVERSAL, post-PMAT-336).
- 12/12 contracts have ≥4 Diamonds (depth-4 UNIVERSAL, post-PMAT-344).
- 12/12 contracts have ≥5 Diamonds (depth-5 UNIVERSAL, post-PMAT-354).
- 12/12 contracts have ≥6 Diamonds (depth-6 UNIVERSAL, post-PMAT-365).
- 12/12 contracts have ≥7 Diamonds (depth-7 UNIVERSAL, post-PMAT-376).
- 12/12 contracts have ≥8 Diamonds (depth-8 UNIVERSAL, post-PMAT-387).
- 12/12 contracts have ≥9 Diamonds (depth-9 UNIVERSAL, post-PMAT-398).
- 12/12 contracts have ≥10 Diamonds (depth-10 UNIVERSAL, post-PMAT-409).
- 12/12 contracts have ≥11 Diamonds (depth-11 UNIVERSAL, post-PMAT-420).
- 12/12 contracts have ≥12 Diamonds (depth-12 UNIVERSAL, post-PMAT-431).
- 12/12 contracts have ≥13 Diamonds (depth-13 UNIVERSAL, post-PMAT-442).
- (Depth-4 ALL 5 LAYERS milestone subsumed by depth-4 UNIVERSAL post-PMAT-344.)
- (Depth-5 ACROSS ALL 5 LAYERS intermediate milestone (PMAT-347) subsumed by depth-5 UNIVERSAL post-PMAT-354.)
- (Depth-6 ACROSS ALL 5 LAYERS intermediate milestone (PMAT-358) subsumed by depth-6 UNIVERSAL post-PMAT-365.)
- (Depth-7 ACROSS ALL 5 LAYERS intermediate milestone (PMAT-369) subsumed by depth-7 UNIVERSAL post-PMAT-376.)
- (Depth-8 ACROSS ALL 5 LAYERS intermediate milestone (PMAT-380) subsumed by depth-8 UNIVERSAL post-PMAT-387.)
- (Depth-9 ACROSS LAYERS milestone (PMAT-296/297) subsumed by depth-9 UNIVERSAL post-PMAT-398.)
- (Depth-10 ACROSS LAYERS milestone (PMAT-300/301) subsumed by depth-10 UNIVERSAL post-PMAT-409.)
- (Depth-11 ACROSS LAYERS milestone (PMAT-302/303) subsumed by depth-11 UNIVERSAL post-PMAT-420.)
- (Depth-12 ACROSS LAYERS milestone (PMAT-305/306) subsumed by depth-12 UNIVERSAL post-PMAT-431.)
- (Depth-13 ACROSS LAYERS milestone (PMAT-307/308) subsumed by depth-13 UNIVERSAL post-PMAT-442.)

A future regression that removes a `_diamond` from any YAML or fails to keep depth-N invariants will fire the gate.
- ≥30 total wired Diamond equations.

PRs that weaken these invariants — e.g., remove a `_diamond` equation from any contract YAML — will fail CI.

Live state can be queried via `xpile diamond` (PMAT-249).

## Cross-references

- **Spec section in xpile-spec.md**: §28 (this file is the canonical body)
- **Tier definition source**: ruchy 5.0 §14.10.5 (`/home/noah/src/ruchy/docs/specifications/sub/provability-roadmap.md`)
- **CI gate**: `crates/xpile/tests/diamond_coverage.rs` (PMAT-251)
- **Reporter**: `xpile diamond` (PMAT-249, `crates/xpile/src/main.rs`)
- **Lean theorems**: `contracts/lean/*.lean` (search for `_diamond`)
- **YAML equations**: `contracts/*-v1.yaml` (search for `_diamond:`)
