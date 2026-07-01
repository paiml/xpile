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

/-
  ── PMAT-995 (slice 3b): the dict/set entry-array layout ──────────────────

  A `dict[int|str, int]` / `set[int|str]` rides the same bump heap as a heap
  string, laid out as an OPEN ASSOC-ARRAY: an 8-byte header (`i32` live count
  @ base+0, `i32` capacity @ base+4) then `capacity` fixed 16-byte entries from
  base+8. Entry `i` starts at `entryAddr base i = base + 8 + i*16`; within an
  entry the key is at `+0` and the value at `+valOff` (= 8). The lemmas below
  machine-check that this layout is WELL-FORMED: consecutive entries are
  contiguous, distinct entries are DISJOINT (the linear scan never aliases a
  neighbour), and an entry's `i64` value fits inside its 16-byte slot. These
  strengthen the C-WASM-HEAP tier with the dict/set structural facts the
  `heap_dict_witness` execution witness relies on. Core-only, sorry-free.
-/

/-- The dict/set header size in bytes (`i32` count @ +0, `i32` capacity @ +4),
    keeping the entry array 8-aligned. -/
def dictHeaderSize : Nat := 8

/-- The fixed byte size of one dict/set entry (a key slot + a value slot). -/
def dictEntrySize : Nat := 16

/-- The value's byte offset within an entry (the key occupies `[0, valOff)`). -/
def dictValOff : Nat := 8

/-- Linear-memory address of entry `i` in a dict/set at base pointer `base`. -/
def entryAddr (base i : Nat) : Nat := base + dictHeaderSize + i * dictEntrySize

/-- Consecutive entries are CONTIGUOUS: entry `i` ends exactly where entry
    `i+1` begins, so the fixed-stride linear scan visits a gap-free array. -/
theorem dict_entries_contiguous (base i : Nat) :
    entryAddr base i + dictEntrySize = entryAddr base (i + 1) := by
  unfold entryAddr dictEntrySize
  omega

/-- Distinct entries are DISJOINT: for `i < j`, entry `i`'s 16-byte region ends
    at or before entry `j` begins — the linear scan never reads a neighbour's
    key/value. This is the non-aliasing safety property of the open array. -/
theorem dict_entries_disjoint (base i j : Nat) (h : i < j) :
    entryAddr base i + dictEntrySize ≤ entryAddr base j := by
  unfold entryAddr dictEntrySize
  have : i + 1 ≤ j := h
  omega

/-- An entry's `i64` value (8 bytes at `+dictValOff`) fits inside the 16-byte
    entry slot — it never spills into the next entry's key. -/
theorem dict_value_fits_in_entry : dictValOff + 8 ≤ dictEntrySize := by
  decide

end XpileContracts.CWasmHeap
