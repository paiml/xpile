/-
  WasmHeap.lean — Lean 4 refinement proof for `C-WASM-HEAP`.

  Proof-lane counterpart to `contracts/c-wasm-heap-v1.yaml` (PMAT-993, the
  PMAT-986 slice-2 bump allocator). A string-RETURNING meta-HIR op (string
  concat `a + b`, `chr(n)`, a `str` return) lowers through `xpile-wasm-codegen`
  to WASM that ALLOCATES + materialises the result in linear memory via a
  bump heap (`$__heap_ptr` global + `$__alloc`). This is the string-construction
  EXTENSION of `C-COMPILE-RUST-TO-WASM` (which governs read-only / scalar /
  control emission, incl. read-only string access).

  The WASM-heap sibling of `XlateRustToWasm.lean` (Layer 5 / compile-time).
  Where that module models the emitted WAT function's `(name, params, result)`
  signature, this one models a bump-heap ALLOCATION as `$__alloc` produces it —
  its returned `base` address and the requested `size` in bytes — and proves
  STRUCTURE EXTENSIONALITY over it: a bump-heap allocation is determined by its
  `(base, size)` signature. This registers `C-WASM-HEAP` at depth-1 under the
  Diamond gate, mirroring the str/list/float/set/wgsl/wasm structural Diamonds.
  Core-only, no Mathlib, sorry-free — machine-checked by the `lake build` pilot.

  Execution-semantics Diamonds (the actual constructed-string agreement —
  the WABT string-building witness reads a concatenated string back and asserts
  it equals CPython `a + b`) are the runtime-stratum half and ratchet into
  deeper tiers later; the depth-1 tier-defining theorem here is the
  allocation's structure extensionality.
-/

namespace XpileContracts.CWasmHeap

/--
  Abstract model of a bump-heap allocation as `$__alloc` produces it: the
  returned `base` linear-memory address and the requested `size` in bytes.
  `$__alloc(n)` returns the current bump pointer (`base`) and advances it by
  `align8(n)`; this models the (base, size) signature of one allocation.
-/
structure WasmAlloc where
  base : Nat
  size : Nat
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `wasm_heap_alloc_structure_extensionality_diamond`
  (the tier-defining equation in the contract YAML): two bump-heap allocations
  with the same base address and the same requested size are equal. Registers
  `C-WASM-HEAP` at depth-1, mirroring the structural Diamonds of the
  str/list/float/set/wgsl/wasm contracts. Sorry-free, core-only.
-/
theorem wasm_heap_alloc_structure_extensionality_diamond (a b : WasmAlloc) :
    a.base = b.base →
    a.size = b.size →
    a = b := by
  intro hb hs
  cases a
  cases b
  simp_all

/--
  Round an allocation request up to the next 8-byte boundary — the alignment
  `$__alloc` applies (`(n + 7) & ~7`). Modelled here as `((n + 7) / 8) * 8`,
  the same value with no bitwise ops (core-only).
-/
def align8 (n : Nat) : Nat := ((n + 7) / 8) * 8

/--
  `align8` is idempotent on an already-8-aligned size: a constructed string's
  region size is rounded once and stays put. A sanity lemma over the alignment
  the bump pointer advances by (so a second `__alloc` of the same already-padded
  size lands at the same stride). Core-only, sorry-free.
-/
theorem align8_idempotent (n : Nat) : align8 (align8 n) = align8 n := by
  unfold align8
  -- Let k = (n+7)/8. Then ((k*8) + 7)/8 = (8*k + 7)/8 = k + 7/8 = k, so
  -- rounding an already-(k*8) value again is identity.
  generalize (n + 7) / 8 = k
  have h : (k * 8 + 7) / 8 = k := by
    rw [Nat.mul_comm]
    -- (8*k + 7) / 8 = k + 7/8 = k + 0 = k
    rw [Nat.mul_add_div (by decide : 0 < 8)]
    -- remaining: k + 7/8 = k, and 7/8 = 0 in Nat
    simp
  rw [h]

end XpileContracts.CWasmHeap
