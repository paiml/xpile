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

As of v0.1.0 (PMAT-214..252), the substrate has:

- **Diamond depth-1 UNIVERSAL** (12/12 contracts): every contract has at least one Diamond at one algebraic category.
- **Diamond depth-2 UNIVERSAL** (12/12 contracts): every contract has at least two **distinct** Diamond categories. CI-enforced via PMAT-251.
- **Diamond depth-3 UNIVERSAL across layers** (5/5 layers): every layer of the contract taxonomy has at least one contract with three distinct Diamond categories.
- **Diamond depth-4 opened** (2/12 contracts): PyIntArith (Layer 1) and CompileRustToPtxMma (Layer 5) each have four distinct Diamond categories.

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

### Group family

Adds inverses to monoid structure:

| Category | Example PMAT | Contract | Carrier |
|---|---|---|---|
| Abelian group | PMAT-216 | C-FFI-CPYTHON-EXT | refcount-delta `(Int, +, 0, -)` |

### Lattice family

Captures `(S, ⊔, ⊓)` with absorption laws:

| Category | Example PMAT | Contract | Operation |
|---|---|---|---|
| Join-semilattice | PMAT-231 | C-COMPILE-RUST-TO-PTX-MMA | `(BoundedSmem, max)` |
| Meet-semilattice | PMAT-242 | C-COMPILE-RUST-TO-PTX-MMA | `(BoundedSmem, min)` |
| Bounded lattice | PMAT-248 | C-COMPILE-RUST-TO-PTX-MMA | `max ↔ min` via absorption |

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

Captures equivalence-relation structures (reflexivity + symmetry + transitivity):

| Category | Example PMAT | Contract | Relation |
|---|---|---|---|
| Equivalence relation | PMAT-217 | C-XPILE-CONTRACT-FRONTEND-TRAIT | `modules_equiv` on TranspileSession |
| Equivalence-class congruence | PMAT-217 (companion) / PMAT-250 (wired) | C-XPILE-CONTRACT-FRONTEND-TRAIT | parse_to_equations preserves `modules_equiv` |
| Frontend equivalence-class | PMAT-224 | C-XPILE-FRONTEND-TRAIT | `lang_equiv` on Frontend pairs |
| Backend equivalence-class | PMAT-225 | C-XPILE-BACKEND-TRAIT | `target_equiv` on Backend pairs |

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
- ≥5 contracts have ≥3 Diamonds (depth-3 across layers).
- ≥2 contracts have ≥4 Diamonds (depth-4 opened).
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
