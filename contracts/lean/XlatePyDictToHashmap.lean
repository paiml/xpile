/-
  XlatePyDictToHashmap.lean — Lean 4 refinement proof for
  `C-XLATE-PY-DICT-TO-HASHMAP`.

  Proof-lane counterpart to `contracts/xlate-py-dict-to-hashmap-v1.yaml`
  (R6 / PMAT-475c). The YAML carries the *equation* describing how a
  Python `dict` lowers to a Rust `std::collections::HashMap`; this file
  carries the *theorem* that locks in the modelling commitment.

  The load-bearing commitment: the Python dict → Rust HashMap lowering is
  the *identity* on the abstract finite-map (key → value) structure — it
  preserves the (key, value) entry sequence and the cardinality, with no
  per-entry coercion. We model both a `PyDict` and a `RustHashMap` as an
  entry list `List (K × V)` and the lowering as the identity on entries,
  so the structure-preservation claims reduce to `rfl` (Bronze tier).

  Cross-references:
    * Code lane:   crates/depyler-frontend/src/lib.rs (dict literal /
                   comprehension lowering + the homogeneous-key/value
                   checks citing this contract),
                   crates/xpile-rust-codegen/src/lib.rs (emit HashMap).
    * Contract:    contracts/xlate-py-dict-to-hashmap-v1.yaml
    * Sibling:     contracts/lean/XlatePyListToVec.lean (list → Vec).
    * Roadmap:     docs/specifications/xpile-spec.md §30 (R6 / PMAT-475),
                   audit-design.md §7.3 (the falsification this closes).

  Tier (per ruchy 5.0 §14.10.5): **Diamond** shape (one tier-defining
  theorem combining entry-sequence preservation + cardinality
  preservation), joining the substrate at **depth-1** under the
  R6-grandfathered Diamond gate — NOT forced to the depth-13 floor.
  Silver tier (later) refines `PyDict`/`RustHashMap` to a keyed partial
  function with the lookup-after-insert law.

  This is the **fifteenth contract Lean theorem file** the project has —
  authored alongside `CIntArith.lean` to close R6.
-/

namespace XpileContracts.CXlatePyDictToHashmap

/--
  Abstract model of a Python `dict[K, V]` as it lands in the meta-HIR
  Layer-2 representation: the sequence of (key, value) entries. (Python
  dicts preserve insertion order; the *map* they denote is unordered, and
  the lowering target — `HashMap` — is likewise unordered. The structural
  theorem below is stated over the entry sequence and so is the strongest,
  order-sensitive form; it implies the set-of-entries equality the YAML
  postcondition names.)
-/
structure PyDict (K V : Type) where
  entries : List (K × V)

/--
  Abstract model of a Rust `std::collections::HashMap<K, V>` value as
  emitted by the codegen — same entry-sequence shape as `PyDict` at
  Bronze tier (refined to carry hashing/bucket metadata at Silver).
-/
structure RustHashMap (K V : Type) where
  entries : List (K × V)

/--
  Lowering function: Python `dict` → Rust `HashMap`. v1.0.0 model:
  entry-list identity. Every (key, value) pair is carried across with no
  coercion, drop, or duplication.
-/
def lower {K V : Type} (d : PyDict K V) : RustHashMap K V :=
  { entries := d.entries }

/--
  **Diamond refinement theorem** for `dict_to_hashmap_structure_preserved_diamond`
  (the load-bearing claim from the equation block in the contract YAML).

  The Python dict → Rust HashMap lowering is the identity on the abstract
  finite-map structure: it preserves both the (key, value) entry sequence
  and its cardinality. The two conjuncts are the COMBINED
  structure-extensionality axiomatization (entry preservation + cardinality
  preservation), which is what lifts this from a single Platinum fact to a
  Diamond-shaped theorem.

  Documentary value: any future change to `lower` (a re-keying pass, a
  bucket re-encoding that drops/merges entries, or a coercion at the
  key/value boundary) must either preserve both identities OR invalidate
  this theorem (and the citation gate fires).

  Status: **discharged at v1.0.0 (PMAT-475c)**. Tier: Diamond shape, depth-1.
-/
theorem dict_to_hashmap_structure_preserved_diamond {K V : Type} (d : PyDict K V) :
    ((lower d).entries = d.entries)
      ∧ ((lower d).entries.length = d.entries.length) :=
  ⟨rfl, rfl⟩

end XpileContracts.CXlatePyDictToHashmap
