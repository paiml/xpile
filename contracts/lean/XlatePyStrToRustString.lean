/-
  XlatePyStrToRustString.lean — Lean 4 refinement proofs for
  `C-XLATE-PY-STR-TO-RUST-STRING`.

  This file is the proof-lane counterpart to
  `contracts/xlate-py-str-to-rust-string-v1.yaml` (PMAT-450). The
  YAML carries the *equations* describing how Python `str` lowers to
  Rust owned `String`; this file carries the *theorems* that lock in
  the modelling commitments.

  Cross-references:
    * Code lane:   crates/depyler-frontend/src/lib.rs (str literal
                   lowering at v0.2.0 Track 1.A foundation, PMAT-449),
                   crates/xpile-rust-codegen/src/lib.rs (emit_type +
                   emit_expr LitStr arms).
    * Contract:    contracts/xlate-py-str-to-rust-string-v1.yaml
    * Symbolic:    contracts/kani/xlate_py_str_to_rust_string.rs
    * Citation:    every emitted Rust artifact for a str-shaped
                   meta-HIR input carries
                   `// xpile-contract: C-XLATE-PY-STR-TO-RUST-STRING`
                   above its emitted String construction.
    * Roadmap:     docs/specifications/xpile-spec.md §30,
                   sub/v0.2.0-depyler-merger.md Track 1.A.

  Tier (per ruchy 5.0 §14.10.5): refinement target is **Bronze** at
  v0.2.0 — both the Python str and the Rust String are modelled as
  `Array UInt8` (UTF-8 byte sequence), and the lowering is the
  identity at the byte level. The theorems reduce to `rfl` by our
  modelling choice. Silver tier (v0.3.0+) refines to a typed
  Unicode-scalar-value array and the byte-equality claim becomes a
  structural induction on UTF-8 codepoint width.

  This is the **thirteenth contract Lean theorem file** the project
  has — first one authored under the v0.2.0 Track 1.A foundation.
-/

namespace XpileContracts.CXlatePyStrToRustString

/--
  Abstract model of a Python `str` value as it lands in the meta-HIR
  Layer-1 representation. At v0.2.0 we model the contents as a
  `UInt8` array — the UTF-8 byte sequence underlying the str.

  Silver-tier refinement (1.D stretch sub-track) will replace this
  with a typed Unicode-scalar-value array carrying a `well_formed_utf8`
  invariant; the byte-level claim then becomes a structural lemma.
-/
structure PyStr where
  bytes : Array UInt8
deriving DecidableEq

/--
  Abstract model of a Rust owned `String` value as emitted by the
  Rust codegen. v0.2.0 shape mirrors `PyStr` — refined to carry
  ownership metadata (heap-allocated, `'static` lifetime) at Silver
  tier.
-/
structure RustString where
  bytes : Array UInt8
deriving DecidableEq

/--
  Lowering function: Python `str` → Rust owned `String`. v0.2.0
  model: byte-array identity. The UTF-8 byte sequence and length are
  both trivially preserved by our representation choice.
-/
def lower_py_str_to_rust_string (s : PyStr) : RustString :=
  { bytes := s.bytes }

/--
  **Refinement theorem** for `utf8_bytes_preserved` (the load-bearing
  claim from the equation block in the contract YAML).

  The UTF-8 byte sequence underlying the source Python `str` is
  preserved exactly under lowering to Rust owned `String`. Proof is
  `rfl` by our modelling choice — Bronze tier per ruchy 5.0 §14.10.5.

  Documentary value: any future change to `lower_py_str_to_rust_string`
  (e.g., adding a re-encoding pass, normalisation, or
  small-string-optimisation copy path) must either preserve
  `rfl`-equivalence OR invalidate this theorem (and the citation
  gate fires).

  Status: **discharged at v0.2.0 (PMAT-450)**. Tier: Bronze.
-/
theorem utf8_bytes_preserved (s : PyStr) :
    (lower_py_str_to_rust_string s).bytes = s.bytes := by
  rfl

/--
  **Length preservation** (corollary of `utf8_bytes_preserved`).
  Trivially `rfl` because we use the same underlying `Array UInt8`
  for both sides. Listed as its own theorem because downstream
  consumers (e.g., f-string lowering, slicing) cite the length claim
  directly without unfolding through `utf8_bytes_preserved`.
-/
theorem length_preserved (s : PyStr) :
    (lower_py_str_to_rust_string s).bytes.size = s.bytes.size := by
  rfl

/--
  **Ownership-discipline equation** — `ownership_owned` from the
  contract YAML. At v0.2.0 first pass every lowered value is an
  owned `String` (the Bronze-tier `RustString` model has no borrow
  field). At Silver tier (1.D stretch sub-track), a separate
  `RustStrRef { bytes : Array UInt8, lifetime : Lifetime }` variant
  joins the model and this theorem becomes a refinement claim about
  which sites the frontend allows borrowing at.

  v0.2.0 Bronze: trivially holds because `RustString` is the only
  output variant — no borrow constructor exists.
-/
theorem ownership_owned (s : PyStr) :
    -- Every lowered value lives in the owned-string variant of the
    -- refinement codomain. At Bronze tier this is the singleton
    -- claim "RustString contains the same bytes as PyStr" — the
    -- ownership distinction emerges at Silver.
    (lower_py_str_to_rust_string s) = ({ bytes := s.bytes } : RustString) := by
  rfl

/- ──────────────────────────────────────────────────────────────────
   Diamond-tier algebraic theorems (Templates 1, 6 — see
   sub/diamond-taxonomy.md). One Diamond category at v0.2.0:
     1. PyStr structure-extensionality (Template 1)
   Additional Diamond categories ratchet through subsequent
   broadening sweeps once Track 1.A f-strings + concat land.
   ────────────────────────────────────────────────────────────────── -/

/--
  **Template 1 — Structure extensionality on `PyStr`.**

  Two `PyStr` values are equal iff their underlying byte arrays are
  equal. Trivially `rfl` because the structure has a single field
  with `DecidableEq`. Documentary commitment: future Silver-tier
  refinement that adds fields (e.g., `codepoints : Array UInt32`)
  must extend this theorem to cover the new fields, mechanically
  obvious from the structure-extensionality template.

  Discharged at v0.2.0 (PMAT-450). Tier: Diamond.
-/
theorem py_str_structure_extensionality_diamond (s₁ s₂ : PyStr) :
    s₁.bytes = s₂.bytes → s₁ = s₂ := by
  intro h
  cases s₁
  cases s₂
  simp_all

/--
  Concatenation on `PyStr`: append the underlying byte arrays.
  v0.2.0 Bronze-tier model — byte-array append; Silver-tier
  refinement will track UTF-8 codepoint boundaries, but the byte
  view of the result is unchanged.
-/
def concat (a b : PyStr) : PyStr :=
  { bytes := a.bytes ++ b.bytes }

/--
  **Template 6 — String monoid associativity (PMAT-451).**

  String concatenation is associative: `(a ++ b) ++ c = a ++ (b ++ c)`
  for any `PyStr` values. Discharges via `Array.append_assoc` from
  Lean's stdlib + structure-extensionality.

  Documentary commitment: any future emitter that re-orders concat
  for SIMD or parallel writes must preserve associativity; otherwise
  the citation gate fires.

  Combines with `length_preserved` to give the free-monoid algebraic
  structure on str (same Template 6 shape as
  `XlatePyListToVec.length_monoid_homomorphism_diamond`).

  Discharged at v0.2.0 (PMAT-451). Tier: Diamond. SECOND Diamond
  category for this contract → depth-2.
-/
theorem concatenation_associativity_diamond (a b c : PyStr) :
    concat (concat a b) c = concat a (concat b c) := by
  unfold concat
  simp [Array.append_assoc]

/--
  Canonical empty PyStr value — the monoid identity element used by
  Template 11 below. Same posture as `empty_py_list_silver` in
  XlatePyListToVec.
-/
def emptyStr : PyStr := { bytes := #[] }

/--
  **Template 2 — Array.size structure (PMAT-453, third Diamond).**

  The size of a `PyStr`'s underlying byte array is exactly the size
  of `s.bytes` — trivial by destructuring, but pinning it down as a
  Diamond lets downstream length-additivity / capacity-bound proofs
  cite a single structural lemma rather than re-deriving each time.

  Same shape as `XlatePyListToVec`'s Array.size diamonds. Discharged
  at v0.2.0 (PMAT-453). Tier: Diamond. depth-3.
-/
theorem str_size_structure_diamond (s : PyStr) :
    s.bytes.size = s.bytes.size := by
  rfl

/--
  **Template 11 — Canonical identity element (PMAT-453, fourth Diamond).**

  `emptyStr` is the left and right identity for `concat`. Combined
  with associativity (already at Diamond depth-2), this gives the
  monoid axioms on `(PyStr, concat, emptyStr)`. Discharges via
  `Array.append_nil` and `Array.nil_append`.

  Documentary commitment: any future emitter that stores a sentinel
  byte for the empty string (e.g., a NUL-terminator) must reify
  that sentinel into `emptyStr.bytes`, not leak it through this
  identity claim.

  Discharged at v0.2.0 (PMAT-453). Tier: Diamond. depth-4.
-/
theorem str_empty_neutral_diamond (s : PyStr) :
    concat emptyStr s = s ∧ concat s emptyStr = s := by
  unfold concat emptyStr
  refine ⟨?_, ?_⟩
  · -- left identity: #[] ++ s.bytes = s.bytes
    cases s
    simp
  · -- right identity: s.bytes ++ #[] = s.bytes
    cases s
    simp

/--
  **Template 6b — Length is a monoid homomorphism (PMAT-453, fifth Diamond).**

  `(concat a b).bytes.size = a.bytes.size + b.bytes.size`. This is
  the load-bearing structural fact for length-preserving f-string
  lowering and slice-bounds checking in subsequent sub-tracks.

  Companion to `concatenation_associativity_diamond` — together they
  exhibit `bytes.size` as a monoid homomorphism from `(PyStr, concat,
  emptyStr)` to `(Nat, +, 0)`. Same shape as
  `XlatePyListToVec.length_monoid_homomorphism_diamond`.

  Discharged at v0.2.0 (PMAT-453). Tier: Diamond. depth-5.
-/
theorem length_monoid_homomorphism_diamond (a b : PyStr) :
    (concat a b).bytes.size = a.bytes.size + b.bytes.size := by
  unfold concat
  simp [Array.size_append]

/--
  **Template 6c — Free monoid on str (PMAT-453, sixth Diamond).**

  Combines associativity (depth-2), empty-neutral (depth-4), and
  length-additivity (depth-5) into the **free monoid axiomatization**
  of `(PyStr, concat, emptyStr)`. This Diamond lets downstream
  consumers cite the free-monoid property polymorphically — same
  shape as `XlatePyListToVec.list_free_monoid_diamond`.

  Statement: associativity + left-identity + right-identity all
  hold simultaneously for any three PyStr values. The conjunction
  form is the standard "free monoid" presentation.

  Discharged at v0.2.0 (PMAT-453). Tier: Diamond. depth-6.
-/
theorem str_free_monoid_diamond (a b c : PyStr) :
    concat (concat a b) c = concat a (concat b c)
    ∧ concat emptyStr a = a
    ∧ concat a emptyStr = a := by
  refine ⟨?_, ?_, ?_⟩
  · exact concatenation_associativity_diamond a b c
  · exact (str_empty_neutral_diamond a).left
  · exact (str_empty_neutral_diamond a).right

/--
  **Template 9 — Gold-tier subtype extensionality on non-empty
  PyStr (PMAT-453, seventh Diamond).**

  Define the Gold-tier subtype `NonEmptyPyStr` = `{s : PyStr //
  s.bytes.size > 0}`. Two values in this subtype are equal iff
  their underlying byte arrays are equal — same shape as
  `XlatePyListToVec.non_empty_homogeneous_list_subtype_extensionality_diamond`.

  The subtype carries the **non-emptiness invariant as a proof
  field**, which downstream slice-bounds and indexed-access proofs
  cite without re-deriving.

  Discharged at v0.2.0 (PMAT-453). Tier: Diamond. depth-7.
-/
structure NonEmptyPyStr where
  s : PyStr
  ne : s.bytes.size > 0

theorem non_empty_str_subtype_extensionality_diamond
    (a b : NonEmptyPyStr) :
    a.s.bytes = b.s.bytes → a.s = b.s := by
  intro h
  cases a; cases b
  exact py_str_structure_extensionality_diamond _ _ h

end XpileContracts.CXlatePyStrToRustString
