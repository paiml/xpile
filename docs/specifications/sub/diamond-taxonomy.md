# Diamond-Tier Refinement Taxonomy

**Section 28 of [xpile-spec.md](../xpile-spec.md).** Catalogs every Diamond-tier algebraic category demonstrated across xpile's contract substrate.

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

As of v0.1.0+ (PMAT-214..376), the substrate has:

- **Diamond depth-1 UNIVERSAL** (12/12 contracts): every contract has at least one Diamond at one algebraic category.
- **Diamond depth-2 UNIVERSAL** (12/12 contracts): every contract has at least two **distinct** Diamond categories. CI-enforced via PMAT-251.
- **Diamond depth-3 UNIVERSAL** (12/12 contracts, post-PMAT-336): every contract has ≥3 distinct Diamond categories. CI-enforced via tightened gate. Achieved through PMAT-241..245 (Layer coverage) + PMAT-289 + PMAT-331..336 broadening sweep.
- **Diamond depth-4 UNIVERSAL** (12/12 contracts, post-PMAT-344): every contract has ≥4 distinct Diamond categories. Achieved through PMAT-247/248/288/329/330 (ALL 5 LAYERS milestone at PMAT-330) followed by 7-PR broadening sweep (PMAT-338..344) using five recurring algebraic templates.
- **Diamond depth-5 UNIVERSAL** (12/12 contracts, post-PMAT-354): every contract has ≥5 distinct Diamond categories. Achieved through PMAT-286/287/328 (initial L1/L4/L5 opens) followed by a 9-PR broadening sweep (PMAT-346..354), with **depth-5 ACROSS ALL 5 TAXONOMY LAYERS** intermediate milestone at PMAT-347 and **depth-5 UNIVERSAL** finale at PMAT-354. The broadening leaned heavily on the structure-extensionality template (PMAT-349, 352, 353, 354) and introduced the String.length Nat-structure as a sixth recurring template (PMAT-346, 350).
- **Diamond depth-6 UNIVERSAL** (12/12 contracts, post-PMAT-365): every contract has ≥6 distinct Diamond categories. Achieved through PMAT-290/291 (initial L1/L5 opens) followed by a 10-PR broadening sweep (PMAT-356..365), with **depth-6 ACROSS ALL 5 TAXONOMY LAYERS** intermediate milestone at PMAT-358 and **depth-6 UNIVERSAL** finale at PMAT-365. The wave was dominated by the structure-extensionality template (PMAT-356/359/360/361/362/363/364) — and closed Rust↔Lean Array.size invariant on both sides (PMAT-344 Rust, PMAT-365 Lean) and ContractFrontend↔ContractBackend inner/outer record extensionality on the trait pair (PMAT-353/354/361/364).
- **Diamond depth-7 UNIVERSAL** (12/12 contracts, post-PMAT-376): every contract has ≥7 distinct Diamond categories. Achieved through PMAT-292/293 (initial L1/L5 opens) followed by a 10-PR broadening sweep (PMAT-367..376), with **depth-7 ACROSS ALL 5 TAXONOMY LAYERS** intermediate milestone at PMAT-369 and **depth-7 UNIVERSAL** finale at PMAT-376. The wave continued the structure-extensionality template (PMAT-367/368/371/373/374) and Array.size template (PMAT-375/376), and **introduced the enum completeness template as a 7th recurring algebraic family** (PMAT-370 Target, PMAT-372 LatexDisplayKind).
- **Diamond depth-7 ACROSS LAYERS** (2/12 contracts): PMAT-292 (order-distributive-lattice on L1) + PMAT-293 (bounded lattice with top+bottom on L5).
- **Diamond depth-8 ACROSS LAYERS** (2/12 contracts): PMAT-294 (divisibility-preorder on L1, FIRST relation-not-operation category) + PMAT-295 (cancellative monoid on L5).
- **Diamond depth-9 ACROSS LAYERS** (2/12 contracts): PMAT-298 (linear-order trichotomy on L1) + PMAT-299 (ordered-monoid on L5).
- **Diamond depth-10 ACROSS LAYERS** (2/12 contracts): PMAT-300 (RING-distributivity / neg × mul bridge on L1) + PMAT-301 (additive-lattice / tropical-semiring axiom on L5).
- **Diamond depth-11 ACROSS LAYERS** (2/12 contracts): PMAT-302 (integral-domain / no-zero-divisors on L1) + PMAT-303 (discrete-order / successor + no-gaps on L5).
- **Diamond depth-12 ACROSS LAYERS** (2/12 contracts): PMAT-305 (ordered-ring sign rules on L1) + PMAT-306 (max/min monotonicity on L5).
- **Diamond depth-13 ACROSS LAYERS** (2/12 contracts): PMAT-307 (absolute value / norm on L1) + PMAT-308 (GLB/LUB universal property on L5).
- **Diamond depth-14 ACROSS LAYERS** (2/12 contracts): PMAT-310 (Nat-cast ring homomorphism on L1, FIRST EXTERNAL claim) + PMAT-311 (subtype extensionality on L5, FIRST SUBTYPE-STRUCTURE claim).
- **Diamond depth-15 ACROSS LAYERS** (2/12 contracts): PMAT-312 (Int-emod quotient ring homomorphism on L1, FIRST QUOTIENT-RING claim) + PMAT-313 (Nat-mod quotient ring homomorphism on L5).
- **Diamond depth-16 ACROSS LAYERS** (2/12 contracts): PMAT-315 (Int gcd-monoid + Bézout / PID on L1, FIRST UNIVERSAL-OBJECT-WITH-CONSTRUCTIVE-WITNESS claim) + PMAT-316 (Nat gcd-monoid on L5).
- **Diamond depth-17 ACROSS LAYERS** (2/12 contracts): PMAT-317 (unit group `{1, -1} ≅ Z/2Z` on L1, FIRST UNIT-GROUP claim) + PMAT-318 (Nat power-monoid on L5).
- **Diamond depth-18 ACROSS LAYERS** (2/12 contracts): PMAT-320 (sign function monoid hom `Int → {-1,0,1}` on L1, third piece of sign×magnitude decomposition) + PMAT-321 (Nat integral domain on L5).
- **Diamond depth-19 ACROSS LAYERS** (2/12 contracts): PMAT-322 (negation-order compatibility / OrderedAddCommGroup on L1) + PMAT-323 (Nat truncated subtraction on L5).
- **Diamond depth-20 ACROSS LAYERS** (2/12 contracts): PMAT-325 (Int.toNat partial inverse on L1) + PMAT-326 (Nat power monotonicity on L5).
- **Diamond depth-21** (1/12 contracts, DEEPEST): PMAT-327 (Nat-cast order embedding on PyIntArith L1, captures Mathlib's `OrderRingHom Nat Int` shape together with PMAT-310).

**Substrate total: 111 wired Diamond equations across 12 contracts.**

## Recurring algebraic templates (substrate-wide)

Seven recurring algebraic templates emerged during the depth-3/4/5/6/7 broadening sweeps. Each is mechanically applicable to specific record/subtype/enum patterns, enabling the **depth-3 UNIVERSAL** (PMAT-336), **depth-4 UNIVERSAL** (PMAT-344), **depth-5 UNIVERSAL** (PMAT-354), **depth-6 UNIVERSAL** (PMAT-365), and **depth-7 UNIVERSAL** (PMAT-376) milestones:

### Template 1: Structure-extensionality

Demonstrated on **26 distinct record/subtype contracts** (PMAT-311 + PMAT-329..336 + PMAT-349, 352..354, 356, 359..364, 367, 368, 371, 373, 374):

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

### Template 2: Array.size structure

Demonstrated on **9 contracts** (PMAT-340/341/344/348/351/358/365/375/376) for `Array.size` axioms on record fields:

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

Demonstrated on 2 contracts (PMAT-346/350) for `String.length` Nat-measure invariants on text-valued record fields:

| PMAT | Contract | Field |
|---|---|---|
| 346 | C-BASHRS-POSIX-IDEMPOTENCE | OutcomeSilver.observable (String) |
| 350 | C-NOTATION-LATEX-MATH-TO-EQUATION | EquationFormulaSilver.ascii_normalised (String) |

Combines: length non-negativity (trivially for Nat), empty-string length-0, field-replacement preservation, other-field independence. Complements Template 2 (Array.size) — both Nat-measure invariants on container fields, targeting String vs. Array containers respectively.

### Template 7: Int-sign decomposition

Demonstrated on 2 contracts (PMAT-328/357) for sign-trichotomy + absolute-value invariants on Int-valued fields with semantic dichotomy:

| PMAT | Contract | Field |
|---|---|---|
| 328 | C-FFI-CPYTHON-EXT | FfiCallSilver.refcount_delta (Int — balanced/leaked/over-decref) |
| 357 | C-BASHRS-POSIX-IDEMPOTENCE | OutcomeSilver.exit_code (Int — success/failure) |

Combines: sign trichotomy (0 < x ∨ x = 0 ∨ x < 0), absolute-value non-negativity, zero-value identity, reflexivity. Cross-substrate parallel — both contracts host Int fields with semantic success/failure dichotomies that the sign-structure axiomatizes.

### Template 8: Enum completeness

Demonstrated on 2 contracts (PMAT-370/372) for total-coverage axiomatization of finite enum types:

| PMAT | Contract | Enum |
|---|---|---|
| 370 | C-XPILE-BACKEND-TRAIT | Target (7 variants — rust/ruchy/lean/ptx/wgsl/spirv/shell) |
| 372 | C-NOTATION-LATEX-MATH-TO-EQUATION | LatexDisplayKind (3 variants — displayMath/equation/align) |

Combines: total coverage (every value matches one of N known variants), self-equality, decidable membership, constructor distinctness sample. Complement to Template 3 (enum distinctness) — together they give the full finite-enumeration axiomatization. Introduced during the depth-7 broadening sweep (PMAT-367..376) to add genuine new algebraic categories beyond struct-extensionality/Array.size repetition.

These eight templates enabled mechanical 3rd/4th/5th/6th/7th-Diamond addition to every depth-2/depth-3/depth-4/depth-5/depth-6 contract, driving all five UNIVERSAL milestones (depth-3/4/5/6/7).

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
- (Depth-4 ALL 5 LAYERS milestone subsumed by depth-4 UNIVERSAL post-PMAT-344.)
- (Depth-5 ACROSS ALL 5 LAYERS intermediate milestone (PMAT-347) subsumed by depth-5 UNIVERSAL post-PMAT-354.)
- (Depth-6 ACROSS ALL 5 LAYERS intermediate milestone (PMAT-358) subsumed by depth-6 UNIVERSAL post-PMAT-365.)
- (Depth-7 ACROSS ALL 5 LAYERS intermediate milestone (PMAT-369) subsumed by depth-7 UNIVERSAL post-PMAT-376.)

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
