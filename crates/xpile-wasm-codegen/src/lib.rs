//! Native WebAssembly backend — the EMIT half of first-class
//! bidirectional native WASM (PMAT-951).
//!
//! Lowers the meta-HIR **scalar/control subset** directly to
//! WebAssembly Text format (WAT) text, emitted into [`Artifact::primary`]
//! as a `String` — **no runtime dependency** (a String-emitter shape, the
//! same posture as the PTX/WGSL scaffold emitters, but here a *real*
//! meta-HIR lowering rather than a hardcoded shader).
//!
//! This is the tractable (high→low) direction of the bidirectional WASM
//! goal: structured meta-HIR (funcs/locals/if/loops/i64/f64/calls) →
//! WASM. It deliberately does NOT go through the Ruchy `WasmEmitter` hop
//! (the spec §73/§484 delegated-only stance, now superseded) — xpile owns
//! the lane, exactly as the cuda-oxide "own it, drop the delegation"
//! decision (`project-gpu-cuda-oxide-wgpu-first-class`).
//!
//! ## Supported subset
//!
//! - Types: `I64`/`CLong` → `i64`, `F64` → `f64`, `F32` → `f32`,
//!   `Bool` → `i32` (WASM has no bool; 0/1 in an i32), and a 32-bit-ish
//!   `CUInt` → `i32`. Everything else (`Dict`/`Set`/`Struct`/`Tuple`/
//!   `BigInt`/`Optional`/pointers/…) is **refused** with
//!   [`BackendError::Lower`] — a Lean-style honest refusal, never wrong
//!   code.
//! - The FIRST string support (PMAT-986 slice 1): a `str` **parameter**
//!   lowers to an `i32` base-pointer into WASM **linear memory**, mirroring
//!   the `list[scalar]` ABI exactly — an `i32` UTF-8 **byte count** at
//!   `base+0`, then the raw UTF-8 bytes from `base+8` (the same 8-byte
//!   header offset the list layout uses). This length header unlocks two
//!   read-only string operations: (1) **`len(s)`** over a str param lowers
//!   to the SAME header `i32.load` + `i64`-extend the list `len(xs)` uses —
//!   ASCII-restricted (for ASCII the byte count equals the Python `len`
//!   char count; the emitter cannot cheaply distinguish a multi-byte UTF-8
//!   string, so a non-ASCII string would report a byte count that is NOT
//!   the Python char count — the honest posture is that callers pass ASCII,
//!   documented on the lowering); and (2) **`ord(s[i])`** (`Expr::Ord` over
//!   an `Expr::StrCharAt` of a str param) lowers to a bounds-checked
//!   `i32.load8_u` of the `i`-th byte — the per-byte code point, the same
//!   bounds-guard shape (`i < 0 || i >= byte_count → unreachable`) the list
//!   index path uses, then zero-extended to the `i64` Python-int domain.
//!   As of **PMAT-993 (slice 2)** string-RETURNING ops are unblocked by a
//!   linear-memory **bump allocator** (`(global $__heap_ptr (mut i32))` past
//!   the static `(data)` region plus `$__alloc(n)`, 8-byte-aligned, bump-only
//!   with no free; see `HEAP_BASE`/`heap_helpers`). The slice-2 string ops:
//!   **string concatenation `a + b`** (`Expr::Concat`) does `alloc(8 + Σ
//!   len(opᵢ))`, writes the count header, `memory.copy`s each operand's bytes,
//!   and returns the new base-pointer (left-nested `(a+b)+c` flattens to ONE
//!   alloc with N copies); and **`chr(n)`** (`Expr::Chr`) does `alloc(9)`, a
//!   count-1 header, then an `i32.store8` of `n & 0xFF` (ASCII-bounded, the
//!   `ord` mirror). A function RETURNING a `str` now works (the result is the
//!   new string's `i32` heap pointer). As of **PMAT-994 (slice 3a)** string
//!   support is substantially complete: (1) string **LITERALS** (`Expr::LitStr`,
//!   e.g. `"Hello, "`) are materialised at emit time into static `(data …)`
//!   segments in `[LITERAL_BASE, HEAP_BASE)` (length-prefixed, the same ABI),
//!   and a `LitStr` lowers to a constant `i32.const <base>` — so `"Hi " +
//!   name`, `return "done"`, and literal args all work; (2) **`s[i]` as a
//!   1-char string** (`Expr::StrCharAt` outside `ord`) materialises a new
//!   1-char heap string (the `chr` mirror, copying byte `i` of the
//!   string-valued base, bounds-checked); and (3) string **content equality**
//!   `a == b` / `a != b` lowers to a `$__wasm_str_eq` helper (length check +
//!   byte-compare loop → i32 bool) — REAL content logic, never a base-pointer
//!   compare. Still **refused** honestly (a hard `BackendError`): string
//!   ORDERING (`<` / `>`), slicing, `str(x)`/`repr(x)`, f-strings, string
//!   methods, and `dict` / `set` / `struct` (slice 3b).
//! - The FIRST aggregate (PMAT-966 + PMAT-968): a `list[int]`/`list[float]`
//!   **parameter** lowers to an `i32` base-pointer into WASM **linear
//!   memory**. As of PMAT-968 the pointed-at region is a length-prefixed
//!   layout: an `i32` element **count** at `base+0`, then the packed
//!   elements starting at `base+8` (the 8-byte offset keeps every `i64`/
//!   `f64` element naturally aligned). This length header unlocks two
//!   PMAT-968 deliverables that PMAT-966 deliberately refused — (1)
//!   **bounds-checked `xs[i]`**, where the `Index` lowering loads the header
//!   length and emits a guard `i < 0 || i >= len → unreachable` (a WASM trap)
//!   BEFORE the `*.load`, the faithful Python `IndexError` analogue (PMAT-966
//!   let an out-of-range address silently mis-read or trap only on an
//!   unmapped page); and (2) **`len(xs)`** (`Expr::Len`), lowered to an
//!   `i32.load` of the header count zero-extended to the `i64` Python-int
//!   domain. The element type must itself be a supported scalar
//!   (`i64`/`f64`/`f32`); a `list[bool]`, nested list, or list of strings is
//!   refused. As of PMAT-978 a single-index **write** `xs[i] = v`
//!   (`Stmt::IndexAssign`) is also supported — the mutation companion of the
//!   read path, reusing the SAME bounds guard + address math but terminating
//!   in a natural-width `*.store`. A list **literal**, list **return**, list
//!   **append** / growth, and a MULTI-index / nested-list write
//!   (`xs[i][j] = v`) remain refused — fixed-list scalar access (read +
//!   single-index write) plus `len` is the deliverable.
//! - Statements: `Let`/`Assign` (→ `local` + `local.set`), `If`/`While`/
//!   `Break`/`Continue`/`Return`, and `xs[i] = v` (`IndexAssign`) over a
//!   `list[scalar]` param (bounds-checked `*.store`).
//! - Expressions: `Ident` (→ `local.get`), `LitInt`/`LitFloat`/`LitBool`,
//!   `BinOp` (arith/bitwise/shift + comparisons), `FloatBinOp`, `UnOp`,
//!   `IfExpr`, `Index` over a `list[scalar]` param (bounds-checked `*.load`),
//!   `Len` over a `list[scalar]` param (→ header `i32.load` + `i64` extend),
//!   and a direct intra-module `Call`.
//!
//! ## Semantic posture (replicated from the Rust/PTX lanes)
//!
//! - **`C-PY-INT-ARITH` overflow:** `i64.add`/`sub`/`mul` WRAP silently in
//!   WASM (two's-complement modular). The Python contract bounds overflow,
//!   so for `I64` arithmetic the emitter inserts an explicit checked
//!   sequence that **traps** (`unreachable`) on signed overflow rather
//!   than silently wrapping — matching the Rust lane's checked-arithmetic
//!   panic posture. (`CUInt` modular wrapping is the *defined* C semantics,
//!   so it uses bare `i32` ops.)
//! - **FloorDiv / Mod:** WASM `i64.div_s` truncates toward zero and traps
//!   on divide-by-zero; `i64.rem_s` is the truncating remainder. Python's
//!   `//`/`%` floor toward −∞ and the remainder takes the divisor's sign,
//!   so the emitter applies the same floor correction the Rust lane uses
//!   (`div_euclid`/`rem_euclid`-equivalent), implemented inline in WAT.
//!
//! Layer 5 compile contract: `contracts/compile-rust-to-wasm-v1.yaml`
//! (`C-COMPILE-RUST-TO-WASM`), proof lane `contracts/lean/XlateRustToWasm.lean`.

use std::fmt::Write as _;

use xpile_backend::{
    Artifact, Backend, BackendConfig, BackendError, EmittedText, MultiEmitterBackend, QuorumPolicy,
    QuorumStatus, Target, TargetEmitter,
};
use xpile_contracts::ContractId;
use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, Stmt, Type, UnOp,
};

mod wasm_diffexec;
pub use wasm_diffexec::{
    general_module_wat, wasm_runtime_available, WasmDiffExecEngine, FIXTURE_INPUT,
};

/// The Layer-5 compile contract every emitted WAT function cites.
const CONTRACT_ID: &str = "C-COMPILE-RUST-TO-WASM";

/// PMAT-968 list ABI / PMAT-986 str ABI: a `list[scalar]` base-pointer
/// points at an `i32` element-count header at `base+0`; the packed elements
/// start at this byte offset. The offset is 8 (not 4) so every `i64`/`f64`
/// element stays naturally aligned for `i64.load`/`f64.load`. The PMAT-986
/// `str` ABI is byte-identical — an `i32` UTF-8 **byte count** at `base+0`,
/// the raw bytes from `base+8` — so a str shares this same constant (its
/// per-byte `i32.load8_u` access needs no alignment, but reusing the layout
/// keeps the single list/str linear-memory ABI uniform).
const LIST_ELEMS_OFFSET: i32 = 8;

/// PMAT-968: name of the per-function scratch `i64` local that holds an
/// evaluated `Index` index, reused by the bounds guard and the address
/// computation (so the index expression is evaluated exactly once). Prefixed
/// with `__wasm` to avoid colliding with a user local — meta-HIR identifiers
/// from the supported frontends never start `__wasm`.
const IDX_SCRATCH: &str = "__wasm_idx";

/// PMAT-993: per-function scratch `i32` locals for string construction. The
/// destination base-pointer (`$__wasm_str_dst`) and the first operand's byte
/// length (`$__wasm_str_la`, the write offset of the second operand) of a
/// `Concat`. Like [`IDX_SCRATCH`], declared from the emitted body so they
/// appear exactly when a string-RETURNING op uses them.
const STR_DST_SCRATCH: &str = "__wasm_str_dst";
const STR_LA_SCRATCH: &str = "__wasm_str_la";

/// PMAT-998: the concat DESTINATION base-pointer, held in a local DISTINCT from
/// [`STR_DST_SCRATCH`]. A `Concat`'s operands can THEMSELVES be string-RETURNING
/// ops (`chr(n)`, `s[i]`) whose evaluation `local.set`s `$__wasm_str_dst`; if the
/// concat's destination shared that local it would be CLOBBERED by each operand
/// eval — corrupting later copies and the returned pointer (the bug that made
/// `chr(65) + chr(66)` return the 1-char `"B"` instead of `"AB"`). A dedicated
/// local for the destination survives operand evaluation. (Concats flatten to a
/// single level, so this never self-collides.)
const STR_CONCAT_DST: &str = "__wasm_concat_dst";

/// PMAT-1000: the concat's running WRITE-OFFSET, held in a local DISTINCT from
/// [`STR_LA_SCRATCH`]. `s[i]` (`Expr::StrCharAt`) uses `$__wasm_str_la` as its
/// OWN scratch (the source string's base), so if the concat tracked its offset
/// in that same local, a `StrCharAt` concat operand would CLOBBER the offset —
/// the second half of the concat-operand-aliasing bug ([`STR_CONCAT_DST`] fixed
/// the destination clobber for `chr`; this fixes the offset clobber for `s[i]`).
const STR_CONCAT_OFF: &str = "__wasm_concat_off";

/// PMAT-995 (slice 3b): per-function scratch `i32` local holding a freshly
/// `$__alloc`-ed dict/set base-pointer while [`emit_dict_lit`] writes its
/// header + entries. Body-driven declaration, like the string scratches.
const DICT_DST_SCRATCH: &str = "__wasm_dict_dst";

/// PMAT-996 (slice 4): the `$__alloc`-ed struct base-pointer while
/// [`emit_struct_lit`] writes its fields. Mirrors [`DICT_DST_SCRATCH`].
const STRUCT_DST_SCRATCH: &str = "__wasm_struct_dst";

/// PMAT-996 (slice 4): every field of a struct instance occupies a uniform
/// 8-byte slot on the bump heap, keeping i64/f64 naturally aligned (an i32/
/// f32/bool field uses the low 4 bytes of its slot). Field `i` (definition
/// order) is at `base + i*STRUCT_FIELD_SIZE`.
const STRUCT_FIELD_SIZE: i32 = 8;

/// PMAT-995 (slice 3b): the bump-heap layout of a `dict[K, V]` / `set[E]`.
///
/// A dict/set rides an `i32` base-pointer to a bump-heap region:
///   * header (8 bytes, keeps the entry array 8-aligned):
///       - `i32` live-entry **count** at `base+0` (the same `+0` count header
///         a list/str carries, so `len(d)` reuses the list/str header read),
///       - `i32` slot **capacity** at `base+4` ([`DICT_CAP_OFFSET`]),
///   * then `capacity` fixed entries from `base+8` ([`LIST_ELEMS_OFFSET`]).
///     Each entry is [`DICT_ENTRY_SIZE`] (16) bytes:
///       - the **key** at `entry+0` (an `i64` for an int key; the `i32` string
///         base-pointer in the low 4 bytes for a str key),
///       - the **value** at `entry+`[`DICT_VAL_OFFSET`] (an `i64`; a set stores
///         a `0` sentinel).
///
/// A bump heap cannot realloc, so the capacity is FIXED at construction
/// ([`DICT_GROWTH_SLACK`] spare slots past the literal entries); an insert past
/// capacity TRAPS (`unreachable`) rather than reallocating — an honest
/// bounded-capacity posture, never a silent miscompile (documented on
/// `$__wasm_dict_set_*`).
const DICT_CAP_OFFSET: i32 = 4;
const DICT_ENTRY_SIZE: i32 = 16;
const DICT_VAL_OFFSET: i32 = 8;

/// PMAT-995 (slice 3b): spare entry slots a `DictLit`/`SetLit` over-allocates
/// past its literal entries, so subsequent `d[k] = v` / `s.add(e)` inserts have
/// room in the (realloc-free) bump heap. An insert beyond `literal_count +
/// DICT_GROWTH_SLACK` traps honestly. 16 is room for realistic build-up; a
/// program exceeding it is trapped, not miscompiled.
const DICT_GROWTH_SLACK: i32 = 16;

/// PMAT-995 (slice 3b): the key representation a `dict`/`set` uses, derived from
/// the binding's `Type::Dict(K, _)` / `Type::Set(K)`. Selects the comparison
/// the heap helpers use (an `i64.eq` for an int key; `$__wasm_str_eq` over the
/// stored `i32` string pointers for a str key) — the WASM dict subset's two
/// supported key shapes. Every other key type is refused at binding time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    /// `dict[int, _]` / `set[int]` — an `i64` key compared with `i64.eq`.
    Int,
    /// `dict[str, _]` / `set[str]` — an `i32` string base-pointer compared by
    /// CONTENT via `$__wasm_str_eq` (never a base-pointer `i32.eq`).
    Str,
}

impl KeyKind {
    /// The helper-function suffix (`i` for int keys, `s` for str keys).
    fn suffix(self) -> &'static str {
        match self {
            KeyKind::Int => "i",
            KeyKind::Str => "s",
        }
    }
}

/// PMAT-995: classify a dict/set KEY type, refusing anything outside the
/// int/str key subset. Int keys (`I64`/`CLong`) ride an `i64`; str keys ride an
/// `i32` string base-pointer (content-compared). Bool/float/unsigned/nested
/// keys are refused honestly.
fn dict_key_kind(ty: &Type) -> Result<KeyKind, BackendError> {
    match ty {
        Type::I64 | Type::CLong => Ok(KeyKind::Int),
        Type::Str => Ok(KeyKind::Str),
        other => Err(unsupported(&format!(
            "dict/set key type {other:?} — the WASM dict subset supports int \
             (i64) or str keys only; bool/float/unsigned/nested keys are refused"
        ))),
    }
}

/// PMAT-995: validate a dict VALUE type. The bump-heap dict stores each value in
/// an 8-byte `i64` slot, so the first cut supports the `i64` integer domain
/// (`I64`/`CLong`) only; bool/float/unsigned/str/nested values are refused
/// honestly (no silent width-narrowing or reinterpret).
fn dict_value_is_supported(ty: &Type) -> Result<(), BackendError> {
    match ty {
        Type::I64 | Type::CLong => Ok(()),
        other => Err(unsupported(&format!(
            "dict value type {other:?} — the WASM dict subset stores i64 integer \
             values only (dict[K, int]); bool/float/str/nested values are refused"
        ))),
    }
}

/// PMAT-993 (slice 2): the base linear-memory address of the bump heap.
///
/// String / list params (the static inputs a host preloads via `(data …)`)
/// live at low addresses; the bump heap — where string-RETURNING ops
/// (`a + b`, `chr(n)`) materialise their results — starts above that static
/// region at this fixed, honestly-documented offset. 1024 bytes reserves
/// room for the static inputs in the witness/host layout; the emitter does
/// NOT know how much static data a host preloads, so this is a CONVENTION
/// (the host keeps its `(data …)` inputs below `__HEAP_BASE`). One 64-KiB
/// memory page (declared in `emit_module`) holds both regions.
///
/// PMAT-994 (slice 3a): string LITERALS get their own static `(data …)`
/// segments placed in `[LITERAL_BASE, HEAP_BASE)` — emitter-owned (NOT
/// host-preloaded) so they cannot collide with the heap above, and the host
/// keeps its str/list PARAM inputs below [`LITERAL_BASE`].
const HEAP_BASE: i32 = 1024;

/// PMAT-994 (slice 3a): the base linear-memory address of the EMITTER-OWNED
/// static string-literal region.
///
/// String literals (`Expr::LitStr`, e.g. `"Hello, "`) are materialised at
/// emit time into `(data …)` segments — each a length-prefixed region (i32
/// byte-count header at `base+0`, UTF-8 bytes at `base+8`, the same ABI a str
/// param / heap string uses) — laid down contiguously from this address,
/// 8-byte aligned, BELOW [`HEAP_BASE`]. A `LitStr` then lowers to a constant
/// `i32.const <offset>` base-pointer, so `"Hi " + name`, `return "done"`,
/// literal args, and literal equality all compose with the heap/concat path.
///
/// The literal region `[LITERAL_BASE, HEAP_BASE)` is reserved for the
/// emitter; a host/witness MUST keep its preloaded str/list PARAM inputs
/// below `LITERAL_BASE` (and the emitter refuses if the module's literals
/// overflow `HEAP_BASE`, an honest out-of-room error rather than aliasing the
/// heap). 512 bytes (`[512, 1024)`) is room for realistic literal use; a
/// program exceeding it is refused, not miscompiled.
const LITERAL_BASE: i32 = 512;

/// PMAT-994 (slice 3a): the emit-time layout of a module's distinct string
/// literals into the static `[LITERAL_BASE, HEAP_BASE)` region.
///
/// Built once per module by [`collect_str_literals`]: every DISTINCT
/// `Expr::LitStr` content is assigned a fixed base address (8-byte aligned,
/// deduplicated so `"x"` used twice shares one segment) and laid down as a
/// length-prefixed region (i32 byte-count header at `base+0`, the UTF-8 bytes
/// at `base+8`). [`emit_str_literal_data`] renders the `(data …)` segments;
/// [`Scope::literal_addr`] resolves a `LitStr` to its `i32.const <base>`.
#[derive(Default)]
struct StrLiterals {
    /// `(content, base_addr)` for each distinct literal, in first-seen order.
    entries: Vec<(String, i32)>,
}

impl StrLiterals {
    /// The base address assigned to `content`, or `None` if not laid out
    /// (a module with no string literals).
    fn addr_of(&self, content: &str) -> Option<i32> {
        self.entries
            .iter()
            .find(|(c, _)| c == content)
            .map(|(_, a)| *a)
    }

    /// `true` if any literal was laid out (the module references at least one
    /// `LitStr`) — gates the `(memory …)` + `(data …)` emission.
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// PMAT-994: align `n` up to the next multiple of 8 (the str/list ABI
/// alignment — same `align8` the bump allocator uses).
fn align8(n: i32) -> i32 {
    (n + 7) & !7
}

/// PMAT-994: collect every DISTINCT string literal in `module` and assign each
/// a fixed length-prefixed base address in `[LITERAL_BASE, HEAP_BASE)`.
///
/// Each literal occupies `align8(8 + byte_len)` bytes (the 8-byte count header
/// plus the UTF-8 bytes, rounded up so the next literal stays 8-aligned).
/// Deduplicated by content. Refuses with a hard error if the literals would
/// overflow `HEAP_BASE` (aliasing the bump heap) — an honest out-of-room
/// posture rather than a silent miscompile.
fn collect_str_literals(module: &Module) -> Result<StrLiterals, BackendError> {
    let mut contents: Vec<String> = Vec::new();
    for item in &module.items {
        if let Item::Function(f) = item {
            collect_block_literals(&f.body, &mut contents);
        }
    }
    let mut lits = StrLiterals::default();
    let mut next = LITERAL_BASE;
    for c in contents {
        if lits.addr_of(&c).is_some() {
            continue; // dedup by content — one segment per distinct literal
        }
        let size = align8(LIST_ELEMS_OFFSET + c.len() as i32);
        if next + size > HEAP_BASE {
            return Err(unsupported(&format!(
                "string literals overflow the static region [{LITERAL_BASE}, \
                 {HEAP_BASE}) — this module's literals need more than \
                 {avail} bytes. Static string-literal `(data)` segments are \
                 bounded below the bump heap; refused honestly rather than \
                 aliasing the heap.",
                avail = HEAP_BASE - LITERAL_BASE
            )));
        }
        lits.entries.push((c, next));
        next += size;
    }
    Ok(lits)
}

/// Walk a block collecting `Expr::LitStr` contents (in first-seen order).
fn collect_block_literals(block: &Block, out: &mut Vec<String>) {
    for s in &block.stmts {
        collect_stmt_literals(s, out);
    }
    collect_expr_literals(&block.trailing_return, out);
}

fn collect_stmt_literals(s: &Stmt, out: &mut Vec<String>) {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } => collect_expr_literals(value, out),
        Stmt::Return(e) => collect_expr_literals(e, out),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_expr_literals(cond, out);
            for s in then_body {
                collect_stmt_literals(s, out);
            }
            for s in else_body {
                collect_stmt_literals(s, out);
            }
        }
        Stmt::While { cond, body } => {
            collect_expr_literals(cond, out);
            for s in body {
                collect_stmt_literals(s, out);
            }
        }
        Stmt::IndexAssign { value, .. } => collect_expr_literals(value, out),
        // PMAT-995: `d[k] = v` — a str KEY literal must be laid out too.
        Stmt::DictSet { key, value, .. } => {
            collect_expr_literals(key, out);
            collect_expr_literals(value, out);
        }
        _ => {}
    }
}

fn collect_expr_literals(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::LitStr(s) => out.push(s.clone()),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => {
            collect_expr_literals(lhs, out);
            collect_expr_literals(rhs, out);
        }
        Expr::UnOp { operand, .. } => collect_expr_literals(operand, out),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            collect_expr_literals(cond, out);
            collect_expr_literals(then_expr, out);
            collect_expr_literals(else_expr, out);
        }
        Expr::Call { args, .. } => {
            for a in args {
                collect_expr_literals(a, out);
            }
        }
        Expr::Index { collection, index } => {
            collect_expr_literals(collection, out);
            collect_expr_literals(index, out);
        }
        Expr::Len(c) => collect_expr_literals(c, out),
        Expr::Ord { value } | Expr::Chr { value } => collect_expr_literals(value, out),
        Expr::StrCharAt { string, index } => {
            collect_expr_literals(string, out);
            collect_expr_literals(index, out);
        }
        // PMAT-995: a str-keyed dict/set lays out its KEY string literals into
        // the same deduped static `(data)` table — recurse into the dict/set
        // nodes so `{"x": 1}` / `d["y"]` / `"x" in d` register their literals.
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            collect_expr_literals(dict, out);
            collect_expr_literals(key, out);
        }
        Expr::SetContains { set, elem } => {
            collect_expr_literals(set, out);
            collect_expr_literals(elem, out);
        }
        Expr::DictLit(pairs) => {
            for (k, v) in pairs {
                collect_expr_literals(k, out);
                collect_expr_literals(v, out);
            }
        }
        Expr::SetLit(elems) => {
            for el in elems {
                collect_expr_literals(el, out);
            }
        }
        _ => {}
    }
}

/// PMAT-994: render the `(data …)` segments for the laid-out string literals.
/// Each literal becomes two segments: the i32 byte-count header at its base,
/// and the raw UTF-8 bytes at `base + LIST_ELEMS_OFFSET` — exactly the
/// length-prefixed ABI a str param / heap string uses, so a literal pointer
/// composes uniformly with `len`/`ord`/`Concat`/equality.
fn emit_str_literal_data(lits: &StrLiterals) -> String {
    let mut out = String::new();
    for (content, base) in &lits.entries {
        let bytes = content.as_bytes();
        writeln!(
            out,
            "  ;; PMAT-994 string literal {content:?} @ {base} (len {})",
            bytes.len()
        )
        .expect("write");
        // i32 byte-count header (little-endian) at base+0.
        write!(out, "  (data (i32.const {base}) \"").expect("write");
        for b in (bytes.len() as i32).to_le_bytes() {
            write!(out, "\\{b:02x}").expect("write");
        }
        writeln!(out, "\")").expect("write");
        // raw UTF-8 bytes at base+LIST_ELEMS_OFFSET.
        write!(out, "  (data (i32.const {}) \"", base + LIST_ELEMS_OFFSET).expect("write");
        for &b in bytes {
            write!(out, "\\{b:02x}").expect("write");
        }
        writeln!(out, "\")").expect("write");
    }
    out
}

/// PMAT-994 (slice 3a): a WAT helper that compares two length-prefixed strings
/// for content equality (Python `a == b` over `str`), returning an `i32`
/// boolean (1 = equal, 0 = not).
///
/// `$__wasm_str_eq(a, b)` first compares the i32 byte-count headers; on a
/// length mismatch it returns 0 immediately. On equal lengths it byte-compares
/// the UTF-8 payloads (`base+8 …`) in a loop, returning 0 on the first
/// differing byte and 1 if all bytes match (including two empty strings). This
/// is real string-CONTENT logic — never a base-pointer compare — so it is
/// correct for literals, str params, and heap strings uniformly (all share the
/// length-prefixed ABI). Emitted once per module (gated on
/// [`module_needs_str_eq`]).
const STR_EQ_HELPER: &str = "\
  ;; __wasm_str_eq(a, b) = (content of a) == (content of b)  (Python str ==)
  ;; a, b are i32 base-pointers to length-prefixed regions (i32 count @ base+0,
  ;; UTF-8 bytes @ base+8). Returns i32 1 if equal, 0 otherwise.
  (func $__wasm_str_eq (param $a i32) (param $b i32) (result i32)
    (local $n i32)
    (local $i i32)
    ;; if len(a) != len(b) return 0
    local.get $a
    i32.load
    local.get $b
    i32.load
    i32.ne
    if
      i32.const 0
      return
    end
    ;; n = len(a)  (== len(b))
    local.get $a
    i32.load
    local.set $n
    ;; i = 0; while i < n: if a[8+i] != b[8+i] return 0; i += 1
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        local.get $i
        local.get $n
        i32.ge_s
        br_if $done
        ;; a byte i
        local.get $a
        i32.const 8
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        ;; b byte i
        local.get $b
        i32.const 8
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        i32.ne
        if
          i32.const 0
          return
        end
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    i32.const 1
  )
";

/// PMAT-993: name of the mutable global holding the bump pointer (the next
/// free heap address). Initialised to [`HEAP_BASE`]; advanced 8-byte-aligned
/// by `$__alloc`.
const HEAP_PTR_GLOBAL: &str = "__heap_ptr";

/// PMAT-993 (slice 2): the linear-memory bump allocator, in WAT.
///
/// A bump-only heap — NO free (documented; slice 2's deliverable is the
/// enabling primitive, not a general allocator). `$__alloc(n)` returns the
/// current bump pointer and advances it by `align8(n)` so every allocation
/// (string headers, `i64`/`f64` payloads) stays 8-byte aligned. The heap
/// lives above the static `(data …)` region (see [`HEAP_BASE`]) in the same
/// single memory page; a host/witness preloads its inputs below `HEAP_BASE`
/// and reads constructed strings back from the returned pointer.
///
/// This intentionally does NOT call `memory.grow`: the module pre-sizes one
/// page (64 KiB), which bounds slice-2 string construction honestly. A heap
/// that outgrows the page traps (an out-of-memory analogue) rather than
/// silently corrupting — growth is a clean follow-up.
///
/// Emitted once per module (gated on [`module_needs_heap`]); the bump pointer
/// is initialised to [`HEAP_BASE`].
fn heap_helpers() -> String {
    format!(
        "\
  ;; PMAT-993 bump heap: $__heap_ptr is the next free address (8-aligned),
  ;; initialised past the static (data) inputs at __HEAP_BASE = {HEAP_BASE}.
  (global ${HEAP_PTR_GLOBAL} (mut i32) (i32.const {HEAP_BASE}))
  ;; __alloc(n) = current bump pointer; advance it by align8(n). Bump-only
  ;; (no free). Returns the allocation's base address.
  (func $__alloc (param $n i32) (result i32)
    (local $base i32)
    global.get ${HEAP_PTR_GLOBAL}
    local.set $base
    ;; __heap_ptr = base + ((n + 7) & ~7)   (round the size up to 8 bytes)
    local.get $base
    local.get $n
    i32.const 7
    i32.add
    i32.const -8
    i32.and
    i32.add
    global.set ${HEAP_PTR_GLOBAL}
    local.get $base
  )
"
    )
}

/// PMAT-995 (slice 3b): the `dict`/`set` heap helper functions, in WAT.
///
/// A dict/set is an open assoc-array over the bump heap (see [`DICT_CAP_OFFSET`]
/// for the layout). Three helpers per key kind do a LINEAR scan of the `count`
/// live entries:
///   * `$__wasm_dict_get_<k>(p, key) -> i64` — Python `d[k]`: returns the value
///     or `unreachable`-TRAPS on an absent key (the `KeyError` analogue,
///     mirroring the list-index / dict Rust-`HashMap`-panic posture);
///   * `$__wasm_dict_has_<k>(p, key) -> i32` — Python `k in d`: 1 if present,
///     0 otherwise (never traps);
///   * `$__wasm_dict_set_<k>(p, key, val)` — Python `d[k] = v` / `s.add(e)`:
///     updates an existing key in place, else appends at `count` (incrementing
///     it). An append past `capacity` TRAPS (`unreachable`) — the bump heap
///     cannot realloc, so capacity is bounded; an honest trap, never a
///     miscompile.
///
/// `<k>` is `i` (int keys: an `i64` key compared with `i64.eq`) or `s` (str
/// keys: an `i32` string base-pointer compared by CONTENT via `$__wasm_str_eq`
/// — so the str-key set FORCES `$__wasm_str_eq` to be emitted). Emitted once per
/// module, gated on [`module_dict_key_kinds`]; a set reuses the `has`/`set`
/// helpers (a set never `get`s — you cannot subscript a Python set).
fn dict_helpers(need_int: bool, need_str: bool) -> String {
    let mut out = String::new();
    if need_int {
        out.push_str(&dict_helpers_for(KeyKind::Int));
    }
    if need_str {
        out.push_str(&dict_helpers_for(KeyKind::Str));
    }
    out
}

/// The shared linear-scan prologue: declare scratch locals, set `$n = count`,
/// `$i = 0`, open `(block $done (loop $next …))`, bounds-exit at `i >= n`, and
/// compute `$ea = p + LIST_ELEMS_OFFSET + i*DICT_ENTRY_SIZE` (entry `i`'s
/// address). After it, the caller emits the per-op key compare + match body.
fn emit_dict_scan_prologue(out: &mut String) {
    for line in [
        "    (local $i i32) (local $n i32) (local $ea i32)",
        "    local.get $p",
        "    i32.load",
        "    local.set $n",
        "    i32.const 0",
        "    local.set $i",
        "    (block $done",
        "      (loop $next",
        "        local.get $i",
        "        local.get $n",
        "        i32.ge_s",
        "        br_if $done",
        "        local.get $p",
    ] {
        writeln!(out, "{line}").expect("write");
    }
    writeln!(out, "        i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const {DICT_ENTRY_SIZE}").expect("write");
    for line in [
        "        i32.mul",
        "        i32.add",
        "        local.set $ea",
    ] {
        writeln!(out, "{line}").expect("write");
    }
}

/// Push entry `$ea`'s key and compare it with `$k`, leaving an i32 bool on the
/// stack — `i64.eq` for an int key, `$__wasm_str_eq` (CONTENT) for a str key.
fn emit_dict_key_compare(out: &mut String, kind: KeyKind) {
    writeln!(out, "        local.get $ea").expect("write");
    match kind {
        KeyKind::Int => {
            writeln!(out, "        i64.load").expect("write");
            writeln!(out, "        local.get $k").expect("write");
            writeln!(out, "        i64.eq").expect("write");
        }
        KeyKind::Str => {
            writeln!(out, "        i32.load").expect("write");
            writeln!(out, "        local.get $k").expect("write");
            writeln!(out, "        call $__wasm_str_eq").expect("write");
        }
    }
}

/// The loop step + `loop`/`block` close, after the per-op match body. After it
/// the caller emits the not-found tail.
fn emit_dict_scan_epilogue(out: &mut String) {
    for line in [
        "        local.get $i",
        "        i32.const 1",
        "        i32.add",
        "        local.set $i",
        "        br $next",
        "      )",
        "    )",
    ] {
        writeln!(out, "{line}").expect("write");
    }
}

/// Store `$k` as entry `$ea`'s key — `i64.store` for an int key, `i32.store`
/// (the low 4 bytes; the slot's high half stays 0 from the zero-init heap and is
/// never read) for a str-key pointer.
fn emit_dict_store_key(out: &mut String, kind: KeyKind) {
    writeln!(out, "    local.get $ea").expect("write");
    writeln!(out, "    local.get $k").expect("write");
    match kind {
        KeyKind::Int => writeln!(out, "    i64.store").expect("write"),
        KeyKind::Str => writeln!(out, "    i32.store").expect("write"),
    }
}

/// Emit the three heap helpers for one [`KeyKind`].
fn dict_helpers_for(kind: KeyKind) -> String {
    let s = kind.suffix();
    let kparam = match kind {
        KeyKind::Int => "i64",
        KeyKind::Str => "i32",
    };
    let mut out = String::new();

    // get: return the value at a matching key, else trap (KeyError analogue).
    writeln!(
        out,
        "  ;; __wasm_dict_get_{s}(p, key) = d[key]; traps (unreachable) if absent (KeyError)"
    )
    .expect("write");
    writeln!(
        out,
        "  (func $__wasm_dict_get_{s} (param $p i32) (param $k {kparam}) (result i64)"
    )
    .expect("write");
    emit_dict_scan_prologue(&mut out);
    emit_dict_key_compare(&mut out, kind);
    writeln!(out, "        if").expect("write");
    writeln!(out, "          local.get $ea").expect("write");
    writeln!(out, "          i64.load offset={DICT_VAL_OFFSET}").expect("write");
    writeln!(out, "          return").expect("write");
    writeln!(out, "        end").expect("write");
    emit_dict_scan_epilogue(&mut out);
    writeln!(out, "    unreachable").expect("write");
    writeln!(out, "  )").expect("write");

    // has: 1 if a key matches, else 0 (never traps) — Python `k in d`.
    writeln!(out, "  ;; __wasm_dict_has_{s}(p, key) = (key in d) ? 1 : 0").expect("write");
    writeln!(
        out,
        "  (func $__wasm_dict_has_{s} (param $p i32) (param $k {kparam}) (result i32)"
    )
    .expect("write");
    emit_dict_scan_prologue(&mut out);
    emit_dict_key_compare(&mut out, kind);
    writeln!(out, "        if").expect("write");
    writeln!(out, "          i32.const 1").expect("write");
    writeln!(out, "          return").expect("write");
    writeln!(out, "        end").expect("write");
    emit_dict_scan_epilogue(&mut out);
    writeln!(out, "    i32.const 0").expect("write");
    writeln!(out, "  )").expect("write");

    // set: update an existing key in place, else append at count (trap if at
    // capacity — the realloc-free bump heap's honest bound).
    writeln!(
        out,
        "  ;; __wasm_dict_set_{s}(p, key, val): update-or-insert (d[key] = val)."
    )
    .expect("write");
    writeln!(
        out,
        "  ;; Appends at count; TRAPS (unreachable) if at capacity (no realloc)."
    )
    .expect("write");
    writeln!(
        out,
        "  (func $__wasm_dict_set_{s} (param $p i32) (param $k {kparam}) (param $v i64)"
    )
    .expect("write");
    emit_dict_scan_prologue(&mut out);
    emit_dict_key_compare(&mut out, kind);
    writeln!(out, "        if").expect("write");
    writeln!(out, "          local.get $ea").expect("write");
    writeln!(out, "          local.get $v").expect("write");
    writeln!(out, "          i64.store offset={DICT_VAL_OFFSET}").expect("write");
    writeln!(out, "          return").expect("write");
    writeln!(out, "        end").expect("write");
    emit_dict_scan_epilogue(&mut out);
    // not found → append at slot n; trap if at capacity.
    writeln!(out, "    local.get $n").expect("write");
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    i32.load offset={DICT_CAP_OFFSET}").expect("write");
    writeln!(out, "    i32.ge_s").expect("write");
    writeln!(out, "    if").expect("write");
    writeln!(out, "      unreachable").expect("write");
    writeln!(out, "    end").expect("write");
    // $ea = p + LIST_ELEMS_OFFSET + n*DICT_ENTRY_SIZE
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "    i32.add").expect("write");
    writeln!(out, "    local.get $n").expect("write");
    writeln!(out, "    i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "    i32.mul").expect("write");
    writeln!(out, "    i32.add").expect("write");
    writeln!(out, "    local.set $ea").expect("write");
    emit_dict_store_key(&mut out, kind);
    writeln!(out, "    local.get $ea").expect("write");
    writeln!(out, "    local.get $v").expect("write");
    writeln!(out, "    i64.store offset={DICT_VAL_OFFSET}").expect("write");
    // count = n + 1
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    local.get $n").expect("write");
    writeln!(out, "    i32.const 1").expect("write");
    writeln!(out, "    i32.add").expect("write");
    writeln!(out, "    i32.store").expect("write");
    writeln!(out, "  )").expect("write");

    out
}

/// Python floor-division and floor-modulo helper functions, in WAT.
///
/// WASM `i64.div_s` truncates toward zero; Python `//` floors toward −∞.
/// The floor quotient is the truncating quotient minus one when the
/// truncating remainder is non-zero AND its sign differs from the
/// divisor's sign. The floor remainder is `a - b*q_floor`, which carries
/// the divisor's sign (Python's `%` posture). Both trap on a zero divisor
/// (`i64.div_s`/`i64.rem_s` trap on 0 — the ZeroDivisionError analogue).
const FLOOR_HELPERS: &str = "\
  ;; __wasm_floordiv_i64(a, b) = floor(a / b)  (Python //)
  (func $__wasm_floordiv_i64 (param $a i64) (param $b i64) (result i64)
    (local $q i64)
    (local $r i64)
    local.get $a
    local.get $b
    i64.div_s
    local.set $q
    local.get $a
    local.get $b
    i64.rem_s
    local.set $r
    ;; if r != 0 && ((r < 0) != (b < 0)) then q - 1 else q
    local.get $r
    i64.const 0
    i64.ne
    local.get $r
    i64.const 0
    i64.lt_s
    local.get $b
    i64.const 0
    i64.lt_s
    i32.ne
    i32.and
    if (result i64)
      local.get $q
      i64.const 1
      i64.sub
    else
      local.get $q
    end
  )
  ;; __wasm_floormod_i64(a, b) = a - b * floordiv(a, b)  (Python %)
  (func $__wasm_floormod_i64 (param $a i64) (param $b i64) (result i64)
    local.get $a
    local.get $a
    local.get $b
    call $__wasm_floordiv_i64
    local.get $b
    i64.mul
    i64.sub
  )
";

/// Native WASM backend. Lowers the meta-HIR scalar/control subset to WAT
/// text. `Backend` impl (single-emitter — no §29 quorum at this slice;
/// the executed two-emitter `WasmDiffExecEngine` witness is deferred to
/// PMAT-952).
pub struct WasmBackend {
    inner: WasmBackendInner,
}

/// Internal dispatch: the v0.1.0 single real meta-HIR lowering, or the
/// PMAT-952 two-emitter executed-witness quorum.
enum WasmBackendInner {
    /// The real meta-HIR → WAT lowering (PMAT-951 EMIT half).
    Single,
    /// PMAT-952 executed-witness quorum (two categorically-independent WAT
    /// saxpy emitters under `QuorumPolicy::DiffExec`).
    DiffExecWitness(MultiEmitterBackend),
}

impl Default for WasmBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmBackend {
    pub fn new() -> Self {
        Self {
            inner: WasmBackendInner::Single,
        }
    }

    /// PMAT-952 (runtime-witness half) — the executed WASM-runtime
    /// DiffExec-witness constructor (§29).
    ///
    /// Sibling of [`xpile_ptx_codegen::PtxBackend::new_cuda_diffexec_witness`]
    /// and [`xpile_wgsl_codegen::WgslBackend::new_wgpu_diffexec_witness`].
    /// Builds a `WasmBackend` whose `MultiEmitterBackend` carries two REAL
    /// WAT emitters — [`WasmSaxpyGeneralEmitter`] (general) and
    /// [`WasmSaxpySpecialistEmitter`] (specialist) — under
    /// `QuorumPolicy::DiffExec`, with a [`WasmDiffExecEngine`] installed.
    /// Both emitters compute the same semantics (`out[i] = 2*in[i] + 1`
    /// over [`FIXTURE_INPUT`]) via *categorically different* WAT (one an
    /// explicit `f64.mul` + `f64.add`, one a reassociated `(x + x) + 1` with
    /// no `f64.mul` opcode at all), so the `DiffExec` quorum runs BOTH in a
    /// wasm runtime and asserts they agree — the falsifying multi-emitter
    /// check the §29 design specs, here in a WebAssembly interpreter rather
    /// than on a GPU.
    ///
    /// On a host with WABT (`wat2wasm` + `wasm-interp`) this produces a real
    /// [`xpile_backend::DiffExecResult::Match`] instead of the
    /// `NotRun { no-engine }` placeholder — the runtime-stratum upgrade of
    /// `C-COMPILE-RUST-TO-WASM` (deferred from PMAT-951's EMIT half).
    ///
    /// On a host with no WABT the engine is NOT installed (the
    /// `MultiEmitterBackend` keeps `diff_exec_engine = None`), so the
    /// backend records the benign `NotRun { no-engine }` and free CI stays
    /// green — the `nvcc`/wgpu/cc graceful-skip posture.
    pub fn new_wasm_diffexec_witness() -> Self {
        let mut inner = MultiEmitterBackend::new_with_specialist(
            Target::Wasm,
            Box::new(WasmSaxpyGeneralEmitter),
            Box::new(WasmSaxpySpecialistEmitter),
            QuorumPolicy::DiffExec { tolerance: 1.0e-9 },
        );
        if wasm_runtime_available() {
            inner = inner.with_diff_exec_engine(std::sync::Arc::new(WasmDiffExecEngine::new()));
        }
        Self {
            inner: WasmBackendInner::DiffExecWitness(inner),
        }
    }
}

impl Backend for WasmBackend {
    fn name(&self) -> &'static str {
        "wasm"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Wasm]
    }

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        if config.target != Target::Wasm {
            return Err(BackendError::UnsupportedTarget(config.target));
        }
        match &self.inner {
            WasmBackendInner::Single => {
                let wat = emit_module(module)?;
                Ok(Artifact {
                    primary: wat,
                    sidecars: Vec::new(),
                    citations: vec![ContractId::new(CONTRACT_ID)],
                    quorum_status: QuorumStatus::Single {
                        emitter: "xpile-wasm-codegen".to_string(),
                    },
                })
            }
            WasmBackendInner::DiffExecWitness(inner) => inner.lower(module, config),
        }
    }
}

// ─── PMAT-952/976: WAT emitters for the executed WASM-runtime witness ──
//
// Two emitters that produce COMPLETE, wat2wasm-assemblable WAT modules
// computing identical semantics — `out[i] = 2*in[i] + 1` over
// [`FIXTURE_INPUT`] — via categorically different lowerings. Each module
// exports one zero-arg `f64`-returning function per fixture element
// (`e0`..`eN`), so `wasm-interp --run-all-exports` runs the whole vector
// and prints each result.
//
// PMAT-976 (witness integrity): the GENERAL side no longer hand-writes WAT
// — it drives xpile's REAL meta-HIR → WAT emitter via
// [`wasm_diffexec::general_module_wat`] (`(x * 2.0) + 1.0`, an explicit
// `FloatOp::Mul` then `FloatOp::Add`, lowered through `emit_module`). The
// SPECIALIST side stays the hand-written reassociated `(x + x) + 1` with no
// multiply opcode (the categorically-independent trusted oracle). Both are
// run in the wasm runtime under the `DiffExec` quorum (see
// [`WasmBackend::new_wasm_diffexec_witness`]); the engine asserts their
// executed outputs agree within tolerance — so the witness now proves
// `meta-HIR → xpile WAT emit → assemble → run → correct` for the general
// side, not `hardcoded WAT → run`.

/// Format an `f64` as a WAT `f64.const` literal. Rust's `{:?}` always
/// emits a decimal point (e.g. `2.0`, `-0.5`, `100.0`), which `wat2wasm`
/// accepts as an `f64` literal.
fn wat_f64(v: f64) -> String {
    format!("{v:?}")
}

/// Build a WAT module exporting `e0`..`eN`, one per [`FIXTURE_INPUT`]
/// element, each computing `2*x + 1` via the instruction sequence
/// `body(x)` produces (given the fixed input value already on no stack —
/// `body` is responsible for pushing the constant(s) and computing).
fn saxpy_module(comment: &str, body: impl Fn(f64) -> String) -> String {
    let mut out = String::new();
    out.push_str("(module\n");
    out.push_str(&format!("  ;; {comment}\n"));
    out.push_str(&format!("  ;; xpile-contract: {CONTRACT_ID}\n"));
    for (i, &x) in FIXTURE_INPUT.iter().enumerate() {
        out.push_str(&format!(
            "  (func (export \"e{i}\") (result f64)\n    {})\n",
            body(x)
        ));
    }
    out.push_str(")\n");
    out
}

/// General WAT emitter — `2*x + 1` via an explicit `f64.mul` then
/// `f64.add`. Emits a complete wat2wasm-assemblable module.
///
/// PMAT-976 (witness integrity): this side no longer hand-writes WAT. It
/// drives xpile's REAL meta-HIR → WAT emitter via
/// [`wasm_diffexec::general_module_wat`] (a structured meta-HIR module of
/// zero-arg `eN` functions, each `(x * 2.0) + 1.0`, lowered through the SAME
/// [`emit_module`] the single-emitter `lower` path uses). So the executed
/// `DiffExec` quorum the backend actually runs now proves
/// `meta-HIR → xpile WAT emit → assemble → run → correct` for the general
/// side, not `hardcoded WAT → run`. The specialist side stays hand-written
/// (the categorically-independent trusted oracle).
struct WasmSaxpyGeneralEmitter;

impl TargetEmitter for WasmSaxpyGeneralEmitter {
    fn name(&self) -> &str {
        "wasm-saxpy-general"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        if config.target != Target::Wasm {
            return Some(Err(BackendError::UnsupportedTarget(config.target)));
        }
        // REAL emit path: meta-HIR → `emit_module` → WAT (PMAT-976).
        let primary = wasm_diffexec::general_module_wat();
        Some(Ok(EmittedText {
            primary,
            citations: vec![ContractId::new(CONTRACT_ID)],
        }))
    }
}

/// Specialist WAT emitter — same semantics (`2*x + 1`) computed via a
/// reassociated `(x + x) + 1`, with NO `f64.mul` opcode. A categorically
/// independent lowering: the `DiffExec` quorum runs both in the wasm
/// runtime and falsifies the contract if they diverge.
struct WasmSaxpySpecialistEmitter;

impl TargetEmitter for WasmSaxpySpecialistEmitter {
    fn name(&self) -> &str {
        "wasm-saxpy-specialist-doubling"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        if config.target != Target::Wasm {
            return Some(Err(BackendError::UnsupportedTarget(config.target)));
        }
        let primary = saxpy_module(
            "wasm-saxpy-specialist: out = (x + x) + 1.0 (reassociated doubling, no f64.mul)",
            |x| {
                // (x + x) + 1.0
                format!(
                    "f64.const {x}\n    f64.const {x}\n    f64.add\n    f64.const 1.0\n    f64.add",
                    x = wat_f64(x)
                )
            },
        );
        Some(Ok(EmittedText {
            primary,
            citations: vec![ContractId::new(CONTRACT_ID)],
        }))
    }
}

// ─── WAT emission ───────────────────────────────────────────────────

/// Emit a full `(module …)` for `module`. Only [`Item::Function`]s are
/// emitted; any other item kind is refused (no struct/enum/const in the
/// scalar/control subset).
pub fn emit_module(module: &Module) -> Result<String, BackendError> {
    let mut out = String::new();
    writeln!(out, "(module").expect("write to String");
    writeln!(
        out,
        "  ;; xpile-wasm-codegen — native WAT (scalar/control subset)"
    )
    .expect("write");
    writeln!(out, "  ;; source module: {}", module.name).expect("write");
    writeln!(out, "  ;; contract: {CONTRACT_ID}").expect("write");
    // PMAT-966/968: when any function takes a `list[scalar]` parameter, the
    // list rides an `i32` base-pointer into WASM linear memory. The pointed
    // region is a length-prefixed layout — an `i32` element count at
    // `base+0`, then the packed elements from `base+8` (PMAT-968). `xs[i]`
    // lowers to a bounds-checked `*.load`, and `len(xs)` reads the header.
    // Declare one page (64 KiB) of memory once, up front, and export it as
    // `mem` so a host/witness can populate it (count + elements) before the
    // call.
    // PMAT-993 (slice 2): a module needs the bump heap when ANY function
    // performs a string-RETURNING op (`a + b`, `chr(n)`) — those materialise
    // a new length-prefixed string in linear memory. Such a module needs the
    // `(memory …)` even with no list/str PARAMETER (e.g. a `chr(n)`-only fn).
    // PMAT-994 (slice 3a): string LITERALS are materialised at emit time into
    // static `(data …)` segments in `[LITERAL_BASE, HEAP_BASE)` — that region
    // (and thus the `(memory …)`) is needed whenever the module references any
    // literal. `s[i]` as a 1-char string also pulls in the heap (it allocates
    // a 1-char string like `chr`), folded into `module_needs_heap`.
    // PMAT-995 (slice 3b): a `dict`/`set` rides the bump heap too — a `DictLit`/
    // `SetLit` allocates a `[count][cap]`-headed entry array via `$__alloc`, so
    // a module with any dict/set needs the `(memory …)` + allocator. The key
    // kinds drive which `$__wasm_dict_*_<k>` helper set is emitted; a str-keyed
    // dict additionally forces `$__wasm_str_eq` (its key compare is a content
    // compare).
    let (dict_int_keys, dict_str_keys) = module_dict_key_kinds(module);
    let needs_dict = dict_int_keys || dict_str_keys;
    let needs_heap = module_needs_heap(module);
    let literals = collect_str_literals(module)?;
    // PMAT-996 (slice 4): the module's struct layout registry (name → fields),
    // built once; non-scalar-field structs are refused here (honest early error).
    let structs = build_struct_registry(module)?;
    let needs_str_eq = module_needs_str_eq(module) || dict_str_keys;
    if module_uses_list_param(module) || needs_heap || !literals.is_empty() || needs_str_eq {
        writeln!(
            out,
            "  ;; PMAT-968/986: list AND str params are an i32 base-pointer to \
             a length-prefixed region (i32 count @ base+0, elements/bytes @ \
             base+8) in this linear memory"
        )
        .expect("write");
        if needs_heap {
            writeln!(
                out,
                "  ;; PMAT-993: string-RETURNING ops (a + b, chr(n), s[i]) \
                 bump-allocate their result here too (heap above the static \
                 inputs at __HEAP_BASE)"
            )
            .expect("write");
        }
        if !literals.is_empty() {
            writeln!(
                out,
                "  ;; PMAT-994: string LITERALS are static (data) segments in \
                 [{LITERAL_BASE}, {HEAP_BASE}) (below the bump heap)"
            )
            .expect("write");
        }
        writeln!(out, "  (memory (export \"mem\") 1)").expect("write");
    }
    // PMAT-994: lay down the static string-literal `(data …)` segments (each a
    // length-prefixed region in `[LITERAL_BASE, HEAP_BASE)`), so a `LitStr`
    // can lower to a constant `i32.const <base>` pointer.
    if !literals.is_empty() {
        out.push_str(&emit_str_literal_data(&literals));
    }
    // PMAT-993: emit the bump allocator (a mutable `$__heap_ptr` global +
    // `$__alloc`) once, when the module materialises any new string. Gated on
    // `needs_heap` so a scalar/list/read-only-str module carries no allocator.
    if needs_heap {
        out.push_str(&heap_helpers());
    }
    // PMAT-994: emit the string-equality helper once, when any function
    // compares two strings for content equality (`a == b` over `str`). Also
    // pulled in by a str-keyed dict (PMAT-995: its key compare is a content
    // compare via this helper) — `needs_str_eq` folds that in above.
    if needs_str_eq {
        out.push_str(STR_EQ_HELPER);
    }
    // PMAT-995 (slice 3b): emit the dict/set heap helpers (get/has/set per key
    // kind) once, when any function builds a dict/set. After `$__wasm_str_eq`
    // (the str-key helpers call it).
    if needs_dict {
        out.push_str(&dict_helpers(dict_int_keys, dict_str_keys));
    }
    // Emit the Python floor-division / floor-modulo helpers once. WASM
    // `i64.div_s` truncates toward zero and `i64.rem_s` is the truncating
    // remainder; Python's `//`/`%` floor toward −∞ with the remainder
    // taking the divisor's sign. These helpers apply the same floor
    // correction the Rust lane uses (PMAT-538), and trap on a zero
    // divisor (`i64.div_s`/`i64.rem_s` already trap on 0, matching the
    // Python ZeroDivisionError posture).
    out.push_str(FLOOR_HELPERS);
    for item in &module.items {
        match item {
            Item::Function(f) => {
                let f_wat = emit_function(f, &literals, &structs)?;
                out.push_str(&f_wat);
            }
            Item::Const { name, .. } => {
                return Err(unsupported(&format!(
                    "module-level const `{name}` (only scalar/control functions are in the WASM subset)"
                )));
            }
            // PMAT-996 (slice 4): a struct DEFINITION emits no WAT — it is pure
            // layout (recorded in `structs`); its instances are lowered at their
            // `StructLit`/`FieldAccess` use sites. Non-scalar-field structs were
            // already refused by `build_struct_registry`.
            Item::Struct { .. } => {}
            Item::Enum { name, .. } => {
                return Err(unsupported(&format!(
                    "enum `{name}` (outside the WASM scalar/control subset)"
                )));
            }
        }
    }
    writeln!(out, ")").expect("write");
    Ok(out)
}

/// `true` when any function in `module` takes a `list[...]` OR a `str`
/// parameter — the trigger for emitting the `(memory …)` declaration
/// (PMAT-966 for lists, PMAT-986 for strings). Both ride an `i32`
/// base-pointer into that linear memory under the same length-prefixed ABI.
fn module_uses_list_param(module: &Module) -> bool {
    module.items.iter().any(|item| match item {
        Item::Function(f) => f
            .params
            .iter()
            // PMAT-996: a struct param is also an i32 base-pointer into linear
            // memory, so it likewise needs the `(memory …)` declaration.
            .any(|p| matches!(p.ty, Type::List(_) | Type::Str | Type::Struct(_))),
        _ => false,
    })
}

/// PMAT-993: `true` when any function in `module` MATERIALISES a new string
/// in linear memory — a string-RETURNING op (`Expr::Concat` string `+`,
/// `Expr::Chr`, or PMAT-994 `Expr::StrCharAt` as a 1-char string) or a `str`
/// RETURN type — so the module needs the bump heap (`$__heap_ptr` +
/// `$__alloc`). A purely read-only string / scalar / list module returns
/// `false` and carries no allocator (the slice-1 posture).
fn module_needs_heap(module: &Module) -> bool {
    let (di, ds) = module_dict_key_kinds(module);
    di || ds
        || module.items.iter().any(|item| match item {
            Item::Function(f) => matches!(f.return_type, Type::Str) || block_has_heap_op(&f.body),
            _ => false,
        })
}

/// PMAT-995 (slice 3b): which dict/set KEY kinds the module uses — `(needs_int,
/// needs_str)` — by scanning every `Let` whose annotated type is a
/// `Type::Dict(K, _)` or `Type::Set(K)`. Drives which `$__wasm_dict_*_<k>`
/// helper set is emitted; a str-keyed dict additionally forces `$__wasm_str_eq`.
///
/// Only LET-bound dicts/sets are scanned: a dict/set is materialised in-function
/// via a `DictLit`/`SetLit` bound to a local (a dict/set PARAMETER rides no
/// host-preload ABI in the WASM subset and is refused by `param_wat_type`). An
/// unsupported key/value type does not panic here — it is refused later, at
/// binding lowering, with a precise diagnostic.
fn module_dict_key_kinds(module: &Module) -> (bool, bool) {
    let mut need_int = false;
    let mut need_str = false;
    for item in &module.items {
        if let Item::Function(f) = item {
            scan_block_dict_kinds(&f.body, &mut need_int, &mut need_str);
        }
    }
    (need_int, need_str)
}

fn scan_block_dict_kinds(block: &Block, need_int: &mut bool, need_str: &mut bool) {
    scan_stmts_dict_kinds(&block.stmts, need_int, need_str);
}

fn scan_stmts_dict_kinds(stmts: &[Stmt], need_int: &mut bool, need_str: &mut bool) {
    for s in stmts {
        match s {
            Stmt::Let { ty, .. } => {
                let key_ty = match ty {
                    Type::Dict(k, _) | Type::Set(k) => Some(k.as_ref()),
                    _ => None,
                };
                if let Some(k) = key_ty {
                    match dict_key_kind(k) {
                        Ok(KeyKind::Int) => *need_int = true,
                        Ok(KeyKind::Str) => *need_str = true,
                        Err(_) => {} // refused later at binding lowering
                    }
                }
            }
            Stmt::While { body, .. } => scan_stmts_dict_kinds(body, need_int, need_str),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                scan_stmts_dict_kinds(then_body, need_int, need_str);
                scan_stmts_dict_kinds(else_body, need_int, need_str);
            }
            _ => {}
        }
    }
}

/// PMAT-994: `true` when any function in `module` compares two strings for
/// content equality (`a == b` / `a != b` over `str` operands) — the trigger
/// for emitting the `$__wasm_str_eq` helper. A binop `Eq`/`NotEq` whose operand
/// is a string-VALUED expression (a str param `Ident`, a literal, a `Concat`,
/// or a `Chr`) needs the content-compare helper. The str-param set is computed
/// per-function so `str_param == str_param` (the common case) is detected.
fn module_needs_str_eq(module: &Module) -> bool {
    module.items.iter().any(|item| match item {
        Item::Function(f) => {
            let str_params: Vec<&str> = f
                .params
                .iter()
                .filter(|p| matches!(p.ty, Type::Str))
                .map(|p| p.name.as_str())
                .collect();
            block_has_str_eq(&f.body, &str_params)
        }
        _ => false,
    })
}

fn block_has_str_eq(block: &Block, str_params: &[&str]) -> bool {
    block.stmts.iter().any(|s| stmt_has_str_eq(s, str_params))
        || expr_has_str_eq(&block.trailing_return, str_params)
}

fn stmt_has_str_eq(s: &Stmt, str_params: &[&str]) -> bool {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } => expr_has_str_eq(value, str_params),
        Stmt::Return(e) => expr_has_str_eq(e, str_params),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_has_str_eq(cond, str_params)
                || then_body.iter().any(|s| stmt_has_str_eq(s, str_params))
                || else_body.iter().any(|s| stmt_has_str_eq(s, str_params))
        }
        Stmt::While { cond, body } => {
            expr_has_str_eq(cond, str_params) || body.iter().any(|s| stmt_has_str_eq(s, str_params))
        }
        Stmt::IndexAssign { value, .. } => expr_has_str_eq(value, str_params),
        _ => false,
    }
}

/// `true` if `e` (or any sub-expression) is a string-valued `==`/`!=` — a
/// content comparison the `$__wasm_str_eq` helper backs. A binop is a str
/// equality iff its op is `Eq`/`NotEq` and either operand is a string-valued
/// `Expr`: a `LitStr` / `Concat` / `Chr` / bare `StrCharAt` (structural), or a
/// str-param `Ident` (looked up in `str_params`).
fn expr_has_str_eq(e: &Expr, str_params: &[&str]) -> bool {
    match e {
        Expr::BinOp { op, lhs, rhs } => {
            (matches!(op, BinOp::Eq | BinOp::NotEq)
                && (expr_is_str_valued(lhs, str_params) || expr_is_str_valued(rhs, str_params)))
                || expr_has_str_eq(lhs, str_params)
                || expr_has_str_eq(rhs, str_params)
        }
        Expr::FloatBinOp { lhs, rhs, .. } | Expr::Concat { lhs, rhs } => {
            expr_has_str_eq(lhs, str_params) || expr_has_str_eq(rhs, str_params)
        }
        Expr::UnOp { operand, .. } => expr_has_str_eq(operand, str_params),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_str_eq(cond, str_params)
                || expr_has_str_eq(then_expr, str_params)
                || expr_has_str_eq(else_expr, str_params)
        }
        Expr::Call { args, .. } => args.iter().any(|a| expr_has_str_eq(a, str_params)),
        Expr::Index { collection, index } => {
            expr_has_str_eq(collection, str_params) || expr_has_str_eq(index, str_params)
        }
        Expr::Len(c) => expr_has_str_eq(c, str_params),
        Expr::Ord { value } | Expr::Chr { value } => expr_has_str_eq(value, str_params),
        Expr::StrCharAt { string, index } => {
            expr_has_str_eq(string, str_params) || expr_has_str_eq(index, str_params)
        }
        _ => false,
    }
}

/// `true` if `e` is a string-valued expression: a `LitStr` / `Concat` / `Chr`
/// / bare `StrCharAt` (structural), or a str-param `Ident` (in `str_params`).
fn expr_is_str_valued(e: &Expr, str_params: &[&str]) -> bool {
    match e {
        Expr::LitStr(_) | Expr::Concat { .. } | Expr::Chr { .. } | Expr::StrCharAt { .. } => true,
        Expr::Ident(name) => str_params.contains(&name.as_str()),
        _ => false,
    }
}

/// `true` if `block` contains a string-materialising expression anywhere.
fn block_has_heap_op(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_heap_op) || expr_has_heap_op(&block.trailing_return)
}

fn stmt_has_heap_op(s: &Stmt) -> bool {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } => expr_has_heap_op(value),
        Stmt::Return(e) => expr_has_heap_op(e),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_has_heap_op(cond)
                || then_body.iter().any(stmt_has_heap_op)
                || else_body.iter().any(stmt_has_heap_op)
        }
        Stmt::While { cond, body } => expr_has_heap_op(cond) || body.iter().any(stmt_has_heap_op),
        Stmt::IndexAssign { value, .. } => expr_has_heap_op(value),
        Stmt::Break | Stmt::Continue => false,
        _ => false,
    }
}

/// `true` if `e` (or any sub-expression) materialises a new string —
/// `Expr::Concat` (string `+`), `Expr::Chr`, or PMAT-994 a bare
/// `Expr::StrCharAt` (`s[i]` as a 1-char string, which allocates a 1-byte heap
/// string like `chr`). A `StrCharAt` that is the OPERAND of an `Ord`
/// (`ord(s[i])`) does NOT materialise — it loads a byte — so the `Ord` arm
/// does not recurse into its direct `StrCharAt`.
fn expr_has_heap_op(e: &Expr) -> bool {
    match e {
        // PMAT-996: a `StructLit` bump-allocates a heap record (like a string
        // materialisation), so it pulls in the allocator + `(memory)`.
        Expr::Concat { .. }
        | Expr::Chr { .. }
        | Expr::StrCharAt { .. }
        | Expr::StructLit { .. } => true,
        Expr::FieldAccess { obj, .. } => expr_has_heap_op(obj),
        Expr::BinOp { lhs, rhs, .. } | Expr::FloatBinOp { lhs, rhs, .. } => {
            expr_has_heap_op(lhs) || expr_has_heap_op(rhs)
        }
        Expr::UnOp { operand, .. } => expr_has_heap_op(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => expr_has_heap_op(cond) || expr_has_heap_op(then_expr) || expr_has_heap_op(else_expr),
        Expr::Call { args, .. } => args.iter().any(expr_has_heap_op),
        Expr::Index { collection, index } => {
            expr_has_heap_op(collection) || expr_has_heap_op(index)
        }
        Expr::Len(c) => expr_has_heap_op(c),
        // `ord(s[i])` loads a byte (no allocation). Only recurse into the
        // operand if it is NOT a direct `StrCharAt` — a `StrCharAt` operand of
        // `Ord` is consumed in `emit_ord` without materialising a string.
        Expr::Ord { value } => match value.as_ref() {
            Expr::StrCharAt { string, index } => {
                expr_has_heap_op(string) || expr_has_heap_op(index)
            }
            other => expr_has_heap_op(other),
        },
        _ => false,
    }
}

/// WAT scalar value type — the lowered shape of a supported meta-HIR
/// [`Type`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatTy {
    I64,
    I32,
    F64,
    F32,
}

impl WatTy {
    fn keyword(self) -> &'static str {
        match self {
            WatTy::I64 => "i64",
            WatTy::I32 => "i32",
            WatTy::F64 => "f64",
            WatTy::F32 => "f32",
        }
    }

    /// Width in bytes of this scalar in WASM linear memory — the stride
    /// used to compute `base + index*size` for a list-element load
    /// (PMAT-966).
    fn byte_size(self) -> i32 {
        match self {
            WatTy::I64 | WatTy::F64 => 8,
            WatTy::I32 | WatTy::F32 => 4,
        }
    }

    /// The natural-width `*.load` opcode for this scalar — used to read a
    /// list element out of linear memory (PMAT-966).
    fn load_instr(self) -> &'static str {
        match self {
            WatTy::I64 => "i64.load",
            WatTy::I32 => "i32.load",
            WatTy::F64 => "f64.load",
            WatTy::F32 => "f32.load",
        }
    }

    /// The natural-width `*.store` opcode for this scalar — the mirror of
    /// [`WatTy::load_instr`], used to WRITE a list element into linear
    /// memory for `xs[i] = v` (PMAT-978). A store consumes `(address,
    /// value)` from the stack and leaves nothing.
    fn store_instr(self) -> &'static str {
        match self {
            WatTy::I64 => "i64.store",
            WatTy::I32 => "i32.store",
            WatTy::F64 => "f64.store",
            WatTy::F32 => "f32.store",
        }
    }
}

/// Map a meta-HIR [`Type`] to its WAT value type, refusing everything
/// outside the scalar subset.
fn map_type(ty: &Type) -> Result<WatTy, BackendError> {
    match ty {
        // 64-bit signed integer (and the C 64-bit-ABI sibling).
        Type::I64 | Type::CLong => Ok(WatTy::I64),
        // Bool has no WASM type — represented as an i32 holding 0/1
        // (the canonical WASM boolean encoding).
        Type::Bool => Ok(WatTy::I32),
        // 32-bit unsigned C integer — rides an i32 (modular C semantics).
        Type::CUInt => Ok(WatTy::I32),
        Type::F64 => Ok(WatTy::F64),
        Type::F32 => Ok(WatTy::F32),
        other => Err(unsupported(&format!(
            "type {other:?} (the WASM emit subset is i64/i32/f64/f32/bool only — \
             str/list/dict/set/struct/tuple/bigint/pointer are refused)"
        ))),
    }
}

/// Map a `list[T]` parameter to the WAT element type its elements load as
/// (PMAT-966). The list itself rides an `i32` base-pointer; the element
/// type must be a supported scalar that has a natural `*.load` —
/// `i64`/`f64`/`f32`. A `list[bool]` is refused (no WASM bool load width
/// is honest), as are nested lists, `list[str]`, etc.
fn map_list_elem_type(inner: &Type) -> Result<WatTy, BackendError> {
    match inner {
        Type::I64 | Type::CLong => Ok(WatTy::I64),
        Type::F64 => Ok(WatTy::F64),
        Type::F32 => Ok(WatTy::F32),
        other => Err(unsupported(&format!(
            "list element type {other:?} — the WASM list subset supports \
             list[int]/list[float] only (i64/f64/f32 elements with a natural \
             *.load); list[bool], list[str], and nested lists are refused"
        ))),
    }
}

/// Map a **parameter** type to its WAT value type. Identical to
/// [`map_type`] for scalars, but additionally accepts a `list[scalar]`
/// (PMAT-966) and a `str` (PMAT-986): both ride an `i32` base-pointer into
/// linear memory, so the param's WAT type is `i32`. For a list the element
/// type is validated here (the caller separately records it for `Index`
/// lowering); a list of a non-scalar element is refused by
/// [`map_list_elem_type`]. A `str` param needs no element validation — it is
/// a length-prefixed UTF-8 byte region (PMAT-986) accessed per-byte
/// (`ord(s[i])` → `i32.load8_u`, `len(s)` → header read).
fn param_wat_type(ty: &Type) -> Result<WatTy, BackendError> {
    if let Type::List(inner) = ty {
        // Validate the element type now (honest early refusal); the list
        // itself is an i32 base-pointer.
        map_list_elem_type(inner)?;
        return Ok(WatTy::I32);
    }
    if matches!(ty, Type::Str) {
        // PMAT-986: a `str` param is an i32 base-pointer to a length-prefixed
        // UTF-8 byte region in linear memory (i32 byte count @ base+0, bytes
        // @ base+8). Same ABI shape as a list param.
        return Ok(WatTy::I32);
    }
    if matches!(ty, Type::Struct(_)) {
        // PMAT-996: a struct param is an i32 base-pointer to a heap record
        // (fields at fixed 8-byte-slot offsets); `p.field` loads from it.
        return Ok(WatTy::I32);
    }
    map_type(ty)
}

fn unsupported(what: &str) -> BackendError {
    BackendError::Lower(format!(
        "xpile-wasm-codegen: unsupported construct — {what}"
    ))
}

/// Per-function lowering scope: the WAT value type of every in-scope
/// local (params + `let` bindings), recorded in declaration order so the
/// emitter can pick `i64.add` vs `f64.add` and emit the right
/// `local`/`local.get`/`local.set`.
struct Scope<'a> {
    /// `(name, watty)` for every local, in stable order. A `list[...]`
    /// param appears here as an `i32` (its base-pointer); its element type
    /// is recorded separately in [`Scope::list_elem`].
    locals: Vec<(String, WatTy)>,
    /// PMAT-966: for each local that is a `list[scalar]` base-pointer, the
    /// WAT type its elements load as (`i64`/`f64`/`f32`). `Index` over such
    /// a local emits `base + i*size` + that element's `*.load`.
    list_elem: Vec<(String, WatTy)>,
    /// PMAT-986: names of params that are `str` base-pointers into linear
    /// memory (i32 byte count @ base+0, UTF-8 bytes @ base+8). `len(s)` reads
    /// the header; `ord(s[i])` does a bounds-checked `i32.load8_u` of byte
    /// `i`. Only str PARAMS land here; PMAT-994 adds str LITERALS (resolved
    /// via [`Scope::literals`]) — there are still no str LOCALS in the subset.
    str_params: Vec<String>,
    /// PMAT-994 (slice 3a): the module's static string-literal layout. A
    /// `LitStr` lowers to a constant `i32.const <base>` resolved here. Shared
    /// across every function in the module (the `(data …)` region is global).
    literals: &'a StrLiterals,
    /// PMAT-995 (slice 3b): `(name, key_kind)` for every LET-bound `dict`/`set`
    /// local. The local itself is an `i32` base-pointer (in [`Scope::locals`]);
    /// this records its key kind so `d[k]` / `k in d` / `d[k]=v` / `s.add(e)`
    /// pick the right `$__wasm_dict_*_<k>` helper and key encoding.
    heap_maps: Vec<(String, KeyKind)>,
    /// PMAT-996 (slice 4): the module's struct layout registry (name → fields),
    /// shared across every function (struct definitions are module-global).
    structs: &'a StructRegistry,
    /// PMAT-996 (slice 4): `(name, struct_name)` for every struct-typed LOCAL
    /// or PARAM. The name itself is an `i32` base-pointer (in [`Scope::locals`]);
    /// this records which struct's layout drives its `obj.field` reads.
    struct_locals: Vec<(String, String)>,
    /// The function's return WAT type (drives `return` checking).
    ret: WatTy,
    /// Whether the return type is the unit/void shape (no value).
    ret_is_unit: bool,
}

impl Scope<'_> {
    /// PMAT-995: the [`KeyKind`] if `name` is a dict/set base-pointer local, else
    /// `None`. Drives dict/set op lowering (`DictGet`/`DictContains`/`DictSet`/
    /// `SetContains`/`SetAdd`/`len`).
    fn heap_map_kind(&self, name: &str) -> Option<KeyKind> {
        self.heap_maps
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, k)| *k)
    }

    /// PMAT-996: the struct type name if `name` is a struct local/param
    /// base-pointer, else `None`. Drives `obj.field` lowering.
    fn struct_of(&self, name: &str) -> Option<String> {
        self.struct_locals
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s.clone())
    }

    fn ty_of(&self, name: &str) -> Result<WatTy, BackendError> {
        self.locals
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, t)| *t)
            .ok_or_else(|| unsupported(&format!("reference to unbound name `{name}`")))
    }

    /// The element WAT type if `name` is a `list[scalar]` base-pointer,
    /// else `None`. Drives [`Expr::Index`] load-shape selection.
    fn list_elem_of(&self, name: &str) -> Option<WatTy> {
        self.list_elem
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, t)| *t)
    }

    /// PMAT-986: `true` if `name` is a `str` parameter base-pointer (a
    /// length-prefixed UTF-8 byte region in linear memory). Drives
    /// `len(s)` and `ord(s[i])` lowering.
    fn is_str_param(&self, name: &str) -> bool {
        self.str_params.iter().any(|n| n == name)
    }

    /// PMAT-994: the static base address of the string literal `content`, or
    /// `None` if it was not laid out (should not happen — every literal in the
    /// module body is collected by [`collect_str_literals`]).
    fn literal_addr(&self, content: &str) -> Option<i32> {
        self.literals.addr_of(content)
    }

    /// Declare a new local; idempotent on an existing name (re-`Let` of a
    /// name reuses the slot — WASM locals are function-scoped).
    fn declare(&mut self, name: &str, ty: WatTy) {
        if !self.locals.iter().any(|(n, _)| n == name) {
            self.locals.push((name.to_string(), ty));
        }
    }
}

/// Emit one `(func …)` for `f`.
fn emit_function(
    f: &Function,
    literals: &StrLiterals,
    structs: &StructRegistry,
) -> Result<String, BackendError> {
    // PMAT-993 (slice 2): a `str` RETURN now lowers to an `i32` result — the
    // base-pointer of the newly-materialised length-prefixed string in linear
    // memory (the bump heap this slice ships). The trailing return must be a
    // string-VALUED expression (a `Concat`/`Chr`/str-param ident), validated
    // by `emit_str_expr`. Slice 1 refused this; slice 2's allocator unblocks
    // it. A str-returning function is the headline deliverable of this slice.
    let ret_is_str = matches!(f.return_type, Type::Str);
    let ret_is_unit = matches!(f.return_type, Type::Unit);
    let ret = if ret_is_unit {
        // A void function: no result type. Use i32 as a placeholder that
        // is never read (ret_is_unit gates emission of any result).
        WatTy::I32
    } else if ret_is_str {
        // A str result rides an i32 base-pointer (the heap-allocated string).
        WatTy::I32
    } else {
        map_type(&f.return_type)?
    };

    let mut scope = Scope {
        locals: Vec::new(),
        list_elem: Vec::new(),
        str_params: Vec::new(),
        heap_maps: Vec::new(),
        structs,
        struct_locals: Vec::new(),
        literals,
        ret,
        ret_is_unit,
    };
    // Params are locals 0..n. A `list[scalar]` param (PMAT-966) rides an
    // `i32` base-pointer into linear memory; its element type is recorded
    // so `Index` knows the load shape. A `str` param (PMAT-986) likewise
    // rides an `i32` base-pointer (length-prefixed UTF-8 byte region) and is
    // recorded so `len(s)` / `ord(s[i])` lower correctly. Every other param
    // maps to its scalar WAT type.
    for Param { name, ty, .. } in &f.params {
        let wt = param_wat_type(ty)?;
        scope.declare(name, wt);
        if let Type::List(inner) = ty {
            let elem = map_list_elem_type(inner)?;
            scope.list_elem.push((name.clone(), elem));
        }
        if matches!(ty, Type::Str) {
            scope.str_params.push(name.clone());
        }
        // PMAT-996: a struct PARAM rides an `i32` base-pointer into linear
        // memory (like a list/str param); record its struct type so `p.field`
        // reads resolve the field offset from the registry.
        if let Type::Struct(sname) = ty {
            scope.struct_locals.push((name.clone(), sname.clone()));
        }
    }

    // Pre-declare every `let`-bound local by walking the body, so the
    // `(local …)` declarations precede the body in WAT (WASM requires all
    // locals declared up front, after the params).
    collect_let_locals(&f.body, &mut scope)?;

    // Emit body into a buffer first (it also validates types).
    let mut body = String::new();
    for stmt in &f.body.stmts {
        emit_stmt(stmt, &mut scope, &mut body, 2)?;
    }
    // Trailing return expression: emit and (if non-unit) it becomes the
    // function result.
    if ret_is_unit {
        // A unit trailing return (`Expr::Unit`) yields nothing; any other
        // trailing expr in a void fn is a value we drop.
        if !matches!(f.body.trailing_return, Expr::Unit) {
            emit_expr(&f.body.trailing_return, &scope, &mut body, 2)?;
            indent(&mut body, 2);
            body.push_str("drop\n");
        }
    } else if ret_is_str {
        // PMAT-993: a str return — the trailing expr must be string-VALUED
        // (a heap pointer); emit it via the dedicated string lowering.
        emit_str_expr(&f.body.trailing_return, &scope, &mut body, 2)?;
    } else {
        emit_expr(&f.body.trailing_return, &scope, &mut body, 2)?;
    }

    // Now assemble the (func) header with signature + local decls.
    let mut out = String::new();
    write!(out, "  (func ${} ", f.name).expect("write");
    for Param { name, ty, .. } in &f.params {
        let wt = param_wat_type(ty)?;
        write!(out, "(param ${} {}) ", name, wt.keyword()).expect("write");
    }
    if !ret_is_unit {
        write!(out, "(result {}) ", ret.keyword()).expect("write");
    }
    writeln!(out, ";; xpile-contract: {CONTRACT_ID}").expect("write");

    // Local declarations (skip params, which are the first locals).
    let n_params = f.params.len();
    for (name, wt) in scope.locals.iter().skip(n_params) {
        writeln!(out, "    (local ${} {})", name, wt.keyword()).expect("write");
    }
    // PMAT-968: declare the bounds-check scratch `i64` local iff a
    // bounds-checked `Index` actually used it (the body references
    // `$__wasm_idx`). Detected from the emitted body so it is declared
    // exactly when needed — no spurious local for index-free functions.
    if body.contains(&format!("${IDX_SCRATCH}")) {
        writeln!(out, "    (local ${IDX_SCRATCH} i64)").expect("write");
    }
    // PMAT-993: declare the string-construction scratch `i32` locals iff a
    // `Concat`/`Chr` actually used them (same body-driven detection as the
    // index scratch). `$__wasm_str_dst` holds the destination base-pointer;
    // `$__wasm_str_la` holds the first operand's byte length (the write
    // offset for the second operand's bytes).
    if body.contains(&format!("${STR_DST_SCRATCH}")) {
        writeln!(out, "    (local ${STR_DST_SCRATCH} i32)").expect("write");
    }
    if body.contains(&format!("${STR_LA_SCRATCH}")) {
        writeln!(out, "    (local ${STR_LA_SCRATCH} i32)").expect("write");
    }
    // PMAT-998: the dedicated concat-destination local (distinct from
    // $__wasm_str_dst so operand evals don't clobber it).
    if body.contains(&format!("${STR_CONCAT_DST}")) {
        writeln!(out, "    (local ${STR_CONCAT_DST} i32)").expect("write");
    }
    // PMAT-1000: the dedicated concat write-offset local (distinct from
    // $__wasm_str_la so an `s[i]` operand's scratch cannot clobber it).
    if body.contains(&format!("${STR_CONCAT_OFF}")) {
        writeln!(out, "    (local ${STR_CONCAT_OFF} i32)").expect("write");
    }
    // PMAT-995: declare the dict-construction scratch `i32` local iff a
    // `DictLit`/`SetLit` actually used it (same body-driven detection).
    if body.contains(&format!("${DICT_DST_SCRATCH}")) {
        writeln!(out, "    (local ${DICT_DST_SCRATCH} i32)").expect("write");
    }
    // PMAT-996: declare the struct-construction scratch `i32` local iff a
    // `StructLit` actually used it (same body-driven detection).
    if body.contains(&format!("${STRUCT_DST_SCRATCH}")) {
        writeln!(out, "    (local ${STRUCT_DST_SCRATCH} i32)").expect("write");
    }

    out.push_str(&body);
    writeln!(out, "  )").expect("write");
    writeln!(out, "  (export \"{}\" (func ${}))", f.name, f.name).expect("write");
    Ok(out)
}

/// Walk the body declaring every `Let` local in `scope` up front.
fn collect_let_locals(block: &Block, scope: &mut Scope) -> Result<(), BackendError> {
    collect_let_locals_stmts(&block.stmts, scope)
}

fn collect_let_locals_stmts(stmts: &[Stmt], scope: &mut Scope) -> Result<(), BackendError> {
    for s in stmts {
        match s {
            // PMAT-995 (slice 3b): a `dict`/`set` LET binds an `i32`
            // base-pointer local AND records its key kind (the value type is
            // validated now too, an honest early refusal). Intercepted before
            // `map_type`, which refuses Dict/Set.
            Stmt::Let {
                name,
                ty: Type::Dict(k, v),
                ..
            } => {
                let kind = dict_key_kind(k)?;
                dict_value_is_supported(v)?;
                scope.declare(name, WatTy::I32);
                scope.heap_maps.push((name.clone(), kind));
            }
            Stmt::Let {
                name,
                ty: Type::Set(e),
                ..
            } => {
                let kind = dict_key_kind(e)?;
                scope.declare(name, WatTy::I32);
                scope.heap_maps.push((name.clone(), kind));
            }
            // PMAT-996 (slice 4): a struct LET binds an `i32` base-pointer local
            // AND records its struct type (so `p.field` resolves the layout).
            // Intercepted before `map_type`, which refuses `Struct`.
            Stmt::Let {
                name,
                ty: Type::Struct(sname),
                ..
            } => {
                scope.declare(name, WatTy::I32);
                scope.struct_locals.push((name.clone(), sname.clone()));
            }
            Stmt::Let { name, ty, .. } => {
                let wt = map_type(ty)?;
                scope.declare(name, wt);
            }
            Stmt::While { body, .. } => collect_let_locals_stmts(body, scope)?,
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_let_locals_stmts(then_body, scope)?;
                collect_let_locals_stmts(else_body, scope)?;
            }
            // PMAT-978: `xs[i] = v` writes an EXISTING list element — it
            // introduces no new `let` local. (The `$__wasm_idx` scratch it
            // uses is declared from the emitted body, like the read path.)
            // PMAT-995: `d[k] = v` (DictSet) and `s.add(e)` (SetAdd) mutate an
            // existing dict/set in place — no new local (the dict/set base
            // local was declared by its `Let`).
            Stmt::Assign { .. }
            | Stmt::IndexAssign { .. }
            | Stmt::DictSet { .. }
            | Stmt::SetAdd { .. }
            | Stmt::Return(_)
            | Stmt::Break
            | Stmt::Continue => {}
            other => {
                return Err(unsupported(&format!(
                    "statement {} (outside the WASM scalar/control subset)",
                    stmt_kind(other)
                )));
            }
        }
    }
    Ok(())
}

fn indent(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push_str("  ");
    }
}

/// Short kind label for an unsupported statement, for the refusal message.
fn stmt_kind(s: &Stmt) -> &'static str {
    match s {
        Stmt::Return(_) => "Return",
        Stmt::If { .. } => "If",
        Stmt::While { .. } => "While",
        Stmt::Let { .. } => "Let",
        Stmt::Assign { .. } => "Assign",
        Stmt::Break => "Break",
        Stmt::Continue => "Continue",
        Stmt::Print { .. } => "Print",
        Stmt::IndexAssign { .. } => "IndexAssign",
        _ => "<container/aggregate statement>",
    }
}

/// Emit a statement at `depth` indentation. `$brk`/`$cont` labels for the
/// nearest loop are implicit in WAT block/loop nesting (br to the named
/// label).
fn emit_stmt(
    s: &Stmt,
    scope: &mut Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    match s {
        Stmt::Let { name, value, .. } => {
            // PMAT-995: a dict/set LET (its local recorded in `scope.heap_maps`)
            // materialises its `DictLit`/`SetLit` on the bump heap and stashes
            // the base-pointer; routed away from the scalar `emit_expr_typed`
            // path (which has no K/V context).
            if let Some(kind) = scope.heap_map_kind(name) {
                emit_heap_map_bind(value, kind, scope, out, depth)?;
                indent(out, depth);
                writeln!(out, "local.set ${name}").expect("write");
                return Ok(());
            }
            let wt = scope.ty_of(name)?;
            emit_expr_typed(value, scope, out, depth, wt)?;
            indent(out, depth);
            writeln!(out, "local.set ${name}").expect("write");
            Ok(())
        }
        Stmt::Assign { name, value } => {
            if let Some(kind) = scope.heap_map_kind(name) {
                emit_heap_map_bind(value, kind, scope, out, depth)?;
                indent(out, depth);
                writeln!(out, "local.set ${name}").expect("write");
                return Ok(());
            }
            let wt = scope.ty_of(name)?;
            emit_expr_typed(value, scope, out, depth, wt)?;
            indent(out, depth);
            writeln!(out, "local.set ${name}").expect("write");
            Ok(())
        }
        // PMAT-995 (slice 3b): `d[k] = v` — update-or-insert over a dict local.
        Stmt::DictSet {
            dict_name,
            key,
            value,
        } => emit_dict_set(dict_name, key, value, scope, out, depth),
        // PMAT-995 (slice 3b): `s.add(e)` — insert into a set local (a keys-only
        // dict; the `set` helper is shared, with a 0 sentinel value).
        Stmt::SetAdd { set_name, elem } => emit_set_add(set_name, elem, scope, out, depth),
        Stmt::Return(e) => {
            if scope.ret_is_unit {
                if !matches!(e, Expr::Unit) {
                    return Err(unsupported(
                        "early `return <value>` from a unit/void function",
                    ));
                }
            } else {
                emit_expr_typed(e, scope, out, depth, scope.ret)?;
            }
            indent(out, depth);
            writeln!(out, "return").expect("write");
            Ok(())
        }
        Stmt::Break => {
            // Break exits the nearest loop's surrounding `$brk` block.
            indent(out, depth);
            writeln!(out, "br $brk").expect("write");
            Ok(())
        }
        Stmt::Continue => {
            // Continue re-enters the nearest `$cont` loop.
            indent(out, depth);
            writeln!(out, "br $cont").expect("write");
            Ok(())
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            // cond is a Bool → i32; `if` consumes an i32 condition.
            emit_expr_typed(cond, scope, out, depth, WatTy::I32)?;
            indent(out, depth);
            writeln!(out, "if").expect("write");
            for st in then_body {
                emit_stmt(st, scope, out, depth + 1)?;
            }
            if !else_body.is_empty() {
                indent(out, depth);
                writeln!(out, "else").expect("write");
                for st in else_body {
                    emit_stmt(st, scope, out, depth + 1)?;
                }
            }
            indent(out, depth);
            writeln!(out, "end").expect("write");
            Ok(())
        }
        Stmt::While { cond, body } => {
            // While →
            //   (block $brk (loop $cont <cond> i32.eqz br_if $brk <body> br $cont))
            indent(out, depth);
            writeln!(out, "(block $brk").expect("write");
            indent(out, depth + 1);
            writeln!(out, "(loop $cont").expect("write");
            emit_expr_typed(cond, scope, out, depth + 2, WatTy::I32)?;
            indent(out, depth + 2);
            writeln!(out, "i32.eqz").expect("write");
            indent(out, depth + 2);
            writeln!(out, "br_if $brk").expect("write");
            for st in body {
                emit_stmt(st, scope, out, depth + 2)?;
            }
            indent(out, depth + 2);
            writeln!(out, "br $cont").expect("write");
            indent(out, depth + 1);
            writeln!(out, ")").expect("write");
            indent(out, depth);
            writeln!(out, ")").expect("write");
            Ok(())
        }
        // PMAT-978: `xs[i] = v` — in-place list-element write over a
        // `list[scalar]` parameter, via the shared bounds-checked
        // linear-memory address + a natural-width `*.store`.
        Stmt::IndexAssign {
            list_name,
            indices,
            value,
        } => emit_index_assign(list_name, indices, value, scope, out, depth),
        other => Err(unsupported(&format!(
            "statement {} (outside the WASM scalar/control subset)",
            stmt_kind(other)
        ))),
    }
}

/// Emit an expression, asserting its result lowers to WAT type `expect`.
/// (A light static check — the meta-HIR doesn't carry per-expr types, so
/// the emitter infers the WAT type from operands and validates against
/// the binding/return site.)
fn emit_expr_typed(
    e: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
    expect: WatTy,
) -> Result<(), BackendError> {
    let got = emit_expr(e, scope, out, depth)?;
    if got != expect {
        return Err(unsupported(&format!(
            "type mismatch — expected WASM {} but expression lowered to {}",
            expect.keyword(),
            got.keyword()
        )));
    }
    Ok(())
}

/// Emit an expression, returning the WAT type it leaves on the stack.
fn emit_expr(
    e: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    match e {
        Expr::Ident(name) => {
            let wt = scope.ty_of(name)?;
            indent(out, depth);
            writeln!(out, "local.get ${name}").expect("write");
            Ok(wt)
        }
        Expr::LitInt(v) => {
            indent(out, depth);
            writeln!(out, "i64.const {v}").expect("write");
            Ok(WatTy::I64)
        }
        Expr::LitBool(b) => {
            indent(out, depth);
            writeln!(out, "i32.const {}", if *b { 1 } else { 0 }).expect("write");
            Ok(WatTy::I32)
        }
        Expr::LitFloat(v) => {
            indent(out, depth);
            // WAT float literals accept the standard decimal form; ensure
            // a decimal point so it parses as a float, and handle the
            // non-finite cases explicitly.
            writeln!(out, "f64.const {}", wat_float_literal(*v)).expect("write");
            Ok(WatTy::F64)
        }
        Expr::UnOp { op, operand } => emit_unop(*op, operand, scope, out, depth),
        Expr::BinOp { op, lhs, rhs } => emit_binop(*op, lhs, rhs, scope, out, depth),
        Expr::FloatBinOp { op, lhs, rhs } => emit_float_binop(*op, lhs, rhs, scope, out, depth),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            // Both arms must lower to the same WAT type — that becomes the
            // (if (result T) …) type.
            // Emit cond first (i32), then a typed if/else producing a value.
            emit_expr_typed(cond, scope, out, depth, WatTy::I32)?;
            // We need the arm type; emit the then-arm into a temp to learn
            // its type, then re-emit both inline. Simpler: emit then-arm,
            // capture its type, require else to match.
            let mut then_buf = String::new();
            let then_ty = emit_expr(then_expr, scope, &mut then_buf, depth + 1)?;
            let mut else_buf = String::new();
            let else_ty = emit_expr(else_expr, scope, &mut else_buf, depth + 1)?;
            if then_ty != else_ty {
                return Err(unsupported(&format!(
                    "if-expression arms lower to different WASM types ({} vs {})",
                    then_ty.keyword(),
                    else_ty.keyword()
                )));
            }
            indent(out, depth);
            writeln!(out, "if (result {})", then_ty.keyword()).expect("write");
            out.push_str(&then_buf);
            indent(out, depth);
            writeln!(out, "else").expect("write");
            out.push_str(&else_buf);
            indent(out, depth);
            writeln!(out, "end").expect("write");
            Ok(then_ty)
        }
        Expr::Call { callee, args } => {
            // Direct intra-module call — args left-to-right, then `call $f`.
            // The result type is not carried in the meta-HIR Call node, so
            // we cannot statically know it; default to the function's
            // declared shape is unavailable here. We refuse a call whose
            // result type we can't determine UNLESS it is used in a typed
            // position (emit_expr_typed validates). Push args then call;
            // report I64 as the conservative default and let the typed
            // check catch a mismatch.
            for a in args {
                emit_expr(a, scope, out, depth)?;
            }
            indent(out, depth);
            writeln!(out, "call ${callee}").expect("write");
            // The meta-HIR Call carries no result type; intra-module calls
            // in the scalar subset return i64 (the dominant scalar). A
            // float/bool-returning call is validated at the typed use site;
            // if that fails the user gets the honest type-mismatch refusal.
            Ok(WatTy::I64)
        }
        Expr::Index { collection, index } => emit_index(collection, index, scope, out, depth),
        Expr::Len(collection) => emit_len(collection, scope, out, depth),
        // PMAT-986: `ord(s[i])` over a `str` param — the ONE string op that
        // returns an int (a code point), so it needs no result string. Any
        // other `ord` operand (e.g. `ord(chr(n))`) is refused.
        Expr::Ord { value } => emit_ord(value, scope, out, depth),
        // PMAT-994 (slice 3a): `s[i]` used AS a 1-char string (a `StrCharAt`
        // NOT wrapped in `ord`) now materialises a NEW 1-char heap string (the
        // `chr` mirror, copying byte `i` of the string-valued base). Its result
        // is an i32 (the str pointer). Lowering `ord(s[i])` consumes the inner
        // `StrCharAt` directly in `emit_ord`, so a `StrCharAt` reaching here is
        // a string-VALUED use.
        Expr::StrCharAt { string, index } => {
            emit_str_char_at(string, index, scope, out, depth)?;
            Ok(WatTy::I32)
        }
        // PMAT-994: a string LITERAL is a constant i32 pointer to its static
        // `(data …)` segment. A use in a non-string position is an honest type
        // mismatch at the typed site (an i32 string pointer is not an arithmetic
        // value).
        Expr::LitStr(s) => {
            let addr = scope.literal_addr(s).ok_or_else(|| {
                unsupported(&format!(
                    "string literal {s:?} was not laid out into a static (data) \
                     segment — internal layout error"
                ))
            })?;
            indent(out, depth);
            writeln!(out, "i32.const {addr}").expect("write");
            Ok(WatTy::I32)
        }
        // PMAT-993: `chr(n)` and string concat `a + b` MATERIALISE a new
        // length-prefixed string in the bump heap and leave its i32
        // base-pointer on the stack — the slice-2 string-RETURNING ops. The
        // result is an i32 (the str pointer); a use of it in a non-string
        // (scalar-arithmetic) position is an honest type mismatch at the typed
        // site.
        Expr::Chr { value } => {
            emit_chr(value, scope, out, depth)?;
            Ok(WatTy::I32)
        }
        Expr::Concat { lhs, rhs } => {
            emit_concat(lhs, rhs, scope, out, depth)?;
            Ok(WatTy::I32)
        }
        // PMAT-995 (slice 3b): `d[k]` — keyed dict read; returns the i64 value
        // or TRAPS on an absent key (the Python KeyError analogue).
        Expr::DictGet { dict, key } => emit_dict_get(dict, key, scope, out, depth),
        // PMAT-995 (slice 3b): `k in d` / `x in s` — i32 bool membership.
        Expr::DictContains { dict, key } => emit_dict_contains(dict, key, scope, out, depth),
        Expr::SetContains { set, elem } => emit_dict_contains(set, elem, scope, out, depth),
        // PMAT-996 (slice 4): `Name(f=v, …)` — allocate + populate a plain-data
        // struct on the bump heap; leaves the instance's i32 base-pointer.
        Expr::StructLit { name, fields } => emit_struct_lit(name, fields, scope, out, depth),
        // PMAT-996 (slice 4): `obj.field` — load a field from a struct local/param.
        Expr::FieldAccess { obj, field } => emit_field_access(obj, field, scope, out, depth),
        // PMAT-995: a `DictLit`/`SetLit` needs its K/V from the binding's
        // declared type; it is materialised by the dict/set `Let` path
        // (`emit_heap_map_bind`), never standalone. Reaching here means a
        // dict/set literal in a non-binding value position — refused honestly
        // (the binding's K/V context is unavailable here).
        Expr::DictLit(_) | Expr::SetLit(_) => Err(unsupported(
            "a dict/set literal outside a `dict`/`set`-typed `let` binding — the \
             WASM subset materialises a dict/set only at its annotated binding \
             site (it needs the key/value types); a bare literal value is refused",
        )),
        Expr::Unit => Err(unsupported(
            "unit value `()` in a value position (WASM has no unit operand)",
        )),
        other => Err(unsupported(&format!(
            "expression {} (outside the WASM scalar/control subset — \
             str/list/dict/set/struct/tuple/closure/print are refused)",
            expr_kind(other)
        ))),
    }
}

/// Emit a **bounds-checked** read-only `xs[i]` over a `list[scalar]`
/// parameter (PMAT-966 layout, PMAT-968 bounds-check + offset).
///
/// `collection` must be an [`Expr::Ident`] naming a list-param base-pointer
/// (the only list shape in the WASM subset — there are no list literals,
/// list-typed locals, or list returns). The index is a non-negative `i64`
/// (the meta-HIR `Index.index` posture).
///
/// As of PMAT-968 the pointed-at region is length-prefixed: an `i32`
/// element count at `base+0`, then the packed elements from
/// `base + LIST_ELEMS_OFFSET`. The lowering first evaluates the index into
/// a scratch `i64` local, then emits a bounds guard —
/// `i < 0 || i >= len → unreachable` — which traps the way Python raises
/// `IndexError` (and the Rust `vec[i]` lane panics). PMAT-966 deliberately
/// refused this guard, letting an out-of-range linear address silently
/// mis-read (or only trap on an unmapped page); PMAT-968 makes the
/// out-of-bounds read a deterministic trap. After the guard the element
/// address is `base + LIST_ELEMS_OFFSET + (index as i32) * elem_size`, read
/// with the element's natural `*.load`. The result type is the list's
/// element WAT type.
fn emit_index(
    collection: &Expr,
    index: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let Expr::Ident(name) = collection else {
        return Err(unsupported(
            "indexing a non-name collection — the WASM list subset only \
             indexes a `list[scalar]` PARAMETER (an i32 base-pointer); \
             list literals / temporaries / nested indexing are refused",
        ));
    };
    let Some(elem) = scope.list_elem_of(name) else {
        return Err(unsupported(&format!(
            "index over `{name}` which is not a `list[scalar]` parameter — \
             only a list param (i32 base-pointer into linear memory) can be \
             indexed in the WASM subset (no str/dict/tuple indexing)"
        )));
    };
    // Emit the bounds-checked element address onto the stack, then read the
    // element at it with the element's natural `*.load`.
    emit_list_elem_addr(name, elem, index, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "{}", elem.load_instr()).expect("write");
    Ok(elem)
}

/// Emit the bounds-checked linear-memory ADDRESS of `name[index]` onto the
/// WASM stack, for a `list[scalar]` parameter `name` whose elements load as
/// `elem`. Shared by the READ path ([`emit_index`] — append a `*.load`) and
/// the WRITE path ([`emit_index_assign`] — push the value, append a
/// `*.store`), so the PMAT-968 bounds guard lives in exactly ONE place and
/// can never drift between read and write.
///
/// Sequence (PMAT-968 + PMAT-978):
///   1. evaluate `index` once into the per-function scratch `i64`
///      `$__wasm_idx` (so a possibly-effectful index is not re-run);
///   2. bounds guard — `if (i < 0) | (i >= len) { unreachable }` — the
///      Python `IndexError` / Rust `vec[i]`-panic analogue (`len` is the
///      i32 header at `base+0`, zero-extended to i64);
///   3. leave `addr = base + LIST_ELEMS_OFFSET + (i as i32) * elem_size`
///      on the stack.
fn emit_list_elem_addr(
    name: &str,
    elem: WatTy,
    index: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // Evaluate the index expression once into the per-function scratch i64
    // local `$__wasm_idx` so it can be reused by both the bounds guard and
    // the address computation without re-evaluating a (possibly effectful)
    // call.
    emit_expr_typed(index, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "local.set ${IDX_SCRATCH}").expect("write");

    // PMAT-1001: Python negative-index normalization — `if i < 0 { i += len }`.
    // A RUNTIME-negative index (`xs[len(xs)-5]`), or a store-side negative
    // literal (`xs[-1] = v`, which the frontend does NOT fold the way it folds a
    // read-side `xs[-1]` to `xs[len-1]`), wraps to the tail — matching CPython —
    // instead of fail-loud trapping. A still-negative result (`i < -len`) is
    // caught by the bounds guard below (Python raises IndexError there too).
    // Harmless for an already-non-negative index (the `if` body is skipped).
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i64.const 0").expect("write");
    indent(out, depth);
    writeln!(out, "i64.lt_s").expect("write");
    indent(out, depth);
    writeln!(out, "if").expect("write");
    indent(out, depth + 1);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth + 1);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth + 1);
    writeln!(out, "i32.load").expect("write"); // element count (header @ base+0)
    indent(out, depth + 1);
    writeln!(out, "i64.extend_i32_u").expect("write");
    indent(out, depth + 1);
    writeln!(out, "i64.add").expect("write"); // i + len
    indent(out, depth + 1);
    writeln!(out, "local.set ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "end").expect("write");

    // Bounds guard (PMAT-968 — the Python IndexError analogue):
    //   if (i < 0) | (i >= len) { unreachable }
    // `len` is the i32 header at base+0, zero-extended to i64 for the
    // signed compare against the i64 index.
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i64.const 0").expect("write");
    indent(out, depth);
    writeln!(out, "i64.lt_s").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load").expect("write"); // header element count
    indent(out, depth);
    writeln!(out, "i64.extend_i32_u").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i64.le_s").expect("write"); // len <= i  ⇔  i >= len
    indent(out, depth);
    writeln!(out, "i32.or").expect("write");
    indent(out, depth);
    writeln!(out, "if").expect("write");
    indent(out, depth + 1);
    writeln!(out, "unreachable").expect("write");
    indent(out, depth);
    writeln!(out, "end").expect("write");

    // addr = base + LIST_ELEMS_OFFSET + (index as i32) * elem_size
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {LIST_ELEMS_OFFSET}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.wrap_i64").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {}", elem.byte_size()).expect("write");
    indent(out, depth);
    writeln!(out, "i32.mul").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");
    Ok(())
}

/// Emit `xs[i] = v` (`Stmt::IndexAssign`) over a `list[scalar]` parameter
/// (PMAT-978) — the in-place-mutation companion of [`emit_index`].
///
/// Reuses the entire PMAT-968 length-prefixed linear-memory ABI: the SAME
/// bounds-checked address computation ([`emit_list_elem_addr`]) as the read
/// path, but terminates in the element's natural `*.store` instead of a
/// `*.load`. A WASM store consumes `(address, value)` from the stack, so the
/// element address is emitted first, then the value (typed to the element
/// WAT type), then `*.store`.
///
/// ONLY a single-index `xs[i] = v` over a `list[scalar]` PARAMETER is
/// supported. Honestly refused (a hard `BackendError`, never a silent
/// miscompile): a multi-index / nested write (`xs[i][j] = v`, `indices.len()
/// != 1`), an index-assign whose `list_name` is not a bound `list[scalar]`
/// parameter base-pointer, a `list[bool]` element (no honest store width —
/// already excluded by [`Scope::list_elem_of`]), and a value whose lowered
/// WAT type is not the element type (caught by [`emit_expr_typed`]).
fn emit_index_assign(
    list_name: &str,
    indices: &[Expr],
    value: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let [index] = indices else {
        return Err(unsupported(&format!(
            "multi-index assignment `{list_name}[…][…] = v` ({} indices) — the \
             WASM list subset writes a single-index `list[scalar]` element \
             only; nested-list index-assignment is refused",
            indices.len()
        )));
    };
    let Some(elem) = scope.list_elem_of(list_name) else {
        return Err(unsupported(&format!(
            "index-assignment `{list_name}[i] = v` over `{list_name}` which is \
             not a `list[scalar]` parameter — only a list param (i32 \
             base-pointer into linear memory) can be element-assigned in the \
             WASM subset (no str/dict/tuple element-assignment)"
        )));
    };
    // addr = base + 8 + (i as i32)*stride, bounds-checked (shared with the
    // read path). Leaves the i32 address on the stack.
    emit_list_elem_addr(list_name, elem, index, scope, out, depth)?;
    // value — must lower to the element's WAT type (else an honest mismatch).
    emit_expr_typed(value, scope, out, depth, elem)?;
    // store the value at the computed address (consumes addr + value).
    indent(out, depth);
    writeln!(out, "{}", elem.store_instr()).expect("write");
    Ok(())
}

/// Emit `len(xs)` over a `list[scalar]` parameter (PMAT-968) or `len(s)`
/// over a `str` parameter (PMAT-986). Both lower IDENTICALLY: read the `i32`
/// count header at `base+0` and zero-extend it to the `i64` Python-int
/// domain (`len` returns a non-negative Python `int`). For a list the header
/// is the element count; for a str it is the UTF-8 **byte** count.
///
/// PMAT-986 (str): the byte count equals the Python char count ONLY for
/// ASCII — a multi-byte UTF-8 string has byte_count > char_count, so this
/// `len(s)` reports the byte count, which is the honest ASCII-restricted
/// posture (callers pass ASCII; the emitter cannot cheaply count chars
/// without scanning UTF-8 continuation bytes, deferred with the rest of the
/// string runtime). For an ASCII fixture the executed witness asserts the
/// value matches CPython `len`.
///
/// `collection` must be an [`Expr::Ident`] naming a list-param OR str-param
/// base-pointer; `len` over anything else in the WASM subset (a scalar, a
/// dict, a literal/temporary) is refused — only a length-prefixed list/str
/// param carries a length header.
fn emit_len(
    collection: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let Expr::Ident(name) = collection else {
        return Err(unsupported(
            "len() of a non-name collection — the WASM subset only takes \
             len() of a `list[scalar]` or `str` PARAMETER (its i32 length \
             header); len of a list literal / dict / temporary is refused",
        ));
    };
    if scope.list_elem_of(name).is_none()
        && !scope.is_str_param(name)
        && scope.heap_map_kind(name).is_none()
    {
        return Err(unsupported(&format!(
            "len() over `{name}` which is not a `list[scalar]`/`str` parameter \
             or a `dict`/`set` local — only those carry the i32 count header at \
             base+0 in the WASM subset"
        )));
    }
    // PMAT-995: len = (i32 header at base+0) zero-extended to i64. Identical for
    // a list (element count), a str (byte count), AND a dict/set (live-entry
    // count) — all three share the `+0` i32 count header.
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load").expect("write");
    indent(out, depth);
    writeln!(out, "i64.extend_i32_u").expect("write");
    Ok(WatTy::I64)
}

/// Emit `ord(s[i])` over a `str` parameter (PMAT-986) — the one
/// string-reading op slice 1 supports that returns an `int` (a code point)
/// rather than a new string. The meta-HIR shape is `Expr::Ord { value:
/// Expr::StrCharAt { string: Ident(s), index } }`: the frontend lowers
/// Python `ord(s[i])` to exactly that, and lowering the `StrCharAt` here
/// (instead of materialising a 1-char string) avoids any allocation.
///
/// Lowers to a bounds-checked `i32.load8_u` of the `i`-th UTF-8 byte:
///   1. evaluate `i` once into the scratch `i64` `$__wasm_idx`;
///   2. bounds guard — `i < 0 || i >= byte_count → unreachable` — the
///      Python `IndexError` analogue, reusing the SAME header read the list
///      index path uses (`byte_count` is the i32 header at `base+0`);
///   3. `addr = base + LIST_ELEMS_OFFSET + (i as i32)` (byte stride 1);
///   4. `i32.load8_u` the byte, then `i64.extend_i32_u` to the Python-int
///      domain (a byte is 0..=255; for ASCII this is the code point, exactly
///      CPython's `ord`).
///
/// Any other `ord` operand is refused: `ord` of a non-`StrCharAt` (e.g.
/// `ord(chr(n))`, `ord` of a whole-string `Ident`) needs a materialised
/// char and is outside slice 1; an `s[i]` whose base is not a str param is
/// likewise refused.
fn emit_ord(
    value: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let Expr::StrCharAt { string, index } = value else {
        return Err(unsupported(
            "ord() of a non-`s[i]` operand — the WASM subset lowers `ord(s[i])` \
             over a `str` PARAMETER to a bounds-checked i32.load8_u of byte i; \
             ord() of a whole string, of `chr(n)`, or of a literal is refused",
        ));
    };
    let Expr::Ident(name) = string.as_ref() else {
        return Err(unsupported(
            "ord(s[i]) where the indexed value is not a name — only `ord(s[i])` \
             over a `str` parameter (i32 base-pointer) is supported",
        ));
    };
    if !scope.is_str_param(name) {
        return Err(unsupported(&format!(
            "ord({name}[i]) where `{name}` is not a `str` parameter — only a \
             str param (i32 base-pointer into linear memory) supports \
             per-byte ord() in the WASM subset"
        )));
    }
    // Bounds-checked byte address: base + 8 + (i as i32)*1, with the
    // `i < 0 || i >= byte_count → unreachable` guard. Reuse the shared
    // list-element address helper with a synthetic 1-byte stride: the i32
    // path multiplies by `elem.byte_size()`, so a single-byte stride needs a
    // dedicated emit (we cannot pass a 1-byte `WatTy`). Emit it inline here,
    // mirroring `emit_list_elem_addr` but with stride 1 and a load8.
    emit_str_byte_addr(name, index, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "i32.load8_u").expect("write");
    indent(out, depth);
    writeln!(out, "i64.extend_i32_u").expect("write");
    Ok(WatTy::I64)
}

/// Emit the bounds-checked linear-memory ADDRESS of the `index`-th UTF-8
/// byte of the `str` parameter `name` onto the WASM stack (PMAT-986).
///
/// The str-byte sibling of [`emit_list_elem_addr`]: identical bounds-guard
/// shape (`i < 0 || i >= byte_count → unreachable`, the Python `IndexError`
/// analogue), but a **byte** stride of 1 (no `*size` multiply) and reading
/// the count header as a UTF-8 byte count. Leaves `addr = base +
/// LIST_ELEMS_OFFSET + (i as i32)` on the stack for an `i32.load8_u`.
fn emit_str_byte_addr(
    name: &str,
    index: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // Evaluate the index once into the per-function scratch i64.
    emit_expr_typed(index, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "local.set ${IDX_SCRATCH}").expect("write");

    // Bounds guard: if (i < 0) | (i >= byte_count) { unreachable }.
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i64.const 0").expect("write");
    indent(out, depth);
    writeln!(out, "i64.lt_s").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load").expect("write"); // header byte count
    indent(out, depth);
    writeln!(out, "i64.extend_i32_u").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i64.le_s").expect("write"); // byte_count <= i  ⇔  i >= byte_count
    indent(out, depth);
    writeln!(out, "i32.or").expect("write");
    indent(out, depth);
    writeln!(out, "if").expect("write");
    indent(out, depth + 1);
    writeln!(out, "unreachable").expect("write");
    indent(out, depth);
    writeln!(out, "end").expect("write");

    // addr = base + LIST_ELEMS_OFFSET + (index as i32)  (byte stride 1).
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {LIST_ELEMS_OFFSET}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.wrap_i64").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");
    Ok(())
}

/// PMAT-993: emit a string-VALUED expression, leaving its `i32` base-pointer
/// (into the length-prefixed linear-memory region) on the WASM stack.
///
/// The string-valued forms are: a `str` PARAMETER (`Expr::Ident` of a str
/// param — already a base-pointer), a string LITERAL (PMAT-994 `Expr::LitStr`,
/// a constant static-`(data)` base-pointer), a `Concat` (string `+`,
/// materialised in the heap), a `Chr` (a new 1-char string), and a bare
/// `StrCharAt` (PMAT-994 `s[i]` as a new 1-char heap string). Any other
/// expression in a string position is refused.
///
/// Used by a `str`-returning function's trailing return and (transitively, via
/// `concat_operands`) by nested concat. A str param, a literal pointer, and a
/// heap string ALL share the SAME length-prefixed ABI (i32 byte-count header at
/// base+0, UTF-8 bytes at base+8), so this uniform base-pointer is enough for
/// `len` / byte-copy / content equality.
fn emit_str_expr(
    e: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    match e {
        Expr::Ident(name) if scope.is_str_param(name) => {
            indent(out, depth);
            writeln!(out, "local.get ${name}").expect("write");
            Ok(())
        }
        Expr::Ident(name) => Err(unsupported(&format!(
            "string-position use of `{name}` which is not a `str` parameter — \
             the WASM string subset has no str locals (only str params, string \
             literals, and heap-constructed Concat/Chr/s[i] results)"
        ))),
        // PMAT-994: a string LITERAL is a constant pointer to its static
        // `(data …)` segment in `[LITERAL_BASE, HEAP_BASE)`. It shares the
        // length-prefixed ABI, so it composes with concat / len / equality.
        Expr::LitStr(s) => {
            let addr = scope.literal_addr(s).ok_or_else(|| {
                unsupported(&format!(
                    "string literal {s:?} was not laid out into a static (data) \
                     segment — internal layout error"
                ))
            })?;
            indent(out, depth);
            writeln!(out, "i32.const {addr}").expect("write");
            Ok(())
        }
        Expr::Concat { lhs, rhs } => {
            emit_concat(lhs, rhs, scope, out, depth)?;
            Ok(())
        }
        Expr::Chr { value } => {
            emit_chr(value, scope, out, depth)?;
            Ok(())
        }
        // PMAT-994: `s[i]` used AS a 1-char string — materialise a new 1-char
        // heap string (the `chr` mirror, but copying byte `i` of `s` with the
        // same bounds guard `ord(s[i])` uses).
        Expr::StrCharAt { string, index } => {
            emit_str_char_at(string, index, scope, out, depth)?;
            Ok(())
        }
        other => Err(unsupported(&format!(
            "expression {} in a string position — the WASM string subset \
             returns a `str` param, a string literal, a `Concat` (a + b), a \
             `Chr` (chr(n)), or `s[i]`; slicing / str() / f-strings are refused",
            expr_kind(other)
        ))),
    }
}

/// PMAT-993: push the `i32` BYTE LENGTH of a string-valued expression onto the
/// stack — the i32 count header at its `base+0`. The expression's base-pointer
/// is evaluated by `emit_str_expr`; this appends the header `i32.load`.
fn emit_str_len_i32(
    e: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    emit_str_expr(e, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "i32.load").expect("write");
    Ok(())
}

/// PMAT-993: lower string concatenation `a + b` (`Expr::Concat`) — the
/// headline string-RETURNING op of slice 2.
///
/// Flattens a left-nested `Concat` tree (`((a + b) + c)`, the frontend's
/// left-assoc shape) into its leaf operands and joins them in ONE pass:
///   1. total = Σ len(opᵢ)   (each header i32.load);
///   2. dst = __alloc(8 + total);
///   3. store the i32 count header `total` at dst+0;
///   4. for each operand, `memory.copy` its `len` UTF-8 bytes from
///      `opᵢ + 8` to `dst + 8 + running_offset`, advancing the offset;
///   5. leave `dst` (the new string's base-pointer) on the stack.
///
/// Single-pass (no nested heap allocation) so the per-function scratch locals
/// never alias. Each leaf operand must itself be string-valued
/// (`emit_str_expr`): a str param, or a `Chr`. A non-str operand is refused.
fn emit_concat(
    lhs: &Expr,
    rhs: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // Flatten the (left-nested) concat tree into its ordered leaf operands.
    let mut operands: Vec<&Expr> = Vec::new();
    flatten_concat(lhs, &mut operands);
    flatten_concat(rhs, &mut operands);

    // total_bytes = Σ len(opᵢ): push each operand's i32 byte length and add.
    emit_str_len_i32(operands[0], scope, out, depth)?;
    for op in &operands[1..] {
        emit_str_len_i32(op, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "i32.add").expect("write");
    }
    // dst = __alloc(8 + total_bytes): add the header size, call the allocator,
    // and stash the base-pointer in the DEDICATED concat-dst local (PMAT-998:
    // NOT $__wasm_str_dst — an operand's own string-returning eval clobbers that).
    indent(out, depth);
    writeln!(out, "i32.const {LIST_ELEMS_OFFSET}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");
    indent(out, depth);
    writeln!(out, "call $__alloc").expect("write");
    indent(out, depth);
    writeln!(out, "local.set ${STR_CONCAT_DST}").expect("write");

    // store the count header (total_bytes) at dst+0. Recompute the total from
    // the operand lengths (cheap header loads) so it lands in the header slot.
    indent(out, depth);
    writeln!(out, "local.get ${STR_CONCAT_DST}").expect("write");
    emit_str_len_i32(operands[0], scope, out, depth)?;
    for op in &operands[1..] {
        emit_str_len_i32(op, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "i32.add").expect("write");
    }
    indent(out, depth);
    writeln!(out, "i32.store").expect("write");

    // Copy each operand's bytes to dst+8+offset, tracking the running offset
    // in the DEDICATED $__wasm_concat_off local (PMAT-1000: NOT $__wasm_str_la —
    // an `s[i]` operand uses that as its own source-base scratch). Start at 0.
    indent(out, depth);
    writeln!(out, "i32.const 0").expect("write");
    indent(out, depth);
    writeln!(out, "local.set ${STR_CONCAT_OFF}").expect("write");
    for op in &operands {
        // memory.copy(dest = dst+8+offset, src = op+8, n = len(op))
        // dest: (the dedicated concat-dst local, which survives the operand's
        // own string-returning eval below — PMAT-998).
        indent(out, depth);
        writeln!(out, "local.get ${STR_CONCAT_DST}").expect("write");
        indent(out, depth);
        writeln!(out, "i32.const {LIST_ELEMS_OFFSET}").expect("write");
        indent(out, depth);
        writeln!(out, "i32.add").expect("write");
        indent(out, depth);
        writeln!(out, "local.get ${STR_CONCAT_OFF}").expect("write");
        indent(out, depth);
        writeln!(out, "i32.add").expect("write");
        // src = op_base + 8:
        emit_str_expr(op, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "i32.const {LIST_ELEMS_OFFSET}").expect("write");
        indent(out, depth);
        writeln!(out, "i32.add").expect("write");
        // n = len(op):
        emit_str_len_i32(op, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "memory.copy").expect("write");
        // offset += len(op):
        indent(out, depth);
        writeln!(out, "local.get ${STR_CONCAT_OFF}").expect("write");
        emit_str_len_i32(op, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "i32.add").expect("write");
        indent(out, depth);
        writeln!(out, "local.set ${STR_CONCAT_OFF}").expect("write");
    }
    // result = dst (the new string's base-pointer, from the dedicated concat-dst
    // local — never the operand-clobbered $__wasm_str_dst; PMAT-998).
    indent(out, depth);
    writeln!(out, "local.get ${STR_CONCAT_DST}").expect("write");
    Ok(())
}

/// Flatten a left-nested `Expr::Concat` into its ordered leaf operands. A
/// `Concat` node recurses; any other expression is a leaf (validated as
/// string-valued later by `emit_str_expr`).
fn flatten_concat<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Concat { lhs, rhs } = e {
        flatten_concat(lhs, out);
        flatten_concat(rhs, out);
    } else {
        out.push(e);
    }
}

// ─── PMAT-995 (slice 3b): dict / set over the bump heap ──────────────────────

/// Lower a `dict`/`set` BINDING value — its `DictLit`/`SetLit` — onto the bump
/// heap, leaving the new region's `i32` base-pointer on the stack (the caller
/// `local.set`s it). A dict/set-returning call / comprehension is refused (no
/// dict-returning op in the WASM subset).
fn emit_heap_map_bind(
    value: &Expr,
    kind: KeyKind,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    match value {
        Expr::DictLit(pairs) => emit_dict_lit(pairs, kind, scope, out, depth),
        Expr::SetLit(elems) => emit_set_lit(elems, kind, scope, out, depth),
        other => Err(unsupported(&format!(
            "a `dict`/`set` binding must be a dict/set LITERAL in the WASM subset \
             (a dict/set-returning call, comprehension, or copy is refused) — got {}",
            expr_kind(other)
        ))),
    }
}

/// `$__alloc` a fresh dict/set region of `cap` slots, write its `[count=0][cap]`
/// header, and stash the base-pointer in [`DICT_DST_SCRATCH`]. Construction
/// starts EMPTY (count 0); each literal entry is then update-or-inserted via
/// `$__wasm_dict_set_<k>`, so a DUPLICATE key collapses (CPython last-wins +
/// distinct live count).
fn emit_map_alloc(cap: i32, out: &mut String, depth: usize) {
    let size = LIST_ELEMS_OFFSET + cap * DICT_ENTRY_SIZE;
    // dst = __alloc(8 + cap*16)
    indent(out, depth);
    writeln!(out, "i32.const {size}").expect("write");
    indent(out, depth);
    writeln!(out, "call $__alloc").expect("write");
    indent(out, depth);
    writeln!(out, "local.set ${DICT_DST_SCRATCH}").expect("write");
    // header: count = 0 at dst+0 (entries are inserted below, incrementing it)
    indent(out, depth);
    writeln!(out, "local.get ${DICT_DST_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const 0").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store").expect("write");
    // header: capacity = cap at dst+4
    indent(out, depth);
    writeln!(out, "local.get ${DICT_DST_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {cap}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store offset={DICT_CAP_OFFSET}").expect("write");
}

/// Push a dict/set KEY: an `i64` for an int key, the `i32` string base-pointer
/// for a str key. Used by both literal construction and the op lowerings.
fn emit_dict_key(
    key: &Expr,
    kind: KeyKind,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    match kind {
        KeyKind::Int => emit_expr_typed(key, scope, out, depth, WatTy::I64),
        // A str key is a string-VALUED expr (a str param, a literal); its i32
        // base-pointer is the stored/compared key.
        KeyKind::Str => emit_str_expr(key, scope, out, depth),
    }
}

/// Lower a `DictLit` — Python `{k0: v0, k1: v1, …}` — onto the bump heap. Builds
/// an EMPTY region then update-or-inserts each pair (in source order) via
/// `$__wasm_dict_set_<k>`, so a DUPLICATE key keeps the LAST value and the live
/// count is the number of DISTINCT keys — matching CPython (`{1: 10, 1: 20}` ==
/// `{1: 20}`, len 1). A blind sequential write (the pre-fix path) kept both
/// entries: wrong len + a first-wins lookup. `cap = len + slack` guarantees the
/// inserts never trap on capacity (dedup only shrinks the live count).
fn emit_dict_lit(
    pairs: &[(Expr, Expr)],
    kind: KeyKind,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let cap = pairs.len() as i32 + DICT_GROWTH_SLACK;
    emit_map_alloc(cap, out, depth);
    for (k, v) in pairs {
        indent(out, depth);
        writeln!(out, "local.get ${DICT_DST_SCRATCH}").expect("write");
        emit_dict_key(k, kind, scope, out, depth)?;
        emit_expr_typed(v, scope, out, depth, WatTy::I64)?;
        indent(out, depth);
        writeln!(out, "call $__wasm_dict_set_{}", kind.suffix()).expect("write");
    }
    // result = dst (the new dict's base-pointer).
    indent(out, depth);
    writeln!(out, "local.get ${DICT_DST_SCRATCH}").expect("write");
    Ok(())
}

/// Lower a `SetLit` — Python `{e0, e1, …}` — onto the bump heap (a keys-only
/// dict; each entry's value is the `0` sentinel). Builds EMPTY then inserts each
/// elem via `$__wasm_dict_set_<k>`, so a DUPLICATE elem is dropped and len is the
/// distinct count — matching CPython (`{5, 5, 6}` has len 2).
fn emit_set_lit(
    elems: &[Expr],
    kind: KeyKind,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let cap = elems.len() as i32 + DICT_GROWTH_SLACK;
    emit_map_alloc(cap, out, depth);
    for e in elems {
        indent(out, depth);
        writeln!(out, "local.get ${DICT_DST_SCRATCH}").expect("write");
        emit_dict_key(e, kind, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "i64.const 0").expect("write");
        indent(out, depth);
        writeln!(out, "call $__wasm_dict_set_{}", kind.suffix()).expect("write");
    }
    indent(out, depth);
    writeln!(out, "local.get ${DICT_DST_SCRATCH}").expect("write");
    Ok(())
}

/// Resolve a dict/set op's receiver to `(name, key_kind)`, refusing a non-name
/// or non-dict/set receiver honestly.
fn dict_ident_kind<'e>(e: &'e Expr, scope: &Scope) -> Result<(&'e str, KeyKind), BackendError> {
    let Expr::Ident(name) = e else {
        return Err(unsupported(
            "a dict/set op (d[k] / k in d) over a non-name receiver — the WASM \
             subset operates on a `dict`/`set` LOCAL only (no temporaries)",
        ));
    };
    let kind = scope.heap_map_kind(name).ok_or_else(|| {
        unsupported(&format!(
            "dict/set op over `{name}` which is not a `dict`/`set` local in the \
             WASM subset"
        ))
    })?;
    Ok((name.as_str(), kind))
}

/// Lower `d[k]` (`Expr::DictGet`) — push the dict base + key, call the keyed
/// `get` helper (returns the i64 value or TRAPS on an absent key, the Python
/// KeyError analogue).
fn emit_dict_get(
    dict: &Expr,
    key: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let (name, kind) = dict_ident_kind(dict, scope)?;
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    emit_dict_key(key, kind, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_dict_get_{}", kind.suffix()).expect("write");
    Ok(WatTy::I64)
}

/// Lower `k in d` / `x in s` (`Expr::DictContains` / `Expr::SetContains`) — push
/// the base + key, call the keyed `has` helper (i32 bool, never traps).
fn emit_dict_contains(
    dict: &Expr,
    key: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let (name, kind) = dict_ident_kind(dict, scope)?;
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    emit_dict_key(key, kind, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_dict_has_{}", kind.suffix()).expect("write");
    Ok(WatTy::I32)
}

/// Lower `d[k] = v` (`Stmt::DictSet`) — push base + key + i64 value, call the
/// keyed `set` helper (update-or-insert; traps if at capacity).
fn emit_dict_set(
    dict_name: &str,
    key: &Expr,
    value: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let kind = scope.heap_map_kind(dict_name).ok_or_else(|| {
        unsupported(&format!(
            "`{dict_name}[k] = v` over `{dict_name}` which is not a `dict` local \
             in the WASM subset"
        ))
    })?;
    indent(out, depth);
    writeln!(out, "local.get ${dict_name}").expect("write");
    emit_dict_key(key, kind, scope, out, depth)?;
    emit_expr_typed(value, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_dict_set_{}", kind.suffix()).expect("write");
    Ok(())
}

/// Lower `s.add(e)` (`Stmt::SetAdd`) — push base + key + the `0` sentinel value,
/// call the keyed `set` helper (a set is a keys-only dict).
fn emit_set_add(
    set_name: &str,
    elem: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let kind = scope.heap_map_kind(set_name).ok_or_else(|| {
        unsupported(&format!(
            "`{set_name}.add(e)` over `{set_name}` which is not a `set` local in \
             the WASM subset"
        ))
    })?;
    indent(out, depth);
    writeln!(out, "local.get ${set_name}").expect("write");
    emit_dict_key(elem, kind, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "i64.const 0").expect("write");
    indent(out, depth);
    writeln!(out, "call $__wasm_dict_set_{}", kind.suffix()).expect("write");
    Ok(())
}

// ─── PMAT-996 (slice 4): plain-data structs over the bump heap ────────────────

/// A struct's field layout: `(struct_name, fields)` where `fields` is the
/// `(field_name, WatTy)` list in DEFINITION order. Field `i` lives at
/// `base + i*STRUCT_FIELD_SIZE` (a uniform 8-byte slot). Built once per module
/// from its `Item::Struct` definitions; carried in [`Scope::structs`].
type StructRegistry = Vec<(String, Vec<(String, WatTy)>)>;

/// Build the module's struct layout registry, refusing any struct whose field
/// type is outside the WASM scalar subset (an honest early refusal — a
/// str/list/dict/set/nested-struct field has no flat 8-byte-slot layout yet).
fn build_struct_registry(module: &Module) -> Result<StructRegistry, BackendError> {
    let mut reg: StructRegistry = Vec::new();
    for item in &module.items {
        if let Item::Struct { name, fields, .. } = item {
            let mut flds = Vec::with_capacity(fields.len());
            for (fname, fty) in fields {
                let wt = map_type(fty).map_err(|_| {
                    unsupported(&format!(
                        "struct `{name}` field `{fname}`: type {fty:?} — the WASM \
                         struct subset supports SCALAR fields (i64/i32/f64/f32/bool) \
                         only; str/list/dict/set/nested-struct fields are refused"
                    ))
                })?;
                flds.push((fname.clone(), wt));
            }
            reg.push((name.clone(), flds));
        }
    }
    Ok(reg)
}

/// Look up a struct's field layout by name.
fn struct_layout<'r>(
    reg: &'r StructRegistry,
    name: &str,
) -> Result<&'r [(String, WatTy)], BackendError> {
    reg.iter()
        .find(|(n, _)| n == name)
        .map(|(_, f)| f.as_slice())
        .ok_or_else(|| {
            unsupported(&format!(
                "struct `{name}` has no definition in this module (the WASM subset \
                 lowers a struct only alongside its `class`/`@dataclass` definition)"
            ))
        })
}

/// Lower an `Expr::StructLit` (`Name(f0=v0, …)`) onto the bump heap: `$__alloc`
/// `n_fields * 8` bytes, write each DEFINITION-order field into its 8-byte slot,
/// and leave the instance's `i32` base-pointer on the stack. A missing field is
/// refused honestly.
fn emit_struct_lit(
    name: &str,
    lit_fields: &[(String, Expr)],
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let layout = struct_layout(scope.structs, name)?.to_vec();
    let size = layout.len() as i32 * STRUCT_FIELD_SIZE;
    // dst = __alloc(n*8)
    indent(out, depth);
    writeln!(out, "i32.const {size}").expect("write");
    indent(out, depth);
    writeln!(out, "call $__alloc").expect("write");
    indent(out, depth);
    writeln!(out, "local.set ${STRUCT_DST_SCRATCH}").expect("write");
    // Write each field in DEFINITION order (offset = i*8), so field position is
    // independent of the literal's argument order.
    for (i, (fname, fty)) in layout.iter().enumerate() {
        let value = lit_fields
            .iter()
            .find(|(n, _)| n == fname)
            .map(|(_, v)| v)
            .ok_or_else(|| {
                unsupported(&format!(
                    "struct literal `{name}` is missing field `{fname}`"
                ))
            })?;
        indent(out, depth);
        writeln!(out, "local.get ${STRUCT_DST_SCRATCH}").expect("write");
        emit_expr_typed(value, scope, out, depth, *fty)?;
        indent(out, depth);
        writeln!(
            out,
            "{}.store offset={}",
            fty.keyword(),
            i as i32 * STRUCT_FIELD_SIZE
        )
        .expect("write");
    }
    indent(out, depth);
    writeln!(out, "local.get ${STRUCT_DST_SCRATCH}").expect("write");
    Ok(WatTy::I32)
}

/// Lower an `Expr::FieldAccess` (`obj.field`) — load the field from the struct
/// instance `obj` points at. `obj` must be an [`Expr::Ident`] naming a struct
/// LOCAL or PARAM (an `i32` base-pointer); the field's offset + WAT type come
/// from the struct registry. Returns the field's WAT type.
fn emit_field_access(
    obj: &Expr,
    field: &str,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let Expr::Ident(oname) = obj else {
        return Err(unsupported(
            "field access `.f` over a non-name receiver — the WASM subset reads a \
             field from a struct LOCAL/PARAM only (no nested/temporary receivers)",
        ));
    };
    let sname = scope.struct_of(oname).ok_or_else(|| {
        unsupported(&format!(
            "field access over `{oname}` which is not a struct local/param in the \
             WASM subset"
        ))
    })?;
    let layout = struct_layout(scope.structs, &sname)?;
    let (idx, (_, fty)) = layout
        .iter()
        .enumerate()
        .find(|(_, (fn_, _))| fn_ == field)
        .ok_or_else(|| unsupported(&format!("struct `{sname}` has no field `{field}`")))?;
    indent(out, depth);
    writeln!(out, "local.get ${oname}").expect("write");
    indent(out, depth);
    writeln!(
        out,
        "{}.load offset={}",
        fty.keyword(),
        idx as i32 * STRUCT_FIELD_SIZE
    )
    .expect("write");
    Ok(*fty)
}

/// PMAT-993: lower `chr(n)` (`Expr::Chr`) — materialise a new 1-byte string
/// holding the byte `n & 0xFF` and leave its `i32` base-pointer on the stack.
///
/// ASCII-bounded (slice 2's honest restriction, the slice-1 `ord` mirror): a
/// code point ≥ 128 needs multi-byte UTF-8 encoding, deferred with the rest of
/// the string runtime; this writes the low byte, exact for `0 ≤ n < 128`. The
/// result is a length-prefixed string (count header = 1 at base+0, the byte at
/// base+8), so it composes with `Concat` and a `str` return uniformly.
fn emit_chr(
    value: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // dst = __alloc(8 + 1) = a 1-byte string region.
    indent(out, depth);
    writeln!(out, "i32.const {}", LIST_ELEMS_OFFSET + 1).expect("write");
    indent(out, depth);
    writeln!(out, "call $__alloc").expect("write");
    indent(out, depth);
    writeln!(out, "local.set ${STR_DST_SCRATCH}").expect("write");
    // header: count = 1 at dst+0.
    indent(out, depth);
    writeln!(out, "local.get ${STR_DST_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const 1").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store").expect("write");
    // byte: (n & 0xFF) at dst+8 via i32.store8. The code point `n` is an i64
    // Python int; narrow to i32 and mask the low byte.
    indent(out, depth);
    writeln!(out, "local.get ${STR_DST_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {LIST_ELEMS_OFFSET}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");
    emit_expr_typed(value, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "i32.wrap_i64").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const 255").expect("write");
    indent(out, depth);
    writeln!(out, "i32.and").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store8").expect("write");
    // result = dst.
    indent(out, depth);
    writeln!(out, "local.get ${STR_DST_SCRATCH}").expect("write");
    Ok(())
}

/// PMAT-994 (slice 3a): lower `s[i]` used AS a 1-char string
/// (`Expr::StrCharAt` outside an `ord`) — materialise a NEW 1-char heap string
/// holding byte `i` of the string-valued base, and leave its `i32`
/// base-pointer on the stack.
///
/// The `chr` mirror, but the byte comes from `s[i]` (bounds-checked) rather
/// than a masked int. Works over ANY string-valued base — a str param, a
/// string literal, or a heap string — since all share the length-prefixed ABI.
/// The base pointer is evaluated once into [`STR_LA_SCRATCH`] (reused here as
/// the source base-pointer scratch), the index once into [`IDX_SCRATCH`], then:
///   1. bounds guard `i < 0 || i >= byte_count → unreachable` (Python
///      `IndexError`), reading the source header count;
///   2. `dst = __alloc(9)`, count-1 header at `dst+0`;
///   3. copy `src[8 + i]` to `dst+8` via `i32.load8_u` / `i32.store8`;
///   4. leave `dst`.
///
/// ASCII-faithful: a 1-BYTE copy is the char only for ASCII (the slice-1/2
/// honest restriction the whole str path carries); a multi-byte UTF-8 char
/// would copy one byte. Callers pass ASCII (documented).
fn emit_str_char_at(
    string: &Expr,
    index: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // src = the string-valued base pointer, evaluated once into STR_LA_SCRATCH.
    emit_str_expr(string, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "local.set ${STR_LA_SCRATCH}").expect("write");
    // i = the index, evaluated once into IDX_SCRATCH (i64).
    emit_expr_typed(index, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "local.set ${IDX_SCRATCH}").expect("write");

    // Bounds guard: if (i < 0) | (i >= byte_count) { unreachable }.
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i64.const 0").expect("write");
    indent(out, depth);
    writeln!(out, "i64.lt_s").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${STR_LA_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load").expect("write"); // header byte count
    indent(out, depth);
    writeln!(out, "i64.extend_i32_u").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i64.le_s").expect("write"); // byte_count <= i  ⇔  i >= byte_count
    indent(out, depth);
    writeln!(out, "i32.or").expect("write");
    indent(out, depth);
    writeln!(out, "if").expect("write");
    indent(out, depth + 1);
    writeln!(out, "unreachable").expect("write");
    indent(out, depth);
    writeln!(out, "end").expect("write");

    // dst = __alloc(8 + 1) = a 1-byte string region.
    indent(out, depth);
    writeln!(out, "i32.const {}", LIST_ELEMS_OFFSET + 1).expect("write");
    indent(out, depth);
    writeln!(out, "call $__alloc").expect("write");
    indent(out, depth);
    writeln!(out, "local.set ${STR_DST_SCRATCH}").expect("write");
    // header: count = 1 at dst+0.
    indent(out, depth);
    writeln!(out, "local.get ${STR_DST_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const 1").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store").expect("write");
    // dst[8] = src[8 + i]. Store consumes (addr, value): push dst+8, then the
    // source byte (load8_u of src + 8 + i), then i32.store8.
    indent(out, depth);
    writeln!(out, "local.get ${STR_DST_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {LIST_ELEMS_OFFSET}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");
    // source byte addr = src + LIST_ELEMS_OFFSET + (i as i32).
    indent(out, depth);
    writeln!(out, "local.get ${STR_LA_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {LIST_ELEMS_OFFSET}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.wrap_i64").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load8_u").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store8").expect("write");
    // result = dst.
    indent(out, depth);
    writeln!(out, "local.get ${STR_DST_SCRATCH}").expect("write");
    Ok(())
}

/// Render an `f64` as a WAT float literal token.
fn wat_float_literal(v: f64) -> String {
    if v.is_nan() {
        "nan".to_string()
    } else if v.is_infinite() {
        if v < 0.0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        }
    } else {
        // `{:?}` on f64 always renders a decimal point (e.g. `2.0`),
        // which WAT requires to parse the token as a float.
        format!("{v:?}")
    }
}

fn emit_unop(
    op: UnOp,
    operand: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    match op {
        UnOp::Neg => {
            let t = emit_expr(operand, scope, out, depth)?;
            indent(out, depth);
            match t {
                WatTy::I64 => {
                    // -x == 0 - x; checked-overflow on i64::MIN matches the
                    // Rust lane (negation of MIN traps).
                    writeln!(out, "i64.const -1\n").expect("write");
                    indent(out, depth);
                    writeln!(out, "i64.mul").expect("write");
                }
                WatTy::I32 => {
                    return Err(unsupported("unary negation of a bool/i32 value"));
                }
                WatTy::F64 => {
                    writeln!(out, "f64.neg").expect("write");
                }
                WatTy::F32 => {
                    writeln!(out, "f32.neg").expect("write");
                }
            }
            Ok(t)
        }
        UnOp::Not => {
            // Logical not over a bool (i32 0/1): x == 0.
            let t = emit_expr(operand, scope, out, depth)?;
            if t != WatTy::I32 {
                return Err(unsupported("logical `not` of a non-bool value"));
            }
            indent(out, depth);
            writeln!(out, "i32.eqz").expect("write");
            Ok(WatTy::I32)
        }
        UnOp::BitNot => {
            // ~x == x XOR -1 over i64.
            let t = emit_expr(operand, scope, out, depth)?;
            if t != WatTy::I64 {
                return Err(unsupported("bitwise `~` of a non-i64 value"));
            }
            indent(out, depth);
            writeln!(out, "i64.const -1").expect("write");
            indent(out, depth);
            writeln!(out, "i64.xor").expect("write");
            Ok(WatTy::I64)
        }
    }
}

/// PMAT-994: `true` if `e` is a string-VALUED binop operand — a str param
/// `Ident`, a string literal, a `Concat`, a `Chr`, or a bare `StrCharAt`. Such
/// an operand is an i32 base-pointer, NOT an arithmetic/bool value; a `==`/`!=`
/// over it routes to the content-compare helper, any other op is refused.
fn binop_operand_is_string(e: &Expr, scope: &Scope) -> bool {
    match e {
        Expr::Ident(name) => scope.is_str_param(name),
        Expr::LitStr(_) | Expr::Concat { .. } | Expr::Chr { .. } | Expr::StrCharAt { .. } => true,
        _ => false,
    }
}

fn emit_binop(
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    // PMAT-986/994: a `str` lowers to an `i32` base-pointer, INDISTINGUISHABLE
    // from a bool `i32` in the opcode table below — so a naive `a < b` over two
    // strings would silently compare BASE-POINTERS (wrong code). PMAT-994 wires
    // string EQUALITY (`a == b` / `a != b`) via a real content-compare helper
    // (`$__wasm_str_eq`); ordering / arithmetic / methods over strings stay
    // refused (they need ordering / content logic not yet wired).
    if binop_operand_is_string(lhs, scope) || binop_operand_is_string(rhs, scope) {
        // PMAT-994: string content EQUALITY — `a == b` / `a != b` over two
        // string-valued operands. Lower to `$__wasm_str_eq(a, b)` (a length
        // check + byte-compare loop → i32 bool), inverting for `!=`. This is
        // REAL string-content logic — never a base-pointer compare — correct
        // for params, literals, and heap strings (all share the ABI).
        if matches!(op, BinOp::Eq | BinOp::NotEq) {
            // Both operands must be string-valued (no `str == int`).
            if !(binop_operand_is_string(lhs, scope) && binop_operand_is_string(rhs, scope)) {
                return Err(unsupported(&format!(
                    "binary op {op:?} mixing a `str` operand with a non-`str` \
                     operand — string equality compares two strings; a mixed \
                     comparison is refused"
                )));
            }
            emit_str_expr(lhs, scope, out, depth)?;
            emit_str_expr(rhs, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_eq").expect("write");
            if matches!(op, BinOp::NotEq) {
                indent(out, depth);
                writeln!(out, "i32.eqz").expect("write"); // negate: != is !(==)
            }
            return Ok(WatTy::I32);
        }
        // PMAT-993: string concat `a + b` is the frontend's `Expr::Concat`,
        // lowered through the heap path (`emit_concat`). A *`BinOp::Add`* over
        // str base-pointers is genuine (meaningless) pointer arithmetic, NOT
        // string concat — point the caller at `Concat`, don't silently do it.
        if matches!(op, BinOp::Add) {
            return Err(unsupported(
                "BinOp::Add over `str` base-pointers — this is pointer \
                 arithmetic, not string concatenation. String `+` lowers as \
                 `Expr::Concat`, which the WASM lane supports via the heap \
                 allocator (PMAT-993); a raw `BinOp::Add` over str pointers is \
                 refused",
            ));
        }
        return Err(unsupported(&format!(
            "binary op {op:?} over `str` operand(s) — string ORDERING (`<` / \
             `>` / …) / methods are not in the WASM string subset (only \
             read-only `len(s)` + `ord(s[i])` + heap `Concat`/`chr`/`s[i]` + \
             content equality `==`/`!=`); ordering needs lexicographic logic \
             not yet wired, refused honestly rather than comparing base-pointers"
        )));
    }

    // Logical and/or short-circuit — emit as nested if-expressions over
    // i32 booleans (matches Python/Rust short-circuit semantics).
    if matches!(op, BinOp::And | BinOp::Or) {
        return emit_logical(op, lhs, rhs, scope, out, depth);
    }

    let lt = emit_expr(lhs, scope, out, depth)?;
    let rt = emit_expr(rhs, scope, out, depth)?;
    if lt != rt {
        return Err(unsupported(&format!(
            "binary op {op:?} over mixed WASM types ({} and {})",
            lt.keyword(),
            rt.keyword()
        )));
    }
    let ty = lt;
    indent(out, depth);

    // Comparisons yield i32 (bool); arithmetic/bitwise yield the operand type.
    let (instr, result) = match (op, ty) {
        // ── arithmetic over i64 — checked overflow trap posture ──
        (BinOp::Add, WatTy::I64) => ("i64.add", WatTy::I64),
        (BinOp::Sub, WatTy::I64) => ("i64.sub", WatTy::I64),
        (BinOp::Mul, WatTy::I64) => ("i64.mul", WatTy::I64),
        // FloorDiv / Mod need the floor correction — handled below.
        (BinOp::FloorDiv, WatTy::I64) => {
            writeln!(out, "call $__wasm_floordiv_i64").expect("write");
            return Ok(WatTy::I64);
        }
        (BinOp::Mod, WatTy::I64) => {
            writeln!(out, "call $__wasm_floormod_i64").expect("write");
            return Ok(WatTy::I64);
        }
        // ── bitwise / shift over i64 ──
        (BinOp::BitAnd, WatTy::I64) => ("i64.and", WatTy::I64),
        (BinOp::BitOr, WatTy::I64) => ("i64.or", WatTy::I64),
        (BinOp::BitXor, WatTy::I64) => ("i64.xor", WatTy::I64),
        (BinOp::Shl, WatTy::I64) => ("i64.shl", WatTy::I64),
        (BinOp::Shr, WatTy::I64) => ("i64.shr_s", WatTy::I64),
        // ── comparisons over i64 → i32 bool ──
        (BinOp::Eq, WatTy::I64) => ("i64.eq", WatTy::I32),
        (BinOp::NotEq, WatTy::I64) => ("i64.ne", WatTy::I32),
        (BinOp::Lt, WatTy::I64) => ("i64.lt_s", WatTy::I32),
        (BinOp::LtEq, WatTy::I64) => ("i64.le_s", WatTy::I32),
        (BinOp::Gt, WatTy::I64) => ("i64.gt_s", WatTy::I32),
        (BinOp::GtEq, WatTy::I64) => ("i64.ge_s", WatTy::I32),
        // ── comparisons over f64 → i32 bool ──
        (BinOp::Eq, WatTy::F64) => ("f64.eq", WatTy::I32),
        (BinOp::NotEq, WatTy::F64) => ("f64.ne", WatTy::I32),
        (BinOp::Lt, WatTy::F64) => ("f64.lt", WatTy::I32),
        (BinOp::LtEq, WatTy::F64) => ("f64.le", WatTy::I32),
        (BinOp::Gt, WatTy::F64) => ("f64.gt", WatTy::I32),
        (BinOp::GtEq, WatTy::F64) => ("f64.ge", WatTy::I32),
        // ── comparisons over i32 bool ──
        (BinOp::Eq, WatTy::I32) => ("i32.eq", WatTy::I32),
        (BinOp::NotEq, WatTy::I32) => ("i32.ne", WatTy::I32),
        (op, ty) => {
            return Err(unsupported(&format!(
                "binary op {op:?} over WASM {} (not in the scalar/control subset)",
                ty.keyword()
            )));
        }
    };
    writeln!(out, "{instr}").expect("write");
    Ok(result)
}

/// Short-circuit `and`/`or` over i32 booleans, emitted as a typed
/// if-expression (matching Python/Rust short-circuit semantics).
fn emit_logical(
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    // `a and b` => if a then b else 0 ;  `a or b` => if a then 1 else b
    emit_expr_typed(lhs, scope, out, depth, WatTy::I32)?;
    indent(out, depth);
    writeln!(out, "if (result i32)").expect("write");
    match op {
        BinOp::And => {
            emit_expr_typed(rhs, scope, out, depth + 1, WatTy::I32)?;
            indent(out, depth);
            writeln!(out, "else").expect("write");
            indent(out, depth + 1);
            writeln!(out, "i32.const 0").expect("write");
        }
        BinOp::Or => {
            indent(out, depth + 1);
            writeln!(out, "i32.const 1").expect("write");
            indent(out, depth);
            writeln!(out, "else").expect("write");
            emit_expr_typed(rhs, scope, out, depth + 1, WatTy::I32)?;
        }
        _ => unreachable!("emit_logical only handles And/Or"),
    }
    indent(out, depth);
    writeln!(out, "end").expect("write");
    Ok(WatTy::I32)
}

fn emit_float_binop(
    op: FloatOp,
    lhs: &Expr,
    rhs: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let lt = emit_expr(lhs, scope, out, depth)?;
    let rt = emit_expr(rhs, scope, out, depth)?;
    if lt != WatTy::F64 || rt != WatTy::F64 {
        return Err(unsupported(&format!(
            "float op {op:?} requires f64 operands (got {} and {})",
            lt.keyword(),
            rt.keyword()
        )));
    }
    indent(out, depth);
    let instr = match op {
        FloatOp::Add => "f64.add",
        FloatOp::Sub => "f64.sub",
        FloatOp::Mul => "f64.mul",
        FloatOp::Div => "f64.div",
        other => {
            return Err(unsupported(&format!(
                "float op {other:?} (only + - * / are in the WASM scalar subset; \
                 floordiv/mod/pow/hypot/atan2/log are refused)"
            )));
        }
    };
    writeln!(out, "{instr}").expect("write");
    Ok(WatTy::F64)
}

/// Short kind label for an unsupported expression, for the refusal message.
fn expr_kind(e: &Expr) -> &'static str {
    match e {
        Expr::Concat { .. } => "Concat (string)",
        Expr::LitStr(_) => "LitStr",
        Expr::ListLit(_) => "ListLit",
        Expr::DictLit(_) => "DictLit",
        Expr::SetLit(_) => "SetLit",
        Expr::TupleLit(_) => "TupleLit",
        Expr::Len(_) => "Len",
        Expr::Index { .. } => "Index",
        Expr::StructLit { .. } => "StructLit",
        Expr::Block(_) => "Block",
        _ => "<container/aggregate/builtin expression>",
    }
}

#[cfg(test)]
mod tests;
