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
//!   header offset the list layout uses). As of **PMAT-1032** every
//!   Python-VISIBLE string read is **CHAR-oriented (code points)** over that
//!   byte-oriented ABI — sweep #11 (PMAT-1031 finding 2) confirmed the old
//!   byte-oriented reads SILENTLY diverged on non-ASCII input: (1)
//!   **`len(s)`** lowers to `$__wasm_str_charlen` (a non-continuation-byte
//!   count — `len("héllo")` is 5, matching CPython, where the header holds
//!   6 bytes); and (2) **`ord(s[i])`** (`Expr::Ord` over an `Expr::StrCharAt`
//!   of a str name) lowers to `$__wasm_str_ord_at` — a CHAR-indexed walk +
//!   1..4-byte UTF-8 decode, with Python NEGATIVE-index normalisation
//!   (`s[-1]` reads from the end) and the bounds trap (`unreachable`, the
//!   `IndexError` analogue) inside the helper. `ord(ch)` over a 1-char str
//!   name guards `charlen != 1 → unreachable` (the `TypeError` analogue) —
//!   the CHAR count, so `ord("é")` decodes to 233 exactly.
//!   As of **PMAT-993 (slice 2)** string-RETURNING ops are unblocked by a
//!   linear-memory **bump allocator** (`(global $__heap_ptr (mut i32))` past
//!   the static `(data)` region plus `$__alloc(n)`, 8-byte-aligned, bump-only
//!   with no free; see `HEAP_BASE`/`heap_helpers`). The slice-2 string ops:
//!   **string concatenation `a + b`** (`Expr::Concat`) does `alloc(8 + Σ
//!   len(opᵢ))`, writes the count header, `memory.copy`s each operand's bytes,
//!   and returns the new base-pointer (left-nested `(a+b)+c` flattens to ONE
//!   alloc with N copies) — concat/copy/equality stay BYTE ops (byte equality
//!   IS char equality for UTF-8); and **`chr(n)`** (`Expr::Chr`) lowers to
//!   `$__wasm_chr` — the full 1..4-byte UTF-8 encoding of code point `n`,
//!   trapping outside `0..=0x10FFFF` (the `ValueError` analogue; the
//!   pre-PMAT-1032 lowering masked `n & 0xFF`, silently wrong for n > 127).
//!   A function RETURNING a `str` works (the result is the new string's
//!   `i32` heap pointer). As of **PMAT-994 (slice 3a)** string support is
//!   substantially complete: (1) string **LITERALS** (`Expr::LitStr`,
//!   e.g. `"Hello, "`) are materialised at emit time into static `(data …)`
//!   segments in `[LITERAL_BASE, HEAP_BASE)` (length-prefixed, the same ABI),
//!   and a `LitStr` lowers to a constant `i32.const <base>` — so `"Hi " +
//!   name`, `return "done"`, and literal args all work; (2) **`s[i]` as a
//!   1-char string** (`Expr::StrCharAt` outside `ord`) materialises a new
//!   1-char heap string via `$__wasm_str_char_at` (the full encoded char,
//!   1..4 bytes, char-indexed + negative-index-normalised since PMAT-1032);
//!   and (3) string **content equality** `a == b` / `a != b` lowers to a
//!   `$__wasm_str_eq` helper (length check + byte-compare loop → i32 bool) —
//!   REAL content logic, never a base-pointer compare. Since PMAT-1136
//!   `s.find(p)` is supported (the CODE-POINT index of the first occurrence of
//!   `p` in `s`, or -1 → i64) — a non-allocating byte search converting the
//!   match's byte offset to a char index (Python find is char-indexed). Since
//!   PMAT-1058/1059
//!   string **slicing** `s[lo:hi]` and **ordering** (`<`/`<=`/`>`/`>=`) are
//!   supported. As of **PMAT-1142** the sequence repeat **`s * n`**
//!   (`Expr::Repeat`, of_str) MATERIALISES a new heap string via
//!   `$__wasm_str_repeat` — a byte replication (`max(n, 0)` copies), char-exact
//!   for UTF-8 (no code-point transform, so it IS Python `str * int` for any
//!   string); a LIST repeat (`[…] * n`) is refused. And since PMAT-1136
//!   `s.find(p)` returns a CODE-POINT index (i64, or -1). Since PMAT-1060
//!   **`str(int)` / `repr(int)`**
//!   (`Expr::ToStr { of_float: false }`) materialises an i64's decimal-ASCII
//!   form via `$__wasm_int_to_str` (unsigned-magnitude, so `i64::MIN` is
//!   exact). Since PMAT-1126/1127/1128 the string **methods**
//!   `s.startswith(p)` / `s.endswith(p)` (byte prefix/suffix → i32 bool),
//!   `s.count(p)` (non-overlapping byte occurrence count → i64), the substring
//!   test `p in s` (`Expr::StrContains`, a sliding byte search → i32 bool), and
//!   (PMAT-1136/1143) `s.find(p)` / `s.rfind(p)` (the CODE-POINT index of the
//!   first / last occurrence → i64, or -1), their (PMAT-1163/1165) START-BOUNDED
//!   forms `s.find(p, start)` / `s.rfind(p, start)` (Python's negative/overflow
//!   start clamp + char-decoded start, still ABSOLUTE code-point index), and
//!   (PMAT-1144) `s.index(p)` / `s.rindex(p)` (the TRAPPING siblings — same
//!   CODE-POINT index, but an ABSENT needle is Python `ValueError`, lowered to a
//!   WASM `unreachable` trap) are supported — each a non-allocating helper. Still
//!   **refused** honestly (a hard `BackendError`): `str(float)`/`repr(float)`,
//!   the OTHER string methods (upper/lower/strip/split/…), the 3-arg
//!   `s.find`/`s.rfind`(p, start, end) and the start/end forms of
//!   `index`/`rindex`/`count`, and the composite `dict` / `set` value/`in` shapes
//!   not yet wired. Char access is O(chars)
//!   per read (charlen is O(bytes)) — correctness over speed, an honest
//!   documented tradeoff.
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
//!   in a natural-width `*.store`. As of **PMAT-1033** a `list[scalar]`
//!   **LOCAL** (`xs: list[int] = [1, 2, 3]`) binds too: the `ListLit`
//!   materialises a fresh length-prefixed record on the bump heap (the same
//!   ABI a param rides, so `xs[i]`/`xs[i] = v`/`len(xs)` and the PMAT-1030
//!   `for x in xs` desugar work over it verbatim), and a list-name binding
//!   (`ys = xs`) is a bare pointer copy — Python's aliasing, native to
//!   linear memory. A list **return**, list **append** / growth (fixed-size
//!   records; growth would relocate and break aliases — the PMAT-999
//!   posture), list-returning calls, and a MULTI-index / nested-list write
//!   (`xs[i][j] = v`) remain refused.
//! - Statements: `Let`/`Assign` (→ `local` + `local.set`), `If`/`While`/
//!   `Break`/`Continue`/`Return`, `xs[i] = v` (`IndexAssign`) over a
//!   `list[scalar]` param (bounds-checked `*.store`), and as of **PMAT-1023**
//!   `obj.field = v` (`FieldAssign` — a `*.store` at the field's 8-byte-slot
//!   offset) plus statement-position method calls (`SideEffectCall` over an
//!   `Expr::MethodCall`, dropping a discarded result) and, as of PMAT-1024,
//!   statement-position PLAIN function calls (`SideEffectCall` over an
//!   `Expr::Call` — the `bump(c)` mutating-helper idiom the reference-aware
//!   frontend passes through as a bare heap pointer).
//! - Expressions: `Ident` (→ `local.get`), `LitInt`/`LitFloat`/`LitBool`,
//!   `BinOp` (arith/bitwise/shift + comparisons), `FloatBinOp`, `UnOp`,
//!   `IfExpr`, `Index` over a `list[scalar]` param (bounds-checked `*.load`),
//!   `Len` over a `list[scalar]` param (→ header `i32.load` + `i64` extend),
//!   a direct intra-module `Call`, and `obj.method(args)` (`MethodCall`)
//!   over a struct local/param receiver.
//! - **Struct METHODS (PMAT-1023):** each `Item::Struct` method — including
//!   SELF-MUTATING ones (`self.count = self.count + 1`) — emits as an
//!   ordinary WAT function `$<Struct>.<method>` whose `self` receiver is the
//!   instance's `i32` base-pointer. A field write through `self` (or any
//!   struct local/param) is a store through that pointer, so the mutation is
//!   visible to EVERY binding of the record: Python's reference semantics
//!   are NATIVE to linear memory — no clone/refuse alias disposition needed
//!   (the Rust lane must refuse shapes this lane executes exactly). Struct
//!   `==`/ordering is REFUSED honestly (a struct rides an i32 base-pointer;
//!   a naive compare would be pointer identity, not Python's structural
//!   `==`), as are non-`self` receivers and unknown methods.
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

use std::collections::HashMap;
use xpile_backend::{
    Artifact, Backend, BackendConfig, BackendError, EmittedText, MultiEmitterBackend, QuorumPolicy,
    QuorumStatus, Target, TargetEmitter,
};
use xpile_contracts::ContractId;
use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, Stmt, StrMethodOp, Type, UnOp,
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

/// PMAT-1002: an `f64` scratch holding a float DIVISOR while it is checked
/// against `0.0`. Python raises `ZeroDivisionError` on `x / 0.0`; a bare
/// `f64.div` returns IEEE inf/nan instead. This local stashes the divisor so the
/// dividend can stay on the stack while the guard traps on a zero divisor.
const FDIV_SCRATCH: &str = "__wasm_fdiv_d";

/// PMAT-995 (slice 3b): per-function scratch `i32` local holding a freshly
/// `$__alloc`-ed dict/set base-pointer while [`emit_dict_lit`] writes its
/// header + entries. Body-driven declaration, like the string scratches.
const DICT_DST_SCRATCH: &str = "__wasm_dict_dst";

/// PMAT-996 (slice 4): the `$__alloc`-ed struct base-pointer while
/// [`emit_struct_lit`] writes its fields. Mirrors [`DICT_DST_SCRATCH`].
const STRUCT_DST_SCRATCH: &str = "__wasm_struct_dst";

/// PMAT-1033: the `$__alloc`-ed list base-pointer while [`emit_list_lit`]
/// writes its header + elements. Mirrors [`STRUCT_DST_SCRATCH`]. No
/// self-clobber hazard: a `ListLit` is only accepted binding a list-annotated
/// local (never nested inside an element expression — nested lists refuse at
/// element-type validation, and list-valued calls/args stay refused).
const LIST_DST_SCRATCH: &str = "__wasm_list_dst";

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
    for f in module_functions(module) {
        collect_block_literals(&f.body, &mut contents);
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
        // PMAT-1151: the INDEX of `xs[i] = v` (not just the value) can carry a
        // str literal (`xs["ab".find("b")] = v`, `xs[len("hi")] = v`) — lay the
        // indices out too, matching the widened `stmt_has_*` scans.
        Stmt::IndexAssign { indices, value, .. } => {
            for i in indices {
                collect_expr_literals(i, out);
            }
            collect_expr_literals(value, out);
        }
        // PMAT-995: `d[k] = v` — a str KEY literal must be laid out too.
        Stmt::DictSet { key, value, .. } => {
            collect_expr_literals(key, out);
            collect_expr_literals(value, out);
        }
        // PMAT-1151: `s.add(e)` (`Stmt::SetAdd`) — its ELEM can carry a str
        // literal (`q.add("xabx"[1:3])`) that must be laid out into a (data)
        // segment; the collector previously skipped SetAdd entirely, so the
        // literal was "not laid out — internal layout error". The WRITE-side
        // sibling of the DictSet layout arm above.
        Stmt::SetAdd { elem, .. } => collect_expr_literals(elem, out),
        // PMAT-1023: a field write's VALUE and a statement-position method
        // call's ARGS may reference literals (`c.tag(ord("x"))`).
        Stmt::FieldAssign { value, .. } => collect_expr_literals(value, out),
        Stmt::SideEffectCall { call } => collect_expr_literals(call, out),
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
        // PMAT-1058: a slice base / bounds may reference literals
        // (`"hello"[1:4]`, `s[0:n]`).
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            collect_expr_literals(collection, out);
            if let Some(b) = lo {
                collect_expr_literals(b, out);
            }
            if let Some(b) = hi {
                collect_expr_literals(b, out);
            }
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
        // PMAT-1023: method-call args + struct-literal field values may carry
        // literals; the receiver/obj is a bare name (nothing to collect) but
        // recursing is harmless and future-proof.
        Expr::MethodCall { obj, args, .. } => {
            collect_expr_literals(obj, out);
            for a in args {
                collect_expr_literals(a, out);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_expr_literals(v, out);
            }
        }
        // PMAT-1127: `x in s` — both string operands may be LITERALS (`"lo" in
        // "hello"`), so lay their `(data)` segments out (else `emit_str_expr`
        // fails to find a literal address).
        Expr::StrContains { haystack, needle } => {
            collect_expr_literals(haystack, out);
            collect_expr_literals(needle, out);
        }
        // PMAT-1128: a string METHOD's receiver AND args may be LITERALS
        // (`s.count("l")`, `"banana".count(p)`) — both lower via `emit_str_expr`,
        // which needs each literal laid out as a `(data)` segment. (Before this
        // arm, a literal method arg fell through to `_ => {}` and `emit_str_expr`
        // found no address — the same latent gap the startswith/endswith
        // witnesses missed by only ever passing str PARAMS.)
        Expr::StrMethod { recv, args, .. } => {
            collect_expr_literals(recv, out);
            for a in args {
                collect_expr_literals(a, out);
            }
        }
        // PMAT-1142: a string repeat `s * n` — the repeated `seq` may be a
        // LITERAL (`"ab" * 3`), so lay out its `(data)` segment (else
        // `emit_str_expr` finds no address for the source string). The count `n`
        // is an int expr (no str literal), recursed for completeness.
        Expr::Repeat { seq, n, .. } => {
            collect_expr_literals(seq, out);
            collect_expr_literals(n, out);
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

/// PMAT-1059: `$__wasm_str_cmp(a, b)` — Python-style lexicographic 3-way
/// compare of two length-prefixed UTF-8 strings, backing the ORDERING ops
/// (`<` / `<=` / `>` / `>=`). Returns i32 `<0` if `a < b`, `0` if `a == b`,
/// `>0` if `a > b`.
///
/// The compare is a byte-wise UNSIGNED lexicographic compare over
/// `min(len(a), len(b))` bytes, then shorter-is-less on a common prefix
/// (`len(a) - len(b)`). This IS Python's string ordering: UTF-8 is designed so
/// byte-lexicographic order EQUALS code-point-lexicographic order (a fundamental
/// property — the lead byte of a higher code point is numerically larger, and
/// the char boundaries of a shared prefix align, so the first differing byte
/// lands at the same intra-char offset in both). So a byte compare over the
/// UTF-8 payload reproduces CPython's code-point compare EXACTLY — no char walk
/// needed (unlike len / index / slice, which must count code points). Bytes are
/// read with `i32.load8_u` (0..255), so `a[i] - b[i]` carries the correct sign.
/// Emitted once per module (gated on [`module_needs_str_cmp`]).
const STR_CMP_HELPER: &str = "\
  ;; __wasm_str_cmp(a, b) = Python lexicographic 3-way compare (str < / <= / > / >=)
  ;; a, b are i32 base-pointers to length-prefixed regions (i32 byte count @
  ;; base+0, UTF-8 bytes @ base+8). Returns i32 <0 / 0 / >0. Byte-wise UNSIGNED
  ;; compare over min(len) bytes then shorter-is-less — UTF-8 byte order ==
  ;; code-point order, so this IS Python str ordering.
  (func $__wasm_str_cmp (param $a i32) (param $b i32) (result i32)
    (local $na i32)
    (local $nb i32)
    (local $n i32)
    (local $i i32)
    (local $ba i32)
    (local $bb i32)
    ;; na = len(a); nb = len(b)
    local.get $a
    i32.load
    local.set $na
    local.get $b
    i32.load
    local.set $nb
    ;; n = min(na, nb)
    local.get $na
    local.get $nb
    i32.lt_s
    if (result i32)
      local.get $na
    else
      local.get $nb
    end
    local.set $n
    ;; i = 0; while i < n: compare byte i (unsigned)
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        local.get $i
        local.get $n
        i32.ge_s
        br_if $done
        ;; ba = a[8+i]
        local.get $a
        i32.const 8
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        local.set $ba
        ;; bb = b[8+i]
        local.get $b
        i32.const 8
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        local.set $bb
        ;; if ba != bb return ba - bb (unsigned bytes → correct sign)
        local.get $ba
        local.get $bb
        i32.ne
        if
          local.get $ba
          local.get $bb
          i32.sub
          return
        end
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    ;; common prefix equal → shorter is less: na - nb
    local.get $na
    local.get $nb
    i32.sub
  )
";

/// PMAT-1126: `$__wasm_str_startswith(s, p)` — Python `s.startswith(p)` over two
/// length-prefixed UTF-8 strings, returning an `i32` boolean (1 = `s` starts
/// with `p`, 0 = not).
///
/// A pure BYTE-PREFIX compare: if `len(p) > len(s)` return 0; else compare the
/// first `len(p)` bytes of `s` (from `s+8`) against all of `p` (from `p+8`),
/// returning 0 on the first mismatch and 1 if all match. A byte-prefix compare
/// IS a CODE-POINT-prefix compare for valid UTF-8: `p` is a valid UTF-8 string,
/// so `p[0]` is a LEAD byte (never a `0x80..0xBF` continuation), which means a
/// byte match forces the compare to start on a char boundary in `s` — so
/// matching `len(p)` bytes matches exactly `p`'s code points (no split char, no
/// false positive on a shared continuation byte). This mirrors the `$__wasm_str_cmp`
/// rationale (byte order == code-point order) and, like it, allocates NOTHING —
/// it reads linear memory and returns a bool. The empty prefix yields 1
/// (`"abc".startswith("")` is True) since the loop body never runs. Emitted once
/// per module (gated on [`module_uses_str_method`] for `StartsWith`).
const STR_STARTSWITH_HELPER: &str = "\
  ;; __wasm_str_startswith(s, p) = Python s.startswith(p)  (i32 bool)
  ;; s, p are i32 base-pointers to length-prefixed regions (i32 byte count @
  ;; base+0, UTF-8 bytes @ base+8). Byte-prefix compare == code-point-prefix
  ;; compare for valid UTF-8 (p[0] is a lead byte → byte match lands on a char
  ;; boundary), so this IS Python startswith. Allocates nothing.
  (func $__wasm_str_startswith (param $s i32) (param $p i32) (result i32)
    (local $sn i32)
    (local $pn i32)
    (local $i i32)
    ;; sn = len(s); pn = len(p)
    local.get $s
    i32.load
    local.set $sn
    local.get $p
    i32.load
    local.set $pn
    ;; if pn > sn return 0 (a longer prefix can never match)
    local.get $pn
    local.get $sn
    i32.gt_s
    if
      i32.const 0
      return
    end
    ;; i = 0; while i < pn: if s[8+i] != p[8+i] return 0; i += 1
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        local.get $i
        local.get $pn
        i32.ge_s
        br_if $done
        ;; s byte i
        local.get $s
        i32.const 8
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        ;; p byte i
        local.get $p
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

/// PMAT-1126: `$__wasm_str_endswith(s, p)` — Python `s.endswith(p)`, returning
/// an `i32` boolean. The suffix mirror of [`STR_STARTSWITH_HELPER`]: if
/// `len(p) > len(s)` return 0; else compare `p` against the LAST `len(p)` bytes
/// of `s` — from `s+8 + (len(s) - len(p))`. The `len(s) - len(p)` start offset
/// is `>= 0` (guarded by the length check) and lands on a char boundary in `s`
/// for the same reason startswith does (`p[0]` is a lead byte), so a byte-suffix
/// match IS a code-point-suffix match for valid UTF-8. Allocates nothing.
/// Emitted once per module (gated on [`module_uses_str_method`] for `EndsWith`).
const STR_ENDSWITH_HELPER: &str = "\
  ;; __wasm_str_endswith(s, p) = Python s.endswith(p)  (i32 bool)
  ;; s, p are i32 base-pointers to length-prefixed regions (i32 byte count @
  ;; base+0, UTF-8 bytes @ base+8). Compares p against the LAST len(p) bytes of
  ;; s (offset len(s)-len(p)); byte-suffix == code-point-suffix for valid UTF-8.
  ;; Allocates nothing.
  (func $__wasm_str_endswith (param $s i32) (param $p i32) (result i32)
    (local $sn i32)
    (local $pn i32)
    (local $off i32)
    (local $i i32)
    ;; sn = len(s); pn = len(p)
    local.get $s
    i32.load
    local.set $sn
    local.get $p
    i32.load
    local.set $pn
    ;; if pn > sn return 0
    local.get $pn
    local.get $sn
    i32.gt_s
    if
      i32.const 0
      return
    end
    ;; off = sn - pn  (>= 0, guarded above) — the byte where the suffix begins
    local.get $sn
    local.get $pn
    i32.sub
    local.set $off
    ;; i = 0; while i < pn: if s[8+off+i] != p[8+i] return 0; i += 1
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        local.get $i
        local.get $pn
        i32.ge_s
        br_if $done
        ;; s byte off+i
        local.get $s
        i32.const 8
        i32.add
        local.get $off
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        ;; p byte i
        local.get $p
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

/// PMAT-1127: `$__wasm_str_contains(h, n)` — Python `n in h` (substring test)
/// over two length-prefixed UTF-8 strings, returning an `i32` boolean (1 = `n`
/// is a substring of `h`, 0 = not).
///
/// A naive BYTE substring search: slide `n` over `h` at every start offset
/// `0..=(len(h)-len(n))`, comparing `len(n)` bytes at each; the first full match
/// yields 1, exhausting all starts yields 0. A byte-substring match IS a
/// CODE-POINT-substring match for valid UTF-8: `n` is a valid UTF-8 string, so
/// `n[0]` is a LEAD byte (never a `0x80..0xBF` continuation), which means ANY
/// byte match forces the compare to begin on a char boundary in `h` — so a
/// `len(n)`-byte match is exactly an `n`-code-point match (no split char, no
/// false positive from a shared continuation byte straddling a boundary). This
/// mirrors the `$__wasm_str_startswith` rationale (a prefix test is the special
/// case `start == 0`) and, like it, allocates NOTHING — it reads linear memory
/// and returns a bool. The empty needle yields 1 (`"" in "abc"` is True) since
/// the outer/inner loops short-circuit. Emitted once per module (gated on
/// [`module_uses_str_contains`]).
const STR_CONTAINS_HELPER: &str = "\
  ;; __wasm_str_contains(h, n) = Python (n in h)  (i32 bool)
  ;; h, n are i32 base-pointers to length-prefixed regions (i32 byte count @
  ;; base+0, UTF-8 bytes @ base+8). Naive byte substring search: slide n over h
  ;; at each start 0..=(hn-nn). Byte-substring == code-point-substring for valid
  ;; UTF-8 (n[0] is a lead byte → any byte match lands on a char boundary in h).
  ;; Empty needle → 1. Allocates nothing.
  (func $__wasm_str_contains (param $h i32) (param $n i32) (result i32)
    (local $hn i32)
    (local $nn i32)
    (local $start i32)
    (local $last i32)
    (local $j i32)
    (local $match i32)
    ;; hn = len(h); nn = len(n)
    local.get $h
    i32.load
    local.set $hn
    local.get $n
    i32.load
    local.set $nn
    ;; the empty needle is contained in every string (Python `\"\" in s` is True)
    local.get $nn
    i32.eqz
    if
      i32.const 1
      return
    end
    ;; a needle longer than the haystack can never be a substring
    local.get $nn
    local.get $hn
    i32.gt_s
    if
      i32.const 0
      return
    end
    ;; last = hn - nn  (inclusive last start offset; >= 0, guarded above)
    local.get $hn
    local.get $nn
    i32.sub
    local.set $last
    ;; start = 0; while start <= last: try a len(n)-byte match at offset start
    i32.const 0
    local.set $start
    (block $done
      (loop $next_start
        local.get $start
        local.get $last
        i32.gt_s
        br_if $done
        ;; match = 1; j = 0; while j < nn: if h[8+start+j] != n[8+j] fail
        i32.const 1
        local.set $match
        i32.const 0
        local.set $j
        (block $stop
          (loop $next_char
            local.get $j
            local.get $nn
            i32.ge_s
            br_if $stop
            ;; h byte start+j
            local.get $h
            i32.const 8
            i32.add
            local.get $start
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            ;; n byte j
            local.get $n
            i32.const 8
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            i32.ne
            if
              i32.const 0
              local.set $match
              br $stop
            end
            local.get $j
            i32.const 1
            i32.add
            local.set $j
            br $next_char
          )
        )
        ;; a full-length match at this start → contained
        local.get $match
        if
          i32.const 1
          return
        end
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        br $next_start
      )
    )
    i32.const 0
  )
";

/// PMAT-1128: `$__wasm_str_count(h, n) -> i64` — Python `h.count(n)` (the count
/// of NON-OVERLAPPING occurrences of `n` in `h`) over two length-prefixed UTF-8
/// strings, returning an `i64` (a Python `int`).
///
/// The counting generalisation of [`STR_CONTAINS_HELPER`]: same byte slide, but
/// instead of returning at the first match it COUNTS matches and, on each,
/// advances the start cursor by `len(n)` (non-overlapping, exactly like Python's
/// `str.count` and Rust's `str::matches().count()`), returning the total. Two
/// special cases, both pinned to CPython:
///   * an EMPTY needle → `charlen(h) + 1` (Python `"abc".count("")` is 4,
///     `"".count("")` is 1). This is the CODE-POINT count + 1, NOT the byte
///     count + 1 — so it calls `$__wasm_str_charlen` (always emitted for a
///     str-touching module) rather than reading the byte header, keeping the
///     empty-needle answer char-exact for non-ASCII (`"héllo".count("")` is 6,
///     not the byte-derived 7).
///   * a needle LONGER than the haystack → 0.
///
/// For a non-empty needle the count is a pure number of matches, IDENTICAL in
/// byte- or code-point-space (a byte-substring match IS a code-point-substring
/// match for valid UTF-8 — `n[0]` is a LEAD byte, so every match lands on a char
/// boundary in `h`), so the byte slide reproduces CPython EXACTLY. Like
/// `$__wasm_str_contains` it reads linear memory and allocates NOTHING (an int,
/// not a new string). Emitted once per module (gated on [`module_uses_str_method`]
/// for `Count`).
const STR_COUNT_HELPER: &str = "\
  ;; __wasm_str_count(h, n) = Python h.count(n)  (i64: non-overlapping occurrences)
  ;; h, n are i32 base-pointers to length-prefixed regions (i32 byte count @
  ;; base+0, UTF-8 bytes @ base+8). Same byte slide as $__wasm_str_contains, but
  ;; counts matches, advancing start by len(n) on each (non-overlapping). Empty
  ;; needle → charlen(h)+1 (CODE points, char-exact). Byte-count == code-point
  ;; count for a non-empty needle (n[0] is a lead byte). Allocates nothing.
  (func $__wasm_str_count (param $h i32) (param $n i32) (result i64)
    (local $hn i32)
    (local $nn i32)
    (local $start i32)
    (local $last i32)
    (local $j i32)
    (local $match i32)
    (local $count i32)
    ;; hn = len(h); nn = len(n)
    local.get $h
    i32.load
    local.set $hn
    local.get $n
    i32.load
    local.set $nn
    ;; empty needle: Python h.count(\"\") == charlen(h) + 1 (CODE points, not bytes)
    local.get $nn
    i32.eqz
    if
      local.get $h
      call $__wasm_str_charlen
      i32.const 1
      i32.add
      i64.extend_i32_u
      return
    end
    ;; a needle longer than the haystack can never occur
    local.get $nn
    local.get $hn
    i32.gt_s
    if
      i64.const 0
      return
    end
    ;; last = hn - nn  (inclusive last start offset; >= 0, guarded above)
    local.get $hn
    local.get $nn
    i32.sub
    local.set $last
    ;; count = 0; start = 0; while start <= last: match at start → count++, start += nn; else start++
    i32.const 0
    local.set $count
    i32.const 0
    local.set $start
    (block $done
      (loop $next_start
        local.get $start
        local.get $last
        i32.gt_s
        br_if $done
        ;; match = 1; j = 0; while j < nn: if h[8+start+j] != n[8+j] fail
        i32.const 1
        local.set $match
        i32.const 0
        local.set $j
        (block $stop
          (loop $next_char
            local.get $j
            local.get $nn
            i32.ge_s
            br_if $stop
            ;; h byte start+j
            local.get $h
            i32.const 8
            i32.add
            local.get $start
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            ;; n byte j
            local.get $n
            i32.const 8
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            i32.ne
            if
              i32.const 0
              local.set $match
              br $stop
            end
            local.get $j
            i32.const 1
            i32.add
            local.set $j
            br $next_char
          )
        )
        ;; a full-length match → count++, advance start by nn (non-overlapping)
        local.get $match
        if
          local.get $count
          i32.const 1
          i32.add
          local.set $count
          local.get $start
          local.get $nn
          i32.add
          local.set $start
        else
          local.get $start
          i32.const 1
          i32.add
          local.set $start
        end
        br $next_start
      )
    )
    local.get $count
    i64.extend_i32_u
  )
";

/// PMAT-1136: `$__wasm_str_find(h, n) -> i64` — Python `h.find(n)` (the CODE-POINT
/// index of the FIRST occurrence of `n` in `h`, or `-1` if absent) over two
/// length-prefixed UTF-8 strings, returning an `i64` (a Python `int`).
///
/// The index-returning sibling of [`STR_CONTAINS_HELPER`]: the SAME naive byte
/// slide, but instead of a bool it returns WHERE the first match starts — and,
/// crucially, as a **code-point** index (Python `str.find` returns the position
/// in code points, NOT bytes). So on a match at byte offset `start` it converts
/// `start` to a code-point index by counting the non-continuation bytes in
/// `h[0..start]` (`(b & 0xC0) != 0x80`) — the ONE place `find` must diverge from
/// the byte-oriented `contains`/`count`, whose answers (a bool / a match count)
/// are byte/code-point identical. The conversion is exact because `n[0]` is a
/// LEAD byte, so every match lands on a char boundary in `h`; the prefix
/// `h[0..start]` is therefore a whole number of code points.
///
///   * an EMPTY needle → `0` (Python `"abc".find("")` and `"".find("")` are both
///     `0` — the empty string is found at the start). No char walk needed.
///   * a needle LONGER than the haystack → `-1`.
///   * absent → `-1`.
///
/// For non-ASCII input this is char-exact where a byte index would silently
/// diverge (`"héllo".find("llo")` == 2, not the byte offset 4). Like
/// `$__wasm_str_contains` it reads linear memory and allocates NOTHING (an int,
/// not a new string). Emitted once per module (gated on [`module_uses_str_method`]
/// for `Find`).
const STR_FIND_HELPER: &str = "\
  ;; __wasm_str_find(h, n) = Python h.find(n)  (i64: CODE-POINT index of first
  ;; occurrence, or -1). h, n are i32 base-pointers to length-prefixed regions
  ;; (i32 byte count @ base+0, UTF-8 bytes @ base+8). Same byte slide as
  ;; $__wasm_str_contains, but returns the match position converted to a CODE
  ;; POINT index (count non-continuation bytes in h[0..start]) — Python find is
  ;; char-indexed, not byte-indexed. Empty needle → 0. Allocates nothing.
  (func $__wasm_str_find (param $h i32) (param $n i32) (result i64)
    (local $hn i32)
    (local $nn i32)
    (local $start i32)
    (local $last i32)
    (local $j i32)
    (local $match i32)
    (local $ci i32)
    (local $p i32)
    ;; hn = len(h); nn = len(n)
    local.get $h
    i32.load
    local.set $hn
    local.get $n
    i32.load
    local.set $nn
    ;; empty needle: Python h.find(\"\") == 0 (found at the start)
    local.get $nn
    i32.eqz
    if
      i64.const 0
      return
    end
    ;; a needle longer than the haystack can never occur → -1
    local.get $nn
    local.get $hn
    i32.gt_s
    if
      i64.const -1
      return
    end
    ;; last = hn - nn  (inclusive last start offset; >= 0, guarded above)
    local.get $hn
    local.get $nn
    i32.sub
    local.set $last
    ;; start = 0; while start <= last: try a len(n)-byte match at offset start
    i32.const 0
    local.set $start
    (block $done
      (loop $next_start
        local.get $start
        local.get $last
        i32.gt_s
        br_if $done
        ;; match = 1; j = 0; while j < nn: if h[8+start+j] != n[8+j] fail
        i32.const 1
        local.set $match
        i32.const 0
        local.set $j
        (block $stop
          (loop $next_char
            local.get $j
            local.get $nn
            i32.ge_s
            br_if $stop
            ;; h byte start+j
            local.get $h
            i32.const 8
            i32.add
            local.get $start
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            ;; n byte j
            local.get $n
            i32.const 8
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            i32.ne
            if
              i32.const 0
              local.set $match
              br $stop
            end
            local.get $j
            i32.const 1
            i32.add
            local.set $j
            br $next_char
          )
        )
        ;; a full-length match at byte offset $start → convert to a CODE-POINT
        ;; index: count non-continuation bytes in h[0..start], return it.
        local.get $match
        if
          i32.const 0
          local.set $ci
          i32.const 0
          local.set $p
          (block $cdone
            (loop $cnext
              local.get $p
              local.get $start
              i32.ge_s
              br_if $cdone
              ;; if (h[8+p] & 0xC0) != 0x80  → ci++ (a lead / single byte)
              local.get $h
              i32.const 8
              i32.add
              local.get $p
              i32.add
              i32.load8_u
              i32.const 0xC0
              i32.and
              i32.const 0x80
              i32.ne
              if
                local.get $ci
                i32.const 1
                i32.add
                local.set $ci
              end
              local.get $p
              i32.const 1
              i32.add
              local.set $p
              br $cnext
            )
          )
          local.get $ci
          i64.extend_i32_s
          return
        end
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        br $next_start
      )
    )
    ;; no match → -1
    i64.const -1
  )
";

/// PMAT-1163: `$__wasm_str_find_from(h, n, startc) -> i64` — Python
/// `h.find(n, start)` (the CODE-POINT index of the first occurrence of `n` in `h`
/// AT OR AFTER code-point index `start`, or `-1` if absent) over two
/// length-prefixed UTF-8 strings, with a Python `int` (i64) start.
///
/// The start-bounded generalisation of [`STR_FIND_HELPER`]: the SAME naive byte
/// slide and the SAME byte-offset → code-point-index conversion, but the slide
/// begins at the byte offset of the `start`-th code point instead of `0`, and the
/// returned index is the ABSOLUTE code-point position in `h` (Python's `find` with
/// a start still reports the position in the ORIGINAL string, not relative to the
/// start). The full Python start semantics are honoured:
///
///   * `start` is a CODE-POINT index (`$__wasm_str_charlen` gives the length).
///   * a NEGATIVE start counts from the end: `start += charlen`, then clamped up
///     to `0` (`"abcabc".find("bc", -3)` == 4; `"abc".find("a", -100)` == 0).
///   * `start > charlen` → `-1` — including the empty needle
///     (`"abc".find("", 4)` == -1), which is why the `> charlen` guard precedes
///     the empty-needle branch.
///   * an EMPTY needle → the clamped `start` (`"abc".find("", 2)` == 2,
///     `"abc".find("", 3)` == 3) — the empty string is found AT `start`.
///   * a match on or after `start` → its ABSOLUTE code-point index; none → `-1`.
///
/// The byte slide is correct at byte granularity for the same reason
/// `$__wasm_str_find` is: `n[0]` is a LEAD byte, so a candidate offset that falls
/// mid-code-point (a continuation byte) can never match. Reads linear memory and
/// allocates NOTHING (a Python int, not a new string). Its `$__wasm_str_charlen`
/// call rides [`module_touches_str`], which any `StrMethod` sets; emitted once per
/// module (gated on [`module_uses_str_find2`], so a plain 1-arg `.find(p)` module
/// carries no dead helper).
const STR_FIND_FROM_HELPER: &str = "\
  ;; __wasm_str_find_from(h, n, startc) = Python h.find(n, start)  (i64: ABSOLUTE
  ;; CODE-POINT index of the first occurrence of n in h at or after code-point
  ;; index start, or -1). h, n are i32 base-pointers (i32 byte count @ base+0,
  ;; UTF-8 bytes @ base+8); startc is the (possibly negative) Python start as i64.
  ;; Same byte slide + byte->code-point conversion as $__wasm_str_find, begun at
  ;; the byte offset of the start-th code point. Allocates nothing.
  (func $__wasm_str_find_from (param $h i32) (param $n i32) (param $startc i64) (result i64)
    (local $hn i32)      ;; len(h) in BYTES
    (local $nn i32)      ;; len(n) in BYTES
    (local $hchars i32)  ;; charlen(h) (code-point count)
    (local $s i32)       ;; clamped start as a code-point index (0..=hchars)
    (local $sb i32)      ;; byte offset of the start-th code point
    (local $cp i32)      ;; code-point counter while converting start->byte
    (local $off i32)     ;; candidate byte offset in the slide
    (local $last i32)    ;; inclusive last candidate byte offset
    (local $j i32)
    (local $match i32)
    (local $ci i32)
    (local $p i32)
    ;; hn = len(h) bytes; nn = len(n) bytes; hchars = charlen(h)
    local.get $h
    i32.load
    local.set $hn
    local.get $n
    i32.load
    local.set $nn
    local.get $h
    call $__wasm_str_charlen
    local.set $hchars
    ;; clamp start: if startc < 0 -> startc += hchars; if still < 0 -> 0
    local.get $startc
    i64.const 0
    i64.lt_s
    if
      local.get $startc
      local.get $hchars
      i64.extend_i32_s
      i64.add
      local.set $startc
      local.get $startc
      i64.const 0
      i64.lt_s
      if
        i64.const 0
        local.set $startc
      end
    end
    ;; if startc > hchars -> -1 (covers the empty needle: h.find(\"\", len+1) == -1)
    local.get $startc
    local.get $hchars
    i64.extend_i32_s
    i64.gt_s
    if
      i64.const -1
      return
    end
    ;; 0 <= startc <= hchars now; narrow to i32 (charlen fits i32)
    local.get $startc
    i32.wrap_i64
    local.set $s
    ;; empty needle: Python h.find(\"\", start) == start (clamped)
    local.get $nn
    i32.eqz
    if
      local.get $s
      i64.extend_i32_s
      return
    end
    ;; convert the start code-point index -> byte offset sb: walk s code points
    i32.const 0
    local.set $sb
    i32.const 0
    local.set $cp
    (block $sbdone
      (loop $sbnext
        local.get $cp
        local.get $s
        i32.ge_s
        br_if $sbdone
        ;; step past the lead byte
        local.get $sb
        i32.const 1
        i32.add
        local.set $sb
        ;; step past continuation bytes: while sb<hn && (h[8+sb]&0xC0)==0x80: sb++
        (block $contdone
          (loop $contnext
            local.get $sb
            local.get $hn
            i32.ge_s
            br_if $contdone
            local.get $h
            i32.const 8
            i32.add
            local.get $sb
            i32.add
            i32.load8_u
            i32.const 0xC0
            i32.and
            i32.const 0x80
            i32.ne
            br_if $contdone
            local.get $sb
            i32.const 1
            i32.add
            local.set $sb
            br $contnext
          )
        )
        local.get $cp
        i32.const 1
        i32.add
        local.set $cp
        br $sbnext
      )
    )
    ;; last = hn - nn (inclusive last candidate byte start; may be < sb)
    local.get $hn
    local.get $nn
    i32.sub
    local.set $last
    ;; off = sb; while off <= last: try an nn-byte match at off
    local.get $sb
    local.set $off
    (block $done
      (loop $next_off
        local.get $off
        local.get $last
        i32.gt_s
        br_if $done
        ;; match = 1; j = 0; while j < nn: if h[8+off+j] != n[8+j] fail
        i32.const 1
        local.set $match
        i32.const 0
        local.set $j
        (block $stop
          (loop $next_char
            local.get $j
            local.get $nn
            i32.ge_s
            br_if $stop
            local.get $h
            i32.const 8
            i32.add
            local.get $off
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            local.get $n
            i32.const 8
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            i32.ne
            if
              i32.const 0
              local.set $match
              br $stop
            end
            local.get $j
            i32.const 1
            i32.add
            local.set $j
            br $next_char
          )
        )
        ;; a full match at byte offset off -> ABSOLUTE code-point index: count
        ;; non-continuation bytes in h[0..off].
        local.get $match
        if
          i32.const 0
          local.set $ci
          i32.const 0
          local.set $p
          (block $cdone
            (loop $cnext
              local.get $p
              local.get $off
              i32.ge_s
              br_if $cdone
              local.get $h
              i32.const 8
              i32.add
              local.get $p
              i32.add
              i32.load8_u
              i32.const 0xC0
              i32.and
              i32.const 0x80
              i32.ne
              if
                local.get $ci
                i32.const 1
                i32.add
                local.set $ci
              end
              local.get $p
              i32.const 1
              i32.add
              local.set $p
              br $cnext
            )
          )
          local.get $ci
          i64.extend_i32_s
          return
        end
        local.get $off
        i32.const 1
        i32.add
        local.set $off
        br $next_off
      )
    )
    ;; no match at or after start -> -1
    i64.const -1
  )
";

/// PMAT-1143: `$__wasm_str_rfind(h, n) -> i64` — Python `h.rfind(n)` (the
/// CODE-POINT index of the LAST occurrence of `n` in `h`, or `-1` if absent)
/// over two length-prefixed UTF-8 strings, returning an `i64` (a Python `int`).
///
/// The reverse-scan sibling of [`STR_FIND_HELPER`]: the SAME naive byte match
/// and the SAME byte-offset → code-point-index conversion (count the
/// non-continuation bytes in `h[0..start]`, `(b & 0xC0) != 0x80`), but the outer
/// slide runs from the LAST candidate start offset (`hn - nn`) DOWN to `0`, so
/// the FIRST match it finds is the RIGHTMOST (last) occurrence — exactly Python
/// `str.rfind`. The conversion is exact because `n[0]` is a LEAD byte, so every
/// match lands on a char boundary; the prefix `h[0..start]` is a whole number of
/// code points.
///
///   * an EMPTY needle → the code-point length of `h` (`$__wasm_str_charlen`):
///     Python `"abc".rfind("")` == 3 and `"".rfind("")` == 0 (the empty string
///     is found at the END). This is the ONE place `rfind` diverges from `find`
///     (whose empty-needle answer is `0`, the START).
///   * a needle LONGER than the haystack → `-1`.
///   * absent → `-1`.
///
/// For non-ASCII input this is char-exact where a byte index would silently
/// diverge (`"héllo".rfind("l")` == 3, not the byte offset 4). Like
/// `$__wasm_str_find` it reads linear memory and allocates NOTHING (an int, not a
/// new string). Emitted once per module (gated on [`module_uses_str_method`] for
/// `Rfind`); its empty-needle `$__wasm_str_charlen` call rides
/// [`module_touches_str`], which any `StrMethod` sets.
const STR_RFIND_HELPER: &str = "\
  ;; __wasm_str_rfind(h, n) = Python h.rfind(n)  (i64: CODE-POINT index of the
  ;; LAST occurrence, or -1). h, n are i32 base-pointers to length-prefixed
  ;; regions (i32 byte count @ base+0, UTF-8 bytes @ base+8). The reverse-scan
  ;; sibling of $__wasm_str_find: SAME naive byte match + byte→code-point index
  ;; conversion, but the outer slide runs from the LAST candidate offset DOWN to
  ;; 0 (the first match found is the last occurrence). Empty needle → charlen(h)
  ;; in CODE POINTS (found at the END). Allocates nothing.
  (func $__wasm_str_rfind (param $h i32) (param $n i32) (result i64)
    (local $hn i32)
    (local $nn i32)
    (local $start i32)
    (local $last i32)
    (local $j i32)
    (local $match i32)
    (local $ci i32)
    (local $p i32)
    ;; hn = len(h); nn = len(n)
    local.get $h
    i32.load
    local.set $hn
    local.get $n
    i32.load
    local.set $nn
    ;; empty needle: Python h.rfind(\"\") == len(h) in CODE POINTS (found at the end)
    local.get $nn
    i32.eqz
    if
      local.get $h
      call $__wasm_str_charlen
      i64.extend_i32_s
      return
    end
    ;; a needle longer than the haystack can never occur → -1
    local.get $nn
    local.get $hn
    i32.gt_s
    if
      i64.const -1
      return
    end
    ;; last = hn - nn  (inclusive last start offset; >= 0, guarded above)
    local.get $hn
    local.get $nn
    i32.sub
    local.set $last
    ;; start = last; while start >= 0: try a len(n)-byte match at offset start,
    ;; scanning DOWN so the FIRST match is the LAST (rightmost) occurrence
    local.get $last
    local.set $start
    (block $done
      (loop $next_start
        local.get $start
        i32.const 0
        i32.lt_s
        br_if $done
        ;; match = 1; j = 0; while j < nn: if h[8+start+j] != n[8+j] fail
        i32.const 1
        local.set $match
        i32.const 0
        local.set $j
        (block $stop
          (loop $next_char
            local.get $j
            local.get $nn
            i32.ge_s
            br_if $stop
            ;; h byte start+j
            local.get $h
            i32.const 8
            i32.add
            local.get $start
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            ;; n byte j
            local.get $n
            i32.const 8
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            i32.ne
            if
              i32.const 0
              local.set $match
              br $stop
            end
            local.get $j
            i32.const 1
            i32.add
            local.set $j
            br $next_char
          )
        )
        ;; a full-length match at byte offset $start → convert to a CODE-POINT
        ;; index: count non-continuation bytes in h[0..start], return it.
        local.get $match
        if
          i32.const 0
          local.set $ci
          i32.const 0
          local.set $p
          (block $cdone
            (loop $cnext
              local.get $p
              local.get $start
              i32.ge_s
              br_if $cdone
              ;; if (h[8+p] & 0xC0) != 0x80  → ci++ (a lead / single byte)
              local.get $h
              i32.const 8
              i32.add
              local.get $p
              i32.add
              i32.load8_u
              i32.const 0xC0
              i32.and
              i32.const 0x80
              i32.ne
              if
                local.get $ci
                i32.const 1
                i32.add
                local.set $ci
              end
              local.get $p
              i32.const 1
              i32.add
              local.set $p
              br $cnext
            )
          )
          local.get $ci
          i64.extend_i32_s
          return
        end
        local.get $start
        i32.const 1
        i32.sub
        local.set $start
        br $next_start
      )
    )
    ;; no match → -1
    i64.const -1
  )
";

/// PMAT-1165: `$__wasm_str_rfind_from(h, n, startc) -> i64` — Python
/// `h.rfind(n, start)` (the CODE-POINT index of the LAST occurrence of `n` in `h`
/// whose match STARTS at or after code-point index `start`, or `-1` if none) over
/// two length-prefixed UTF-8 strings, with a Python `int` (i64) start.
///
/// The start-bounded generalisation of [`STR_RFIND_HELPER`] — equivalently, the
/// reverse-scan sibling of [`STR_FIND_FROM_HELPER`]. It shares find-from's start
/// machinery (clamp the negative/overflow `start`, then decode the `start`-th code
/// point to a byte offset `sb`) and rfind's DOWNWARD slide: candidate offsets run
/// from the LAST fitting offset (`hn - nn`) DOWN to `sb`, so the FIRST match is the
/// RIGHTMOST at or after `start`. The returned index is the ABSOLUTE code-point
/// position in `h` (Python's `rfind` with a start still reports the position in the
/// ORIGINAL string). The full Python start semantics are honoured:
///
///   * `start` is a CODE-POINT index (`$__wasm_str_charlen` gives the length).
///   * a NEGATIVE start counts from the end: `start += charlen`, then clamped up
///     to `0` (`"abcabc".rfind("bc", -3)` == 4; `"abc".rfind("a", -100)` == 0).
///   * `start > charlen` → `-1` — including the empty needle
///     (`"abc".rfind("", 4)` == -1), which is why the `> charlen` guard precedes
///     the empty-needle branch.
///   * an EMPTY needle → `charlen` (`"abc".rfind("", 2)` == 3): unlike `find`
///     (whose empty answer is the clamped START), `rfind`'s empty match is found at
///     the END, so a clamped-in-range `start` never moves it (only `start > charlen`
///     drives it to `-1`, handled by the guard above). This is the ONE place the
///     rfind-from empty answer diverges from find-from's `start`.
///   * a match starting on or after `start` → its ABSOLUTE code-point index; none
///     → `-1` (also the `nn > hn` case, since then `last < 0 <= sb`).
///
/// The byte slide is correct at byte granularity for the same reason
/// `$__wasm_str_rfind` is: `n[0]` is a LEAD byte, so a candidate offset falling
/// mid-code-point (a continuation byte) can never match, and `off >= sb` ⇔ the
/// match starts at a code point ≥ `start`. Reads linear memory and allocates
/// NOTHING (a Python int, not a new string). Its `$__wasm_str_charlen` call rides
/// [`module_touches_str`], which any `StrMethod` sets; emitted once per module
/// (gated on [`module_uses_str_rfind2`], so a plain 1-arg `.rfind(p)` module
/// carries no dead helper).
const STR_RFIND_FROM_HELPER: &str = "\
  ;; __wasm_str_rfind_from(h, n, startc) = Python h.rfind(n, start)  (i64: ABSOLUTE
  ;; CODE-POINT index of the LAST occurrence of n in h whose match starts at or
  ;; after code-point index start, or -1). h, n are i32 base-pointers (i32 byte
  ;; count @ base+0, UTF-8 bytes @ base+8); startc is the (possibly negative) Python
  ;; start as i64. Shares find-from's start clamp + code-point->byte-offset decode,
  ;; but the candidate slide runs from the last fitting offset (hn-nn) DOWN to the
  ;; start-th code point's byte offset (first match = rightmost). Empty needle ->
  ;; charlen(h) (found at the END). Allocates nothing.
  (func $__wasm_str_rfind_from (param $h i32) (param $n i32) (param $startc i64) (result i64)
    (local $hn i32)      ;; len(h) in BYTES
    (local $nn i32)      ;; len(n) in BYTES
    (local $hchars i32)  ;; charlen(h) (code-point count)
    (local $s i32)       ;; clamped start as a code-point index (0..=hchars)
    (local $sb i32)      ;; byte offset of the start-th code point
    (local $cp i32)      ;; code-point counter while converting start->byte
    (local $off i32)     ;; candidate byte offset in the (downward) slide
    (local $last i32)    ;; inclusive last candidate byte offset (hn - nn)
    (local $j i32)
    (local $match i32)
    (local $ci i32)
    (local $p i32)
    ;; hn = len(h) bytes; nn = len(n) bytes; hchars = charlen(h)
    local.get $h
    i32.load
    local.set $hn
    local.get $n
    i32.load
    local.set $nn
    local.get $h
    call $__wasm_str_charlen
    local.set $hchars
    ;; clamp start: if startc < 0 -> startc += hchars; if still < 0 -> 0
    local.get $startc
    i64.const 0
    i64.lt_s
    if
      local.get $startc
      local.get $hchars
      i64.extend_i32_s
      i64.add
      local.set $startc
      local.get $startc
      i64.const 0
      i64.lt_s
      if
        i64.const 0
        local.set $startc
      end
    end
    ;; if startc > hchars -> -1 (covers the empty needle: h.rfind(\"\", len+1) == -1)
    local.get $startc
    local.get $hchars
    i64.extend_i32_s
    i64.gt_s
    if
      i64.const -1
      return
    end
    ;; 0 <= startc <= hchars now; narrow to i32 (charlen fits i32)
    local.get $startc
    i32.wrap_i64
    local.set $s
    ;; empty needle: Python h.rfind(\"\", start) == charlen(h) (found at the END,
    ;; unaffected by an in-range start; the > charlen case already returned -1)
    local.get $nn
    i32.eqz
    if
      local.get $hchars
      i64.extend_i32_s
      return
    end
    ;; convert the start code-point index -> byte offset sb: walk s code points
    i32.const 0
    local.set $sb
    i32.const 0
    local.set $cp
    (block $sbdone
      (loop $sbnext
        local.get $cp
        local.get $s
        i32.ge_s
        br_if $sbdone
        ;; step past the lead byte
        local.get $sb
        i32.const 1
        i32.add
        local.set $sb
        ;; step past continuation bytes: while sb<hn && (h[8+sb]&0xC0)==0x80: sb++
        (block $contdone
          (loop $contnext
            local.get $sb
            local.get $hn
            i32.ge_s
            br_if $contdone
            local.get $h
            i32.const 8
            i32.add
            local.get $sb
            i32.add
            i32.load8_u
            i32.const 0xC0
            i32.and
            i32.const 0x80
            i32.ne
            br_if $contdone
            local.get $sb
            i32.const 1
            i32.add
            local.set $sb
            br $contnext
          )
        )
        local.get $cp
        i32.const 1
        i32.add
        local.set $cp
        br $sbnext
      )
    )
    ;; last = hn - nn (inclusive last candidate byte start; may be < sb → no match,
    ;; and < 0 when nn > hn → the off < sb guard fails immediately → -1)
    local.get $hn
    local.get $nn
    i32.sub
    local.set $last
    ;; off = last; while off >= sb: try an nn-byte match at off, scanning DOWN so
    ;; the FIRST match is the RIGHTMOST occurrence at or after the start code point
    local.get $last
    local.set $off
    (block $done
      (loop $next_off
        local.get $off
        local.get $sb
        i32.lt_s
        br_if $done
        ;; match = 1; j = 0; while j < nn: if h[8+off+j] != n[8+j] fail
        i32.const 1
        local.set $match
        i32.const 0
        local.set $j
        (block $stop
          (loop $next_char
            local.get $j
            local.get $nn
            i32.ge_s
            br_if $stop
            local.get $h
            i32.const 8
            i32.add
            local.get $off
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            local.get $n
            i32.const 8
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            i32.ne
            if
              i32.const 0
              local.set $match
              br $stop
            end
            local.get $j
            i32.const 1
            i32.add
            local.set $j
            br $next_char
          )
        )
        ;; a full match at byte offset off -> ABSOLUTE code-point index: count
        ;; non-continuation bytes in h[0..off].
        local.get $match
        if
          i32.const 0
          local.set $ci
          i32.const 0
          local.set $p
          (block $cdone
            (loop $cnext
              local.get $p
              local.get $off
              i32.ge_s
              br_if $cdone
              local.get $h
              i32.const 8
              i32.add
              local.get $p
              i32.add
              i32.load8_u
              i32.const 0xC0
              i32.and
              i32.const 0x80
              i32.ne
              if
                local.get $ci
                i32.const 1
                i32.add
                local.set $ci
              end
              local.get $p
              i32.const 1
              i32.add
              local.set $p
              br $cnext
            )
          )
          local.get $ci
          i64.extend_i32_s
          return
        end
        local.get $off
        i32.const 1
        i32.sub
        local.set $off
        br $next_off
      )
    )
    ;; no match at or after start -> -1
    i64.const -1
  )
";

/// PMAT-1144: `$__wasm_str_index(h, n) -> i64` — Python `h.index(n)` (the
/// CODE-POINT index of the FIRST occurrence of `n` in `h`).
///
/// The TRAPPING sibling of [`STR_FIND_HELPER`]: `str.index` is `str.find` except
/// that a MISSING needle raises `ValueError` in CPython instead of returning
/// `-1`. We lower that `ValueError` to a WASM `unreachable` (a trap) — the honest
/// WASM analogue of a Python exception on this no-exception ABI. So the helper is
/// a thin wrapper: call `$__wasm_str_find`, and if it returned `-1` (absent),
/// `unreachable`; otherwise return its (already char-indexed) result unchanged.
/// Every present-needle answer is therefore byte-for-byte identical to `find`
/// (same CODE-POINT index, same empty-needle `0`, same multi-byte correctness);
/// the ONLY observable difference is the absent case (trap vs `-1`). Gated on
/// [`module_uses_str_method`] for `StrIndex`, which also forces `STR_FIND_HELPER`
/// (the wrapper calls it). Allocates nothing.
const STR_INDEX_HELPER: &str = "\
  ;; __wasm_str_index(h, n) = Python h.index(n)  (i64: CODE-POINT index of the
  ;; FIRST occurrence). The TRAPPING sibling of $__wasm_str_find: identical to
  ;; find on a present needle, but an ABSENT needle (find → -1) is Python
  ;; ValueError, lowered here to `unreachable` (a WASM trap) rather than -1.
  ;; Allocates nothing.
  (func $__wasm_str_index (param $h i32) (param $n i32) (result i64)
    (local $r i64)
    local.get $h
    local.get $n
    call $__wasm_str_find
    local.set $r
    ;; find returned -1 (needle absent) → Python raises ValueError → trap
    local.get $r
    i64.const -1
    i64.eq
    if
      unreachable
    end
    local.get $r
  )
";

/// PMAT-1144: `$__wasm_str_rindex(h, n) -> i64` — Python `h.rindex(n)` (the
/// CODE-POINT index of the LAST occurrence of `n` in `h`).
///
/// The TRAPPING sibling of [`STR_RFIND_HELPER`], exactly as `$__wasm_str_index`
/// is to `$__wasm_str_find`: `str.rindex` is `str.rfind` except a MISSING needle
/// raises `ValueError` instead of returning `-1`. Wrapper: call
/// `$__wasm_str_rfind`, `unreachable` on `-1`, else return unchanged. Note the
/// EMPTY-needle answer inherits rfind's `charlen(h)` (found at the END), NOT `0`
/// — so `"abc".rindex("")` == 3, matching CPython (the empty string is never a
/// `ValueError`). Gated on [`module_uses_str_method`] for `RIndex`, which forces
/// `STR_RFIND_HELPER`. Allocates nothing.
const STR_RINDEX_HELPER: &str = "\
  ;; __wasm_str_rindex(h, n) = Python h.rindex(n)  (i64: CODE-POINT index of the
  ;; LAST occurrence). The TRAPPING sibling of $__wasm_str_rfind: identical to
  ;; rfind on a present needle (empty needle → charlen(h), found at the END), but
  ;; an ABSENT needle (rfind → -1) is Python ValueError, lowered here to
  ;; `unreachable` (a WASM trap). Allocates nothing.
  (func $__wasm_str_rindex (param $h i32) (param $n i32) (result i64)
    (local $r i64)
    local.get $h
    local.get $n
    call $__wasm_str_rfind
    local.set $r
    ;; rfind returned -1 (needle absent) → Python raises ValueError → trap
    local.get $r
    i64.const -1
    i64.eq
    if
      unreachable
    end
    local.get $r
  )
";

/// PMAT-1032: the CHAR-semantics helper family (non-allocating half).
///
/// CPython strings are sequences of Unicode CODE POINTS; the WASM str ABI is
/// length-prefixed UTF-8 BYTES (i32 byte count @ base+0, bytes @ base+8).
/// Sweep #11 (PMAT-1031 finding 2) confirmed the byte-oriented reads SILENTLY
/// diverge on non-ASCII input: `len("héllo")` returned 6 (bytes) not 5
/// (chars), `for ch in "abé"` iterated 4 times not 3, `ord("é")` trapped, and
/// `s[-1]` trapped where Python indexes from the end. These helpers make every
/// Python-VISIBLE string read char-oriented while the ABI header stays a byte
/// count (concat/eq/copy remain byte operations — byte equality IS char
/// equality for UTF-8).
///
///   * `$__wasm_str_charlen(s) -> i32` — the code-point count: one pass over
///     the payload counting non-continuation bytes (`(b & 0xC0) != 0x80`).
///     Python `len(s)`. O(bytes) per call — correctness over speed, documented.
///   * `$__wasm_str_char_width(b) -> i32` — the encoded width from a LEAD
///     byte: `<0x80 → 1`, `<0xE0 → 2`, `<0xF0 → 3`, else `4`.
///   * `$__wasm_str_char_addr(s, i) -> i32` — the absolute address of the
///     lead byte of char `i`, with Python NEGATIVE-index normalisation
///     (`i < 0 → i += charlen`) and the bounds trap (`unreachable`, the
///     `IndexError` analogue). O(i) walk from the payload start.
///   * `$__wasm_str_ord_at(s, i) -> i64` — the code point of char `i`
///     (Python `ord(s[i])`): `char_addr` + a 1..4-byte UTF-8 decode.
///
/// Emitted whenever the module touches strings ([`module_touches_str`]); none
/// of these allocate, so they are valid without the bump heap. The allocating
/// half ([`STR_CHAR_ALLOC_HELPERS`]) is additionally gated on the heap.
const STR_CHAR_HELPERS: &str = "\
  ;; PMAT-1032 char-semantics helpers: Python-visible string reads are
  ;; CHAR-oriented (code points) over the byte-oriented UTF-8 ABI.
  ;; __wasm_str_charlen(s) = code-point count (Python len(s)).
  (func $__wasm_str_charlen (param $s i32) (result i32)
    (local $p i32)
    (local $end i32)
    (local $c i32)
    local.get $s
    i32.const 8
    i32.add
    local.set $p
    local.get $p
    local.get $s
    i32.load
    i32.add
    local.set $end
    i32.const 0
    local.set $c
    (block $done
      (loop $next
        local.get $p
        local.get $end
        i32.ge_u
        br_if $done
        ;; count the byte unless it is a continuation byte (b & 0xC0) == 0x80
        local.get $p
        i32.load8_u
        i32.const 192
        i32.and
        i32.const 128
        i32.ne
        if
          local.get $c
          i32.const 1
          i32.add
          local.set $c
        end
        local.get $p
        i32.const 1
        i32.add
        local.set $p
        br $next
      )
    )
    local.get $c
  )
  ;; __wasm_str_char_width(b) = UTF-8 width from the LEAD byte b.
  (func $__wasm_str_char_width (param $b i32) (result i32)
    local.get $b
    i32.const 128
    i32.lt_u
    if
      i32.const 1
      return
    end
    local.get $b
    i32.const 224
    i32.lt_u
    if
      i32.const 2
      return
    end
    local.get $b
    i32.const 240
    i32.lt_u
    if
      i32.const 3
      return
    end
    i32.const 4
  )
  ;; __wasm_str_char_addr(s, i) = address of the lead byte of char i.
  ;; Negative i is normalised Python-style (i += charlen); out-of-range
  ;; traps (the IndexError analogue).
  (func $__wasm_str_char_addr (param $s i32) (param $i i64) (result i32)
    (local $cl i64)
    (local $k i64)
    (local $p i32)
    local.get $s
    call $__wasm_str_charlen
    i64.extend_i32_u
    local.set $cl
    local.get $i
    i64.const 0
    i64.lt_s
    if
      local.get $i
      local.get $cl
      i64.add
      local.set $i
    end
    local.get $i
    i64.const 0
    i64.lt_s
    local.get $i
    local.get $cl
    i64.ge_s
    i32.or
    if
      unreachable ;; string index out of range (Python IndexError)
    end
    local.get $s
    i32.const 8
    i32.add
    local.set $p
    i64.const 0
    local.set $k
    (block $done
      (loop $next
        local.get $k
        local.get $i
        i64.ge_s
        br_if $done
        local.get $p
        local.get $p
        i32.load8_u
        call $__wasm_str_char_width
        i32.add
        local.set $p
        local.get $k
        i64.const 1
        i64.add
        local.set $k
        br $next
      )
    )
    local.get $p
  )
  ;; __wasm_str_ord_at(s, i) = code point of char i (Python ord(s[i])).
  (func $__wasm_str_ord_at (param $s i32) (param $i i64) (result i64)
    (local $p i32)
    (local $b0 i32)
    local.get $s
    local.get $i
    call $__wasm_str_char_addr
    local.set $p
    local.get $p
    i32.load8_u
    local.set $b0
    local.get $b0
    i32.const 128
    i32.lt_u
    if
      local.get $b0
      i64.extend_i32_u
      return
    end
    local.get $b0
    i32.const 224
    i32.lt_u
    if
      ;; 2-byte: ((b0 & 0x1F) << 6) | (p[1] & 0x3F)
      local.get $b0
      i32.const 31
      i32.and
      i32.const 6
      i32.shl
      local.get $p
      i32.load8_u offset=1
      i32.const 63
      i32.and
      i32.or
      i64.extend_i32_u
      return
    end
    local.get $b0
    i32.const 240
    i32.lt_u
    if
      ;; 3-byte: ((b0 & 0x0F) << 12) | ((p[1] & 0x3F) << 6) | (p[2] & 0x3F)
      local.get $b0
      i32.const 15
      i32.and
      i32.const 12
      i32.shl
      local.get $p
      i32.load8_u offset=1
      i32.const 63
      i32.and
      i32.const 6
      i32.shl
      i32.or
      local.get $p
      i32.load8_u offset=2
      i32.const 63
      i32.and
      i32.or
      i64.extend_i32_u
      return
    end
    ;; 4-byte: ((b0 & 0x07) << 18) | ((p[1] & 0x3F) << 12)
    ;;         | ((p[2] & 0x3F) << 6) | (p[3] & 0x3F)
    local.get $b0
    i32.const 7
    i32.and
    i32.const 18
    i32.shl
    local.get $p
    i32.load8_u offset=1
    i32.const 63
    i32.and
    i32.const 12
    i32.shl
    i32.or
    local.get $p
    i32.load8_u offset=2
    i32.const 63
    i32.and
    i32.const 6
    i32.shl
    i32.or
    local.get $p
    i32.load8_u offset=3
    i32.const 63
    i32.and
    i32.or
    i64.extend_i32_u
  )
";

/// PMAT-1032: the CHAR-semantics helper family (allocating half) — gated on
/// the bump heap (both call `$__alloc`), emitted after [`STR_CHAR_HELPERS`].
///
///   * `$__wasm_str_char_at(s, i) -> i32` — Python `s[i]`: a NEW heap string
///     holding char `i` of `s` — the full 1..4-byte encoded char, never a
///     lone byte (the pre-PMAT-1032 lowering copied ONE byte, shredding
///     multi-byte chars). Negative indexing + bounds trap via `char_addr`.
///   * `$__wasm_chr(n) -> i32` — Python `chr(n)`: a NEW heap string holding
///     the UTF-8 encoding of code point `n` (1..4 bytes — the pre-PMAT-1032
///     lowering masked `n & 0xFF` into a single byte, SILENTLY wrong for
///     every n > 127 and not valid UTF-8 for 128..255). `n` outside
///     `0..=0x10FFFF` traps (the Python `ValueError` analogue). Surrogates
///     encode via the generic 3-byte pattern (WTF-8), matching `ord`'s
///     decoder — CPython also allows lone surrogates in `chr`.
const STR_CHAR_ALLOC_HELPERS: &str = "\
  ;; __wasm_str_char_at(s, i) = a NEW heap string holding char i of s
  ;; (Python s[i] — one CHAR, 1..4 bytes).
  (func $__wasm_str_char_at (param $s i32) (param $i i64) (result i32)
    (local $p i32)
    (local $w i32)
    (local $dst i32)
    (local $k i32)
    local.get $s
    local.get $i
    call $__wasm_str_char_addr
    local.set $p
    local.get $p
    i32.load8_u
    call $__wasm_str_char_width
    local.set $w
    local.get $w
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $w
    i32.store
    i32.const 0
    local.set $k
    (block $done
      (loop $next
        local.get $k
        local.get $w
        i32.ge_s
        br_if $done
        local.get $dst
        i32.const 8
        i32.add
        local.get $k
        i32.add
        local.get $p
        local.get $k
        i32.add
        i32.load8_u
        i32.store8
        local.get $k
        i32.const 1
        i32.add
        local.set $k
        br $next
      )
    )
    local.get $dst
  )
  ;; __wasm_chr(n) = a NEW heap string holding the UTF-8 encoding of code
  ;; point n (Python chr(n)); n outside 0..=0x10FFFF traps (ValueError).
  (func $__wasm_chr (param $n i64) (result i32)
    (local $c i32)
    (local $w i32)
    (local $dst i32)
    local.get $n
    i64.const 0
    i64.lt_s
    local.get $n
    i64.const 1114111
    i64.gt_s
    i32.or
    if
      unreachable ;; chr() arg not in range(0x110000) (Python ValueError)
    end
    local.get $n
    i32.wrap_i64
    local.set $c
    i32.const 1
    local.set $w
    local.get $c
    i32.const 128
    i32.ge_u
    if
      i32.const 2
      local.set $w
    end
    local.get $c
    i32.const 2048
    i32.ge_u
    if
      i32.const 3
      local.set $w
    end
    local.get $c
    i32.const 65536
    i32.ge_u
    if
      i32.const 4
      local.set $w
    end
    local.get $w
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $w
    i32.store
    local.get $w
    i32.const 1
    i32.eq
    if
      local.get $dst
      local.get $c
      i32.store8 offset=8
    end
    local.get $w
    i32.const 2
    i32.eq
    if
      ;; 0xC0 | (c >> 6), 0x80 | (c & 0x3F)
      local.get $dst
      local.get $c
      i32.const 6
      i32.shr_u
      i32.const 192
      i32.or
      i32.store8 offset=8
      local.get $dst
      local.get $c
      i32.const 63
      i32.and
      i32.const 128
      i32.or
      i32.store8 offset=9
    end
    local.get $w
    i32.const 3
    i32.eq
    if
      ;; 0xE0 | (c >> 12), 0x80 | ((c >> 6) & 0x3F), 0x80 | (c & 0x3F)
      local.get $dst
      local.get $c
      i32.const 12
      i32.shr_u
      i32.const 224
      i32.or
      i32.store8 offset=8
      local.get $dst
      local.get $c
      i32.const 6
      i32.shr_u
      i32.const 63
      i32.and
      i32.const 128
      i32.or
      i32.store8 offset=9
      local.get $dst
      local.get $c
      i32.const 63
      i32.and
      i32.const 128
      i32.or
      i32.store8 offset=10
    end
    local.get $w
    i32.const 4
    i32.eq
    if
      ;; 0xF0 | (c >> 18), then three 0x80 | six-bit groups
      local.get $dst
      local.get $c
      i32.const 18
      i32.shr_u
      i32.const 240
      i32.or
      i32.store8 offset=8
      local.get $dst
      local.get $c
      i32.const 12
      i32.shr_u
      i32.const 63
      i32.and
      i32.const 128
      i32.or
      i32.store8 offset=9
      local.get $dst
      local.get $c
      i32.const 6
      i32.shr_u
      i32.const 63
      i32.and
      i32.const 128
      i32.or
      i32.store8 offset=10
      local.get $dst
      local.get $c
      i32.const 63
      i32.and
      i32.const 128
      i32.or
      i32.store8 offset=11
    end
    local.get $dst
  )
";

/// PMAT-1058: the string-SLICE helper (allocating — rides the `needs_heap`
/// gate like [`STR_CHAR_ALLOC_HELPERS`], and calls `$__wasm_str_charlen` +
/// `$__wasm_str_char_width` from [`STR_CHAR_HELPERS`]).
///
///   * `$__wasm_str_slice(s, lo, hi) -> i32` — Python `s[lo:hi]` (char-exact,
///     no step): materialise a NEW heap string holding the CHARACTERS in the
///     half-open range `[lo, hi)`. `lo`/`hi` are i64 CHARACTER indices with
///     full Python slice semantics — a negative bound is normalised (`+= len`),
///     both bounds CLAMP to `[0, len]` (out-of-range slice bounds never trap,
///     unlike `s[i]`), and `hi` is raised to `lo` when it would fall below it
///     (an empty slice, never a negative length). A missing bound is passed as
///     `0` (lo) / `i64::MAX` (hi) by the lowering and clamps to `0` / `len`.
///
/// The substring bytes are found by two CHAR walks from `base+8` (each byte
/// advanced by its UTF-8 lead-byte width), so the copied byte range is exactly
/// the encoding of chars `[lo, hi)` — char-exact for non-ASCII, never a byte
/// slice that could split a multi-byte code point. The result is a fresh
/// length-prefixed heap string (i32 byte-count header + UTF-8 bytes), so it
/// composes uniformly with `len` / `Concat` / equality / a str RETURN.
const STR_SLICE_HELPER: &str = "\
  ;; __wasm_str_slice(s, lo, hi) = a NEW heap string holding chars [lo, hi) of s
  ;; (Python s[lo:hi], char-exact, Python clamp semantics, no step).
  (func $__wasm_str_slice (param $s i32) (param $lo i64) (param $hi i64) (result i32)
    (local $cl i64)
    (local $begin i32)
    (local $p i32)
    (local $k i64)
    (local $startoff i32)
    (local $endoff i32)
    (local $nlen i32)
    (local $dst i32)
    ;; cl = charlen(s); begin = base+8 (first payload byte).
    local.get $s
    call $__wasm_str_charlen
    i64.extend_i32_u
    local.set $cl
    local.get $s
    i32.const 8
    i32.add
    local.set $begin
    ;; --- normalise lo: if lo<0 lo+=cl; then clamp to [0, cl] ---
    local.get $lo
    i64.const 0
    i64.lt_s
    if
      local.get $lo
      local.get $cl
      i64.add
      local.set $lo
    end
    local.get $lo
    i64.const 0
    i64.lt_s
    if
      i64.const 0
      local.set $lo
    end
    local.get $lo
    local.get $cl
    i64.gt_s
    if
      local.get $cl
      local.set $lo
    end
    ;; --- normalise hi: if hi<0 hi+=cl; then clamp to [0, cl] ---
    local.get $hi
    i64.const 0
    i64.lt_s
    if
      local.get $hi
      local.get $cl
      i64.add
      local.set $hi
    end
    local.get $hi
    i64.const 0
    i64.lt_s
    if
      i64.const 0
      local.set $hi
    end
    local.get $hi
    local.get $cl
    i64.gt_s
    if
      local.get $cl
      local.set $hi
    end
    ;; hi = max(hi, lo) — an empty slice when hi < lo, never a negative length.
    local.get $hi
    local.get $lo
    i64.lt_s
    if
      local.get $lo
      local.set $hi
    end
    ;; --- walk to the byte offset of char lo (startoff), then char hi (endoff) ---
    local.get $begin
    local.set $p
    i64.const 0
    local.set $k
    (block $s_done
      (loop $s_next
        local.get $k
        local.get $lo
        i64.ge_s
        br_if $s_done
        local.get $p
        local.get $p
        i32.load8_u
        call $__wasm_str_char_width
        i32.add
        local.set $p
        local.get $k
        i64.const 1
        i64.add
        local.set $k
        br $s_next
      )
    )
    local.get $p
    local.get $begin
    i32.sub
    local.set $startoff
    (block $e_done
      (loop $e_next
        local.get $k
        local.get $hi
        i64.ge_s
        br_if $e_done
        local.get $p
        local.get $p
        i32.load8_u
        call $__wasm_str_char_width
        i32.add
        local.set $p
        local.get $k
        i64.const 1
        i64.add
        local.set $k
        br $e_next
      )
    )
    local.get $p
    local.get $begin
    i32.sub
    local.set $endoff
    ;; nlen = endoff - startoff (byte length of the substring).
    local.get $endoff
    local.get $startoff
    i32.sub
    local.set $nlen
    ;; dst = alloc(8 + nlen); header = nlen; copy the bytes.
    local.get $nlen
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $nlen
    i32.store
    local.get $dst
    i32.const 8
    i32.add
    local.get $begin
    local.get $startoff
    i32.add
    local.get $nlen
    memory.copy
    local.get $dst
  )
";

/// PMAT-1142: the string-REPEAT helper (allocating — rides the `needs_heap`
/// gate, calls `$__alloc`).
///
///   * `$__wasm_str_repeat(s, k) -> i32` — Python `s * n`: materialise a NEW
///     heap string holding the UTF-8 payload of `s` replicated `max(k, 0)`
///     times. PURE byte replication — no case/code-point transform — so it is
///     char-EXACT for any valid UTF-8 (a multi-byte code point is copied whole
///     each pass), i.e. it IS Python `str * int` for every string, ASCII or not.
///     A count `k <= 0` clamps to the empty string (Python `"x" * -1 == ""`).
///
/// One pass: (1) `reps = max(k, 0)`; (2) `slen = header(s)`;
/// (3) `total = slen * reps`; (4) `dst = $__alloc(8 + total)`, store the i32
/// byte-count header `total`; (5) loop `reps` times, each `memory.copy`ing the
/// `slen` source bytes from `s + 8` to `dst + 8 + off`, advancing `off`. The
/// byte-count header equals the Python CHAR count of the result iff it does for
/// `s` — replication multiplies both, so the result composes uniformly with
/// `len` / `Concat` / equality / a str RETURN like any other heap string.
const STR_REPEAT_HELPER: &str = "\
  ;; PMAT-1142 __wasm_str_repeat(s, k) = a NEW heap string = the UTF-8 bytes of s
  ;; replicated max(k, 0) times (Python s * n). PURE byte replication — char-exact
  ;; for valid UTF-8, so this IS Python str * int for any string; k <= 0 -> \"\".
  (func $__wasm_str_repeat (param $s i32) (param $k i64) (result i32)
    (local $reps i32)
    (local $slen i32)
    (local $total i32)
    (local $dst i32)
    (local $off i32)
    (local $i i32)
    ;; reps = max(k, 0) as i32 — a negative count clamps to the empty string.
    local.get $k
    i64.const 0
    i64.lt_s
    if
      i32.const 0
      local.set $reps
    else
      local.get $k
      i32.wrap_i64
      local.set $reps
    end
    ;; slen = the i32 byte-count header of s.
    local.get $s
    i32.load
    local.set $slen
    ;; total = slen * reps.
    local.get $slen
    local.get $reps
    i32.mul
    local.set $total
    ;; dst = alloc(8 + total); store the i32 byte-count header = total.
    local.get $total
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $total
    i32.store
    ;; loop reps times: memory.copy slen bytes from s+8 to dst+8+off; off += slen.
    i32.const 0
    local.set $off
    i32.const 0
    local.set $i
    (block $r_done
      (loop $r_next
        local.get $i
        local.get $reps
        i32.ge_s
        br_if $r_done
        ;; dest = dst + 8 + off
        local.get $dst
        i32.const 8
        i32.add
        local.get $off
        i32.add
        ;; src = s + 8
        local.get $s
        i32.const 8
        i32.add
        ;; n = slen
        local.get $slen
        memory.copy
        ;; off += slen
        local.get $off
        local.get $slen
        i32.add
        local.set $off
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $r_next
      )
    )
    local.get $dst
  )
";

/// PMAT-1153: `$__wasm_str_removeprefix(s, p) -> i32` — Python `s.removeprefix(p)`:
/// a NEW heap string equal to `s` with a leading `p` removed if (and only if)
/// `s` starts with `p`, else a fresh copy of `s`. Allocating (rides the
/// `needs_heap` gate, calls `$__alloc`), and FORCES `$__wasm_str_startswith`
/// (`needs_removeprefix` folds into `needs_startswith`). The prefix test is a
/// byte compare and the retained tail `s[len(p)..]` starts on a code-point
/// boundary (Python `p` is whole code points, so `len(p)` bytes end on a char
/// boundary) — so the pure byte copy IS char-exact for any valid UTF-8, no
/// byte→code-point reasoning needed (unlike find/rfind). Empty `p` → a copy of
/// `s` (startswith("") is true but `len(p)` = 0, off = 0); a `p` longer than `s`
/// or not a prefix → off = 0 → a whole copy. `memory.copy` of 0 bytes is a nop.
const STR_REMOVEPREFIX_HELPER: &str = "\
  ;; PMAT-1153 __wasm_str_removeprefix(s, p) = Python s.removeprefix(p) — a NEW
  ;; heap string = s[len(p):] when s starts with p (byte compare), else a copy of
  ;; s. Byte copy is char-exact (the tail starts on a code-point boundary).
  (func $__wasm_str_removeprefix (param $s i32) (param $p i32) (result i32)
    (local $slen i32)
    (local $off i32)
    (local $rlen i32)
    (local $dst i32)
    ;; slen = len(s); off = 0
    local.get $s
    i32.load
    local.set $slen
    i32.const 0
    local.set $off
    ;; if s.startswith(p): off = len(p)
    local.get $s
    local.get $p
    call $__wasm_str_startswith
    if
      local.get $p
      i32.load
      local.set $off
    end
    ;; rlen = slen - off
    local.get $slen
    local.get $off
    i32.sub
    local.set $rlen
    ;; dst = alloc(8 + rlen); store the i32 byte-count header = rlen.
    local.get $rlen
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $rlen
    i32.store
    ;; memory.copy dst+8 <- s+8+off, rlen bytes.
    local.get $dst
    i32.const 8
    i32.add
    local.get $s
    i32.const 8
    i32.add
    local.get $off
    i32.add
    local.get $rlen
    memory.copy
    local.get $dst
  )
";

/// PMAT-1153: `$__wasm_str_removesuffix(s, p) -> i32` — Python `s.removesuffix(p)`:
/// the suffix mirror of [`STR_REMOVEPREFIX_HELPER`]. A NEW heap string equal to
/// `s` with a trailing `p` removed if (and only if) `s` ends with `p`, else a
/// fresh copy of `s`. Allocating; FORCES `$__wasm_str_endswith`
/// (`needs_removesuffix` folds into `needs_endswith`). Retains the FIRST
/// `rlen = len(s) - len(p)` bytes (or all of `s` when `p` is not a suffix); that
/// cut lands on a code-point boundary (Python `p` is whole code points), so the
/// byte copy is char-exact. Empty `p` → endswith("") is true but `len(p)` = 0, so
/// `rlen` = `slen` → a whole copy (Python `\"abc\".removesuffix(\"\")` == `\"abc\"`).
const STR_REMOVESUFFIX_HELPER: &str = "\
  ;; PMAT-1153 __wasm_str_removesuffix(s, p) = Python s.removesuffix(p) — a NEW
  ;; heap string = s[:len(s)-len(p)] when s ends with p (byte compare), else a
  ;; copy of s. Byte copy is char-exact (the cut lands on a code-point boundary).
  (func $__wasm_str_removesuffix (param $s i32) (param $p i32) (result i32)
    (local $slen i32)
    (local $rlen i32)
    (local $dst i32)
    ;; slen = len(s); rlen = slen (the not-a-suffix / copy default)
    local.get $s
    i32.load
    local.set $slen
    local.get $slen
    local.set $rlen
    ;; if s.endswith(p): rlen = slen - len(p)
    local.get $s
    local.get $p
    call $__wasm_str_endswith
    if
      local.get $slen
      local.get $p
      i32.load
      i32.sub
      local.set $rlen
    end
    ;; dst = alloc(8 + rlen); store the i32 byte-count header = rlen.
    local.get $rlen
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $rlen
    i32.store
    ;; memory.copy dst+8 <- s+8, first rlen bytes (the retained prefix).
    local.get $dst
    i32.const 8
    i32.add
    local.get $s
    i32.const 8
    i32.add
    local.get $rlen
    memory.copy
    local.get $dst
  )
";

/// PMAT-1159: `$__wasm_str_replace(s, old, new) -> i32` — Python
/// `s.replace(old, new)`: a NEW heap string with EVERY non-overlapping
/// occurrence of `old` replaced by `new`, scanned left to right. Allocating
/// (rides the `needs_heap` gate, calls `$__alloc`).
///
/// Two regimes, both pinned to CPython:
///   * NON-EMPTY `old` — a byte substring replace. PASS 1 counts non-overlapping
///     matches (the same byte slide as `$__wasm_str_count`); PASS 2 re-walks `s`
///     copying literal bytes and, at each match, the `new` bytes (advancing the
///     source by `len(old)`), then copies the trailing `len(old)-1` bytes that
///     cannot host a match. Output size is exactly `slen + cnt*(nlen - olen)`
///     (the delta may be negative when `new` is shorter — plain i32 arithmetic).
///     Byte search == code-point search for valid UTF-8: `old[0]` is a LEAD byte
///     (never a `0x80..0xBF` continuation), so a match starts on a char boundary
///     and — `old` being whole code points — spans whole chars; the literal
///     single-byte copies only ever move bytes WITHIN a non-replaced char, so the
///     pure byte machinery is char-exact (no split multibyte char, no false
///     positive on a shared continuation byte).
///   * EMPTY `old` — Python interleaves `new` between every code point and at
///     both ends (`"ab".replace("", "-")` == `"-a-b-"`, `"".replace("", "-")` ==
///     `"-"`). This is the ONE regime that must be CODE-POINT aware: it emits
///     `new`, then for each char (walked via `$__wasm_str_char_width`) the char's
///     bytes followed by `new`. Output size is `nlen*(charlen(s)+1) + slen`.
///     Trapping here (like `index`/`rindex`) would be WRONG — Python never raises
///     on an empty pattern, so a trap would be a silent divergence, not a
///     ValueError analogue.
///
/// Empty `new` is a deletion (`memory.copy` of 0 bytes is a nop). `old` longer
/// than `s`, or absent, yields a fresh copy of `s` (`last < 0` / `cnt = 0`).
/// Calls `$__wasm_str_charlen` / `$__wasm_str_char_width` (co-emitted for any
/// str-touching module via `module_touches_str`, which a str-returning replace
/// always satisfies).
const STR_REPLACE_HELPER: &str = "\
  ;; PMAT-1159/1161 __wasm_str_replace(s, old, new, count) = Python
  ;; s.replace(old, new[, count]) — a NEW heap string with the first `count`
  ;; non-overlapping `old` replaced by `new`, left to right. count < 0 means
  ;; UNLIMITED (the 2-arg form passes -1, exactly reproducing replace-all); the
  ;; cap bounds BOTH regimes. Non-empty old: two byte passes (count, then
  ;; copy-with-substitution), char-exact for valid UTF-8 (old[0] is a lead byte).
  ;; Empty old: Python interleaves new between every code point and at both ends
  ;; (a char walk); count caps how many of the charlen+1 gaps get `new`.
  (func $__wasm_str_replace (param $s i32) (param $old i32) (param $new i32) (param $count i64) (result i32)
    (local $slen i32)
    (local $olen i32)
    (local $nlen i32)
    (local $cnt i32)
    (local $start i32)
    (local $last i32)
    (local $j i32)
    (local $match i32)
    (local $out i32)
    (local $dst i32)
    (local $d i32)
    (local $p i32)
    (local $end i32)
    (local $w i32)
    (local $cap i32)
    (local $made i32)
    (local $k i32)
    (local $cl i32)
    (local $gi i32)
    local.get $s
    i32.load
    local.set $slen
    local.get $old
    i32.load
    local.set $olen
    local.get $new
    i32.load
    local.set $nlen
    ;; ── empty `old`: interleave `new` between every code point + at both ends ──
    local.get $olen
    i32.eqz
    if
      ;; cl = charlen(s); k = number of the (cl+1) gaps that get `new`.
      ;; count < 0 → all cl+1 gaps (unlimited); else min(count, cl+1).
      local.get $s
      call $__wasm_str_charlen
      local.set $cl
      local.get $count
      i64.const 0
      i64.lt_s
      if (result i32)
        local.get $cl
        i32.const 1
        i32.add
      else
        local.get $count
        local.get $cl
        i32.const 1
        i32.add
        i64.extend_i32_s
        i64.ge_s
        if (result i32)
          local.get $cl
          i32.const 1
          i32.add
        else
          local.get $count
          i32.wrap_i64
        end
      end
      local.set $k
      ;; out = nlen*k + slen
      local.get $nlen
      local.get $k
      i32.mul
      local.get $slen
      i32.add
      local.set $out
      ;; dst = alloc(8 + out); header = out; d = dst+8
      local.get $out
      i32.const 8
      i32.add
      call $__alloc
      local.set $dst
      local.get $dst
      local.get $out
      i32.store
      local.get $dst
      i32.const 8
      i32.add
      local.set $d
      ;; gi = 0 (gap index); leading gap (before char 0): emit `new` iff gi < k
      i32.const 0
      local.set $gi
      local.get $gi
      local.get $k
      i32.lt_s
      if
        local.get $d
        local.get $new
        i32.const 8
        i32.add
        local.get $nlen
        memory.copy
        local.get $d
        local.get $nlen
        i32.add
        local.set $d
      end
      local.get $gi
      i32.const 1
      i32.add
      local.set $gi
      ;; walk chars: p = s+8; end = s+8+slen
      local.get $s
      i32.const 8
      i32.add
      local.set $p
      local.get $p
      local.get $slen
      i32.add
      local.set $end
      (block $cdone
        (loop $cnext
          local.get $p
          local.get $end
          i32.ge_u
          br_if $cdone
          ;; w = char_width(lead byte at p)
          local.get $p
          i32.load8_u
          call $__wasm_str_char_width
          local.set $w
          ;; copy the char's w bytes, then advance d and p by w
          local.get $d
          local.get $p
          local.get $w
          memory.copy
          local.get $d
          local.get $w
          i32.add
          local.set $d
          local.get $p
          local.get $w
          i32.add
          local.set $p
          ;; gap after this char: emit `new` iff gi < k, then gi++
          local.get $gi
          local.get $k
          i32.lt_s
          if
            local.get $d
            local.get $new
            i32.const 8
            i32.add
            local.get $nlen
            memory.copy
            local.get $d
            local.get $nlen
            i32.add
            local.set $d
          end
          local.get $gi
          i32.const 1
          i32.add
          local.set $gi
          br $cnext
        )
      )
      local.get $dst
      return
    end
    ;; ── non-empty `old`: PASS 1 — count non-overlapping matches ──────────────
    i32.const 0
    local.set $cnt
    ;; last = slen - olen (inclusive last candidate start; may be < 0 → no match)
    local.get $slen
    local.get $olen
    i32.sub
    local.set $last
    ;; cap = max substitutions: count < 0 → slen+1 (>= any possible match count =
    ;; unlimited); else min(count, slen+1). Bounds PASS 1 so cnt = min(matches, cap).
    local.get $count
    i64.const 0
    i64.lt_s
    if (result i32)
      local.get $slen
      i32.const 1
      i32.add
    else
      local.get $count
      local.get $slen
      i32.const 1
      i32.add
      i64.extend_i32_s
      i64.ge_s
      if (result i32)
        local.get $slen
        i32.const 1
        i32.add
      else
        local.get $count
        i32.wrap_i64
      end
    end
    local.set $cap
    i32.const 0
    local.set $start
    (block $done1
      (loop $next1
        local.get $start
        local.get $last
        i32.gt_s
        br_if $done1
        ;; cap reached (checked at the TOP so cap == 0 counts nothing) → stop;
        ;; further matches are copied verbatim in PASS 2, so cnt = min(matches, cap).
        local.get $cnt
        local.get $cap
        i32.ge_s
        br_if $done1
        ;; match = 1; j = 0; while j < olen: if s[8+start+j] != old[8+j] fail
        i32.const 1
        local.set $match
        i32.const 0
        local.set $j
        (block $stop1
          (loop $nc1
            local.get $j
            local.get $olen
            i32.ge_s
            br_if $stop1
            local.get $s
            i32.const 8
            i32.add
            local.get $start
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            local.get $old
            i32.const 8
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            i32.ne
            if
              i32.const 0
              local.set $match
              br $stop1
            end
            local.get $j
            i32.const 1
            i32.add
            local.set $j
            br $nc1
          )
        )
        local.get $match
        if
          local.get $cnt
          i32.const 1
          i32.add
          local.set $cnt
          local.get $start
          local.get $olen
          i32.add
          local.set $start
        else
          local.get $start
          i32.const 1
          i32.add
          local.set $start
        end
        br $next1
      )
    )
    ;; out = slen + cnt*(nlen - olen); dst = alloc(8+out); header; d = dst+8
    local.get $slen
    local.get $cnt
    local.get $nlen
    local.get $olen
    i32.sub
    i32.mul
    i32.add
    local.set $out
    local.get $out
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $out
    i32.store
    local.get $dst
    i32.const 8
    i32.add
    local.set $d
    ;; ── PASS 2 — copy with substitution (same match logic as PASS 1) ─────────
    ;; `made` tracks substitutions; once it reaches `cnt` (the capped count) the
    ;; loop stops and the trailing-tail copy emits the rest of `s` verbatim — any
    ;; beyond-cap `old` occurrences are left untouched, exactly as Python does.
    i32.const 0
    local.set $start
    i32.const 0
    local.set $made
    (block $done2
      (loop $next2
        local.get $made
        local.get $cnt
        i32.ge_s
        br_if $done2
        local.get $start
        local.get $last
        i32.gt_s
        br_if $done2
        i32.const 1
        local.set $match
        i32.const 0
        local.set $j
        (block $stop2
          (loop $nc2
            local.get $j
            local.get $olen
            i32.ge_s
            br_if $stop2
            local.get $s
            i32.const 8
            i32.add
            local.get $start
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            local.get $old
            i32.const 8
            i32.add
            local.get $j
            i32.add
            i32.load8_u
            i32.ne
            if
              i32.const 0
              local.set $match
              br $stop2
            end
            local.get $j
            i32.const 1
            i32.add
            local.set $j
            br $nc2
          )
        )
        local.get $match
        if
          ;; copy `new`, advance d by nlen, advance start by olen (non-overlapping)
          local.get $d
          local.get $new
          i32.const 8
          i32.add
          local.get $nlen
          memory.copy
          local.get $d
          local.get $nlen
          i32.add
          local.set $d
          local.get $start
          local.get $olen
          i32.add
          local.set $start
          ;; one substitution done — count it against the cap
          local.get $made
          i32.const 1
          i32.add
          local.set $made
        else
          ;; copy the single literal byte s[8+start], advance d and start by 1
          local.get $d
          local.get $s
          i32.const 8
          i32.add
          local.get $start
          i32.add
          i32.const 1
          memory.copy
          local.get $d
          i32.const 1
          i32.add
          local.set $d
          local.get $start
          i32.const 1
          i32.add
          local.set $start
        end
        br $next2
      )
    )
    ;; copy the trailing tail s[start..slen] (bytes past the last candidate start)
    local.get $d
    local.get $s
    i32.const 8
    i32.add
    local.get $start
    i32.add
    local.get $slen
    local.get $start
    i32.sub
    memory.copy
    local.get $dst
  )
";

/// PMAT-1173: `$__wasm_str_zfill(s, w) -> i32` — Python `s.zfill(width)`: a NEW
/// heap string left-padded with ASCII `'0'` (`0x30`) to `width` CODE POINTS,
/// **sign-aware** (a leading `'+'` / `'-'` stays first, the zeros go AFTER it).
/// Allocating (rides the `needs_heap` gate, calls `$__alloc`). Calls
/// `$__wasm_str_charlen` (co-emitted for any str-touching module) for the width
/// math.
///
/// The pad count is `max(0, width - charlen(s))` — a `width` no larger than the
/// current code-point length is a plain COPY of `s` (`"42".zfill(1)` == `"42"`,
/// `"".zfill(0)` == `""`). The `'0'` bytes are pure ASCII inserted at a
/// code-point boundary (either the very start, or immediately after a 1-byte
/// `'+'`/`'-'` sign), and the rest of `s` is copied byte-for-byte, so the result
/// is CHAR-EXACT for any valid UTF-8 (`"café".zfill(6)` == `"00café"`,
/// `"-é".zfill(4)` == `"-00é"`) — no Unicode fold, no byte↔code-point ambiguity.
///
/// The sign test reads `s`'s FIRST payload byte, guarded by `slen > 0` so an
/// EMPTY `s` never reads past its (zero-length) payload — `"".zfill(3)` == `"000"`
/// (no sign, three zeros). `memory.fill` of 0 bytes and `memory.copy` of 0 bytes
/// are both nops, so the `pad == 0` (copy) and `sign, slen == 1` (`"+".zfill(3)`
/// == `"+00"`) boundaries fall out of the general path with no special case.
const STR_ZFILL_HELPER: &str = "\
  ;; PMAT-1173 __wasm_str_zfill(s, w) = Python s.zfill(width) — a NEW heap string
  ;; left-padded with ASCII '0' to `width` CODE POINTS, sign-aware ('+'/'-' stays
  ;; first). Pad = max(0, width - charlen(s)); the '0' bytes land on a code-point
  ;; boundary and the rest is a byte copy, so it is char-exact for any UTF-8.
  (func $__wasm_str_zfill (param $s i32) (param $w i64) (result i32)
    (local $slen i32)
    (local $n i32)
    (local $pad i32)
    (local $rlen i32)
    (local $dst i32)
    (local $sign i32)
    (local $wpos i32)
    (local $c i32)
    ;; slen = byte length of s; n = code-point count of s.
    local.get $s
    i32.load
    local.set $slen
    local.get $s
    call $__wasm_str_charlen
    local.set $n
    ;; pad = wrap(w) - n ; clamp to >= 0 (width <= len -> plain copy).
    local.get $w
    i32.wrap_i64
    local.get $n
    i32.sub
    local.set $pad
    local.get $pad
    i32.const 0
    i32.lt_s
    if
      i32.const 0
      local.set $pad
    end
    ;; sign = 1 iff s is non-empty and s[0] is '+' (0x2B) or '-' (0x2D).
    i32.const 0
    local.set $sign
    local.get $slen
    i32.const 0
    i32.gt_s
    if
      local.get $s
      i32.const 8
      i32.add
      i32.load8_u
      local.set $c
      local.get $c
      i32.const 0x2b
      i32.eq
      local.get $c
      i32.const 0x2d
      i32.eq
      i32.or
      if
        i32.const 1
        local.set $sign
      end
    end
    ;; rlen = slen + pad ; dst = alloc(8 + rlen) ; store the i32 header = rlen.
    local.get $slen
    local.get $pad
    i32.add
    local.set $rlen
    local.get $rlen
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $rlen
    i32.store
    ;; wpos = dst + 8 (payload write cursor).
    local.get $dst
    i32.const 8
    i32.add
    local.set $wpos
    ;; if sign: copy the 1 sign byte (s[8]) to wpos; wpos += 1.
    local.get $sign
    if
      local.get $wpos
      local.get $s
      i32.const 8
      i32.add
      i32.load8_u
      i32.store8
      local.get $wpos
      i32.const 1
      i32.add
      local.set $wpos
    end
    ;; fill `pad` '0' (0x30) bytes at wpos ; wpos += pad. (nop when pad == 0.)
    local.get $wpos
    i32.const 0x30
    local.get $pad
    memory.fill
    local.get $wpos
    local.get $pad
    i32.add
    local.set $wpos
    ;; copy the source tail (slen - sign bytes from s+8+sign) to wpos. (nop when
    ;; that length is 0, e.g. \"+\".zfill(3).)
    local.get $wpos
    local.get $s
    i32.const 8
    i32.add
    local.get $sign
    i32.add
    local.get $slen
    local.get $sign
    i32.sub
    memory.copy
    local.get $dst
  )
";

/// PMAT-1185: `$__wasm_str_upper_lower(s, up) -> i32` — Python `s.upper()` (`up`
/// = 1) / `s.lower()` (`up` = 0): a NEW heap string with every ASCII letter
/// case-flipped. Allocating (rides the `needs_heap` gate, calls `$__alloc`).
///
/// **ASCII-only, with an honest runtime boundary.** Python's `str.upper()` /
/// `str.lower()` do FULL Unicode case folding (`"café".upper() == "CAFÉ"`), which
/// needs a case table this scalar lane does not carry. So the helper case-flips
/// only the ASCII letters (`A`–`Z` ↔ `a`–`z`) and, on the FIRST byte `>= 0x80`
/// (any byte of a non-ASCII code point in valid UTF-8), executes `unreachable` —
/// a TRAP, exactly like the `index` / `rindex` ValueError siblings. It NEVER
/// passes a non-ASCII byte through unchanged, so it never silently diverges from
/// CPython: for a pure-ASCII `s` the result is char-exact, and for any non-ASCII
/// `s` it traps rather than returning a wrong (un-folded) string. Because every
/// surviving byte is 1-byte ASCII, byte length == code-point length == the result
/// length, so `len` / `Concat` / equality / a str RETURN compose uniformly.
///
/// One byte-parallel pass: `$__alloc(8 + slen)`, store the i32 BYTE-count header
/// (= `slen`, unchanged by case flipping), then for each payload byte: trap if
/// `>= 0x80`, else conditionally add/subtract `0x20` when it is in the flipped
/// range, and store it. A zero-length `s` writes no payload (the loop guard is
/// `i < slen`) and returns an empty heap string.
const STR_UPPER_LOWER_HELPER: &str = "\
  ;; PMAT-1185 __wasm_str_upper_lower(s, up) = Python s.upper() (up=1) / s.lower()
  ;; (up=0) — a NEW heap string with every ASCII letter case-flipped. ASCII-only:
  ;; a byte >= 0x80 (non-ASCII code point) TRAPS (unreachable), never a silent
  ;; un-folded pass-through, so the result is char-exact for ASCII or aborts.
  (func $__wasm_str_upper_lower (param $s i32) (param $up i32) (result i32)
    (local $slen i32)
    (local $dst i32)
    (local $i i32)
    (local $c i32)
    ;; slen = byte length of s (unchanged by case flipping — all survivors ASCII).
    local.get $s
    i32.load
    local.set $slen
    ;; dst = alloc(8 + slen) ; store the i32 header = slen.
    local.get $slen
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $slen
    i32.store
    ;; for i in 0..slen: c = s[8+i]; trap if non-ASCII; case-flip; dst[8+i] = c.
    i32.const 0
    local.set $i
    block $done
      loop $loop
        local.get $i
        local.get $slen
        i32.ge_s
        br_if $done
        ;; c = load byte s[8 + i]
        local.get $s
        i32.const 8
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        local.set $c
        ;; non-ASCII byte -> honest trap (needs a Unicode case table we don't carry)
        local.get $c
        i32.const 0x80
        i32.ge_u
        if
          unreachable
        end
        ;; up != 0 (upper): 'a'(0x61)..'z'(0x7a) -> c - 0x20
        local.get $up
        if
          local.get $c
          i32.const 0x61
          i32.ge_u
          local.get $c
          i32.const 0x7a
          i32.le_u
          i32.and
          if
            local.get $c
            i32.const 0x20
            i32.sub
            local.set $c
          end
        else
          ;; up == 0 (lower): 'A'(0x41)..'Z'(0x5a) -> c + 0x20
          local.get $c
          i32.const 0x41
          i32.ge_u
          local.get $c
          i32.const 0x5a
          i32.le_u
          i32.and
          if
            local.get $c
            i32.const 0x20
            i32.add
            local.set $c
          end
        end
        ;; dst[8 + i] = c
        local.get $dst
        i32.const 8
        i32.add
        local.get $i
        i32.add
        local.get $c
        i32.store8
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $loop
      end
    end
    local.get $dst
  )
";

/// PMAT-1060: the int-to-string helper (allocating — rides the `needs_heap`
/// gate, calls `$__alloc`).
///
///   * `$__wasm_int_to_str(n) -> i32` — Python `str(n)` / `repr(n)` over an
///     `int`: materialise a NEW heap string holding the DECIMAL ASCII form of
///     the i64 `n`. Sign-aware, and it works in the UNSIGNED magnitude so
///     `i64::MIN` (`-9223372036854775808`) converts without an overflow on the
///     negation (`0 - MIN` wraps to the correct u64 bit pattern, then the digit
///     extraction uses `i64.div_u` / `i64.rem_u`).
///
/// Two passes: (1) count the decimal digits of the magnitude (at least 1, so
/// `0` → `"0"`); (2) `$__alloc(8 + digits + sign)`, store the i32 BYTE-count
/// header, write a leading `-` when negative, then fill the digits from the
/// least-significant end backward. Every digit is ASCII (1 byte), so the
/// byte-count header equals the Python CHAR count — the result composes
/// uniformly with `len` / `Concat` / equality / a str RETURN like any other
/// heap string.
const INT_TO_STR_HELPER: &str = "\
  ;; PMAT-1060 __wasm_int_to_str(n) = a NEW heap string with the decimal ASCII
  ;; form of the i64 n (Python str(int)). Unsigned-magnitude so i64::MIN is
  ;; exact; all digits are 1-byte ASCII so the byte header == the char count.
  (func $__wasm_int_to_str (param $n i64) (result i32)
    (local $neg i32)
    (local $mag i64)
    (local $t i64)
    (local $count i32)
    (local $total i32)
    (local $p i32)
    (local $w i32)
    ;; neg = n < 0
    local.get $n
    i64.const 0
    i64.lt_s
    local.set $neg
    ;; mag = neg ? (0 - n) : n   [wrapping sub → correct u64 magnitude for MIN]
    local.get $neg
    if (result i64)
      i64.const 0
      local.get $n
      i64.sub
    else
      local.get $n
    end
    local.set $mag
    ;; count = number of decimal digits of mag (at least 1)
    local.get $mag
    local.set $t
    i32.const 1
    local.set $count
    block $cnt_done
      loop $cnt
        local.get $t
        i64.const 10
        i64.div_u
        local.set $t
        local.get $t
        i64.eqz
        br_if $cnt_done
        local.get $count
        i32.const 1
        i32.add
        local.set $count
        br $cnt
      end
    end
    ;; total = count + neg
    local.get $count
    local.get $neg
    i32.add
    local.set $total
    ;; p = __alloc(8 + total); store the byte-count header
    i32.const 8
    local.get $total
    i32.add
    call $__alloc
    local.set $p
    local.get $p
    local.get $total
    i32.store
    ;; if negative, write '-' (45) at p+8
    local.get $neg
    if
      local.get $p
      i32.const 8
      i32.add
      i32.const 45
      i32.store8
    end
    ;; fill digits backward from w = p + 8 + total - 1
    local.get $p
    i32.const 8
    i32.add
    local.get $total
    i32.add
    i32.const 1
    i32.sub
    local.set $w
    loop $fill
      local.get $w
      i32.const 48
      local.get $mag
      i64.const 10
      i64.rem_u
      i32.wrap_i64
      i32.add
      i32.store8
      local.get $mag
      i64.const 10
      i64.div_u
      local.set $mag
      local.get $w
      i32.const 1
      i32.sub
      local.set $w
      local.get $mag
      i64.eqz
      i32.eqz
      br_if $fill
    end
    local.get $p
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

    // set: update an existing key in place, else append at count. PMAT-999: on
    // overflow (count >= capacity) the region GROWS — bump-alloc a 2x region,
    // memory.copy the header + entries, and RETURN the (possibly relocated)
    // base-pointer so the caller updates its local. A genuine out-of-memory
    // (the one 64-KiB page exhausted) still traps via $__alloc's later store.
    writeln!(
        out,
        "  ;; __wasm_dict_set_{s}(p, key, val) -> p': update-or-insert (d[key] = val)."
    )
    .expect("write");
    writeln!(
        out,
        "  ;; GROWS (2x realloc + copy) when count >= capacity; returns the base"
    )
    .expect("write");
    writeln!(
        out,
        "  ;; pointer (unchanged unless it grew), which the caller local.set's."
    )
    .expect("write");
    writeln!(
        out,
        "  (func $__wasm_dict_set_{s} (param $p i32) (param $k {kparam}) (param $v i64) (result i32)"
    )
    .expect("write");
    writeln!(out, "    (local $np i32)").expect("write");
    emit_dict_scan_prologue(&mut out);
    emit_dict_key_compare(&mut out, kind);
    writeln!(out, "        if").expect("write");
    writeln!(out, "          local.get $ea").expect("write");
    writeln!(out, "          local.get $v").expect("write");
    writeln!(out, "          i64.store offset={DICT_VAL_OFFSET}").expect("write");
    // in-place update: the base-pointer did not move; return it.
    writeln!(out, "          local.get $p").expect("write");
    writeln!(out, "          return").expect("write");
    writeln!(out, "        end").expect("write");
    emit_dict_scan_epilogue(&mut out);
    // not found → GROW if at capacity, then append at slot n.
    writeln!(out, "    local.get $n").expect("write");
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    i32.load offset={DICT_CAP_OFFSET}").expect("write");
    writeln!(out, "    i32.ge_s").expect("write");
    writeln!(out, "    if").expect("write");
    // np = __alloc(header + (cap*2)*ENTRY); doubling amortises the copies.
    writeln!(out, "      i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "      local.get $p").expect("write");
    writeln!(out, "      i32.load offset={DICT_CAP_OFFSET}").expect("write");
    writeln!(out, "      i32.const 2").expect("write");
    writeln!(out, "      i32.mul").expect("write"); // new_cap = cap*2
    writeln!(out, "      i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "      i32.mul").expect("write"); // new_cap*ENTRY
    writeln!(out, "      i32.add").expect("write"); // header + new_cap*ENTRY
    writeln!(out, "      call $__alloc").expect("write");
    writeln!(out, "      local.set $np").expect("write");
    // memory.copy(np, p, header + cap*ENTRY): header (count+cap) + all entries.
    writeln!(out, "      local.get $np").expect("write");
    writeln!(out, "      local.get $p").expect("write");
    writeln!(out, "      i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "      local.get $p").expect("write");
    writeln!(out, "      i32.load offset={DICT_CAP_OFFSET}").expect("write");
    writeln!(out, "      i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "      i32.mul").expect("write"); // cap*ENTRY
    writeln!(out, "      i32.add").expect("write"); // header + cap*ENTRY
    writeln!(out, "      memory.copy").expect("write");
    // np.capacity = cap*2 (overwrite the copied old cap).
    writeln!(out, "      local.get $np").expect("write");
    writeln!(out, "      local.get $p").expect("write");
    writeln!(out, "      i32.load offset={DICT_CAP_OFFSET}").expect("write");
    writeln!(out, "      i32.const 2").expect("write");
    writeln!(out, "      i32.mul").expect("write");
    writeln!(out, "      i32.store offset={DICT_CAP_OFFSET}").expect("write");
    // p = np (WASM params are reassignable locals).
    writeln!(out, "      local.get $np").expect("write");
    writeln!(out, "      local.set $p").expect("write");
    writeln!(out, "    end").expect("write");
    // $ea = p + LIST_ELEMS_OFFSET + n*DICT_ENTRY_SIZE  (p may have grown).
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
    // return the (possibly grown) base-pointer.
    writeln!(out, "    local.get $p").expect("write");
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

// ─── PMAT-1030: for-loop desugar ────────────────────────────────────

/// PMAT-1030: desugar every [`Stmt::ForEach`] — Python `for x in xs` over a
/// `list[scalar]` name and `for ch in s` over a string
/// ([`Expr::StrChars`]) — into the Let+While+`Index`/`StrCharAt` subset the
/// rest of this backend already lowers, BEFORE any scan or emit pass runs.
/// Every later pass (literal collection, heap/str-eq detection, locals
/// collection, per-function emission) then sees only statements it already
/// handles, so the bounds guards, typed element loads, and the PMAT-1028
/// str-local machinery are reused verbatim rather than re-implemented.
///
/// `for var in <iterable>: body` becomes
///
/// ```text
/// let __wasm_fe_s_<k>: str = <iterable>   ;; str case only, skipped when
///                                         ;; the iterable is already a name
/// let __wasm_fe_i_<k>: int = 0
/// while __wasm_fe_i_<k> < len(<src>):
///     let var: <elem_ty> = <src>[__wasm_fe_i_<k>]
///     __wasm_fe_i_<k> = __wasm_fe_i_<k> + 1
///     <body>
/// ```
///
/// The index increment sits BEFORE the body deliberately: `continue` lowers
/// to `br $cont` (straight back to the `while` condition), so an
/// increment-last desugar would skip it and loop forever. With the
/// increment first, `continue` sees the already-advanced index and `break`
/// simply exits — both CPython-exact. `len(<src>)` is re-read each
/// iteration (a header load for lists; a PMAT-1032 `$__wasm_str_charlen`
/// walk for strings — O(bytes) per iteration, correctness over speed;
/// mutation during iteration is refused upstream, PMAT-1013). The synthetic
/// `__wasm_fe_*_<k>` names follow the `__wasm_*` scratch-local convention
/// (`IDX_SCRATCH`); `<k>` is a per-function counter so nested and
/// sequential loops never share an index slot.
///
/// Honest scope: the loop VAR is a WAT function-scoped local, so a
/// post-loop read sees the last element (CPython-exact for a non-empty
/// iterable); the empty-iterable + post-loop-read degenerate case yields
/// the zero default where Python raises `NameError` — the same PMAT-838
/// tradeoff the Rust lane documents. Dict iteration (`over_keys`), in-place
/// element mutation (`mutate_elems`), and non-name/non-str iterables
/// (list literals, `enumerate`/`zip` — those are `ForEachPair`) refuse with
/// precise messages.
fn desugar_module_foreach(module: &Module) -> Result<Module, BackendError> {
    let mut m = module.clone();
    for item in &mut m.items {
        match item {
            Item::Function(f) => {
                let mut next = 0usize;
                f.body.stmts = desugar_foreach_stmts(&f.body.stmts, &mut next)?;
            }
            Item::Struct { methods, .. } => {
                for f in methods {
                    let mut next = 0usize;
                    f.body.stmts = desugar_foreach_stmts(&f.body.stmts, &mut next)?;
                }
            }
            _ => {}
        }
    }
    Ok(m)
}

/// The recursive statement rewrite behind [`desugar_module_foreach`].
/// `next` numbers the synthetic locals within one function.
fn desugar_foreach_stmts(stmts: &[Stmt], next: &mut usize) -> Result<Vec<Stmt>, BackendError> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            Stmt::ForEach {
                var,
                iter,
                elem_ty,
                body,
                over_keys,
                dict_guard,
                mutate_elems,
            } => {
                if *over_keys || dict_guard.is_some() {
                    return Err(unsupported(
                        "for-loop over a dict — dict iteration is not in the \
                         WASM subset (HashMap-order + heap-relocation \
                         semantics are unresolved; iterate a list or str)",
                    ));
                }
                if *mutate_elems {
                    return Err(unsupported(
                        "for-loop mutating its elements in place — the WASM \
                         list subset holds scalars (copies), so an in-place \
                         element mutation cannot propagate; refused honestly",
                    ));
                }
                let body = desugar_foreach_stmts(body, next)?;
                let k = *next;
                *next += 1;
                let idx = format!("__wasm_fe_i_{k}");
                // Resolve the iteration SOURCE to a name + the per-element
                // read expression.
                let (setup, src): (Option<Stmt>, String) = match iter {
                    // `for ch in s` — the frontend wraps a str iterable in
                    // StrChars. Reuse the name when the operand is already
                    // one; otherwise bind the string ONCE into a synthetic
                    // PMAT-1028 str local (a literal, concat, or proven
                    // str-returning call all lower there).
                    Expr::StrChars { string } => match string.as_ref() {
                        Expr::Ident(n) => (None, n.clone()),
                        other => {
                            let s_name = format!("__wasm_fe_s_{k}");
                            (
                                Some(Stmt::Let {
                                    name: s_name.clone(),
                                    ty: Type::Str,
                                    value: other.clone(),
                                    mutable: false,
                                }),
                                s_name,
                            )
                        }
                    },
                    // `for x in xs` — a named list[scalar]; `len(xs)` and
                    // `xs[i]` refuse precisely downstream if it is not.
                    Expr::Ident(n) => (None, n.clone()),
                    // PMAT-1033: `for x in [1, 2, 3]` — bind the literal ONCE
                    // into a synthetic list local (the PMAT-1028 str-literal
                    // pattern, list edition); the loop then iterates the
                    // name. An unsupported element type refuses at the
                    // local's registration, honestly.
                    lit @ Expr::ListLit(_) => {
                        let l_name = format!("__wasm_fe_l_{k}");
                        (
                            Some(Stmt::Let {
                                name: l_name.clone(),
                                ty: Type::List(Box::new(elem_ty.clone())),
                                value: lit.clone(),
                                mutable: false,
                            }),
                            l_name,
                        )
                    }
                    other => {
                        return Err(unsupported(&format!(
                            "for-loop over {} — the WASM subset iterates a \
                             named `list[scalar]`, a list literal, or a string \
                             (name/literal/concat/str-returning call); bind \
                             the iterable to a name first",
                            expr_kind(other)
                        )));
                    }
                };
                let elem_read = if matches!(iter, Expr::StrChars { .. }) {
                    Expr::StrCharAt {
                        string: Box::new(Expr::Ident(src.clone())),
                        index: Box::new(Expr::Ident(idx.clone())),
                    }
                } else {
                    Expr::Index {
                        collection: Box::new(Expr::Ident(src.clone())),
                        index: Box::new(Expr::Ident(idx.clone())),
                    }
                };
                let mut wbody = Vec::with_capacity(body.len() + 2);
                wbody.push(Stmt::Let {
                    name: var.clone(),
                    ty: elem_ty.clone(),
                    value: elem_read,
                    mutable: false,
                });
                wbody.push(Stmt::Assign {
                    name: idx.clone(),
                    value: Expr::BinOp {
                        op: BinOp::Add,
                        lhs: Box::new(Expr::Ident(idx.clone())),
                        rhs: Box::new(Expr::LitInt(1)),
                    },
                });
                wbody.extend(body);
                if let Some(setup) = setup {
                    out.push(setup);
                }
                out.push(Stmt::Let {
                    name: idx.clone(),
                    ty: Type::I64,
                    value: Expr::LitInt(0),
                    mutable: true,
                });
                out.push(Stmt::While {
                    cond: Expr::BinOp {
                        op: BinOp::Lt,
                        lhs: Box::new(Expr::Ident(idx.clone())),
                        rhs: Box::new(Expr::Len(Box::new(Expr::Ident(src)))),
                    },
                    body: wbody,
                });
            }
            Stmt::While { cond, body } => out.push(Stmt::While {
                cond: cond.clone(),
                body: desugar_foreach_stmts(body, next)?,
            }),
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => out.push(Stmt::If {
                cond: cond.clone(),
                then_body: desugar_foreach_stmts(then_body, next)?,
                else_body: desugar_foreach_stmts(else_body, next)?,
            }),
            other => out.push(other.clone()),
        }
    }
    Ok(out)
}

// ─── PMAT-1164: f-string / format int-operand auto-stringification ──────────
//
// A Python f-string / `str.format` / `%`-format with literal text AROUND an
// interpolated value lowers (in the shared frontend) to a left-nested
// `Expr::Concat` whose operands are the literal chunks (`LitStr`) interleaved
// with the interpolated expressions — e.g. `f"count={n}"` becomes
// `Concat(LitStr("count="), n)`. For a STRING interpolation the operand is
// already string-valued and the existing `emit_concat` handles it; for an INT
// interpolation the operand is a raw `i64` expression. The Rust backend relies
// on `format!`'s `Display` there, but the WASM lane has no `Display` — it must
// materialise the decimal string explicitly.
//
// This pre-pass rewrites every int-valued `Concat` operand into an explicit
// `str(int)` (`Expr::ToStr { of_float: false }`) BEFORE any gate scan or
// emission runs, so:
//   * `module_needs_int_to_str` sees the injected `ToStr` (it recurses through
//     `Concat` and matches `ToStr { of_float: false }`) and emits the
//     `$__wasm_int_to_str` helper — no called-but-undeclared gate hole, and
//   * `emit_concat` sees an ordinary string-valued operand (a `ToStr`, which it
//     already lowers via `emit_int_to_str`).
//
// It is FAIL-SAFE: an operand shape the classifier does not POSITIVELY type as
// int is left untouched, so a genuinely-string operand is never mis-wrapped and
// an unsupported operand still refuses honestly at `emit_str_expr`. A bare
// single-interpolation f-string (`f"{n}"`, no surrounding literal → a raw
// `StrFormat`, not a `Concat`) and any format spec (`f"{x:>5}"`) stay refused.
fn normalize_module_fstring_ints(module: &Module) -> Module {
    let mut m = module.clone();
    for item in &mut m.items {
        match item {
            Item::Function(f) => normalize_fn_fstring_ints(f),
            Item::Struct { methods, .. } => {
                for f in methods {
                    normalize_fn_fstring_ints(f);
                }
            }
            _ => {}
        }
    }
    m
}

fn normalize_fn_fstring_ints(f: &mut Function) {
    // name → declared type, from params + every `let` in the body (a shadowing
    // `let` overrides the param binding). Only the int (`I64`/`CLong`) entries
    // matter to the classifier, but collecting all keeps the check one lookup.
    let mut ctx: HashMap<String, Type> = HashMap::new();
    for p in &f.params {
        ctx.insert(p.name.clone(), p.ty.clone());
    }
    collect_let_types(&f.body.stmts, &mut ctx);
    normalize_stmts_fstring_ints(&mut f.body.stmts, &ctx);
    normalize_expr_fstring_ints(&mut f.body.trailing_return, &ctx);
}

/// Collect `let`-binding names → types, recursing into `if`/`while` bodies.
fn collect_let_types(stmts: &[Stmt], ctx: &mut HashMap<String, Type>) {
    for s in stmts {
        match s {
            Stmt::Let { name, ty, .. } => {
                ctx.insert(name.clone(), ty.clone());
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_let_types(then_body, ctx);
                collect_let_types(else_body, ctx);
            }
            Stmt::While { body, .. } => collect_let_types(body, ctx),
            _ => {}
        }
    }
}

/// Walk the statement Exprs that can carry a string-building `Concat` (a
/// returned value, a `let`/reassignment value, an `if`/`while` guard) and
/// normalise their int operands. Recurse into nested `if`/`while` bodies.
fn normalize_stmts_fstring_ints(stmts: &mut [Stmt], ctx: &HashMap<String, Type>) {
    for s in stmts {
        match s {
            Stmt::Return(e) => normalize_expr_fstring_ints(e, ctx),
            Stmt::Let { value, .. } | Stmt::Assign { value, .. } => {
                normalize_expr_fstring_ints(value, ctx)
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                normalize_expr_fstring_ints(cond, ctx);
                normalize_stmts_fstring_ints(then_body, ctx);
                normalize_stmts_fstring_ints(else_body, ctx);
            }
            Stmt::While { cond, body } => {
                normalize_expr_fstring_ints(cond, ctx);
                normalize_stmts_fstring_ints(body, ctx);
            }
            _ => {}
        }
    }
}

/// Recurse into the Expr containers where a `Concat` can appear as a string
/// value, rewriting its int operands. Not exhaustive over every Expr variant —
/// an un-recursed nesting just leaves the operand raw, which refuses honestly
/// downstream (never a crash).
fn normalize_expr_fstring_ints(e: &mut Expr, ctx: &HashMap<String, Type>) {
    match e {
        Expr::Concat { lhs, rhs } => {
            normalize_concat_operand_fstring_int(lhs, ctx);
            normalize_concat_operand_fstring_int(rhs, ctx);
        }
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            normalize_expr_fstring_ints(cond, ctx);
            normalize_expr_fstring_ints(then_expr, ctx);
            normalize_expr_fstring_ints(else_expr, ctx);
        }
        Expr::Call { args, .. } => {
            for a in args {
                normalize_expr_fstring_ints(a, ctx);
            }
        }
        Expr::MethodCall { obj, args, .. } => {
            normalize_expr_fstring_ints(obj, ctx);
            for a in args {
                normalize_expr_fstring_ints(a, ctx);
            }
        }
        // PMAT-1166: a `str.format` / `%`-format template reaches the WASM lane
        // as a raw `Expr::StrFormat { fmt, args }` (the frontend only folds an
        // f-string's literal text into a `Concat`; `.format(...)` / `% (...)`
        // stay templated). Fold the SIMPLE bare-`{}` case into the same
        // left-nested `Concat` the lane already lowers, then re-run this pass so
        // int operands auto-stringify (PMAT-1164). A spec / positional /
        // named field, or an arg-count mismatch, leaves the `StrFormat` intact
        // for the honest refusal at `emit_str_expr`.
        Expr::StrFormat { fmt, args } => {
            if let Some(folded) = try_fold_strformat_to_concat(fmt, args) {
                *e = folded;
                // PMAT-1167: the fold yields either a left-nested `Concat` (there
                // WAS surrounding literal text, e.g. `f"n={n}"`) OR — for a BARE
                // single interpolation with no literal chunks (`f"{n}"` /
                // `"{}".format(n)`) — the lone argument itself. Route through the
                // concat-OPERAND normaliser (not `normalize_expr_fstring_ints`) so
                // BOTH shapes are covered: a `Concat` recurses into its operands
                // (int→str per PMAT-1164, unchanged), AND a bare int-valued argument
                // in string-RETURN position auto-stringifies via `str(int)` instead
                // of landing as a raw `i64` that refuses at `emit_str_expr`. A bare
                // STRING argument (`f"{s}"`) is not int-typed, so it is left as-is
                // and emits directly (already worked). The injected top-level
                // `ToStr{of_float:false}` is seen by `expr_has_int_to_str` /
                // `expr_has_heap_op` (both scan `Stmt::Return`), so the int→str
                // helper + `$__alloc` + `(memory)` stay gated — no gate hole.
                normalize_concat_operand_fstring_int(e, ctx);
            }
        }
        // PMAT-1167: a BARE single-interpolation int f-string `f"{n}"` does NOT
        // reach the lane as a `StrFormat` — the frontend's
        // `stringify_lone_fstring_field` wraps the lone int field in
        // `Expr::FormatSpec { value, rust_spec: "", of_float: false }` (rendered
        // `format!("{:}", n)`), an EMPTY spec that is semantically `str(int)`.
        // Rewrite that empty-spec, non-float, int-valued case into the `ToStr`
        // the WASM lane already emits — so `f"{n}"` / `f"{a+b}"` / `f"{len(s)}"`
        // stringify instead of refusing. A NON-empty spec (`f"{x:>5}"` —
        // width/precision/alignment) or a float field (`of_float`, whose Python
        // vs Rust `Display` disagree) is left intact for the honest refusal at
        // `emit_str_expr`. The injected `ToStr{of_float:false}` is seen by the
        // return/let-scanning `expr_has_int_to_str` / `expr_has_heap_op` gates,
        // so `$__wasm_int_to_str` + `$__alloc` + `(memory)` stay declared — no
        // gate hole.
        Expr::FormatSpec {
            value,
            rust_spec,
            of_float,
        } => {
            normalize_expr_fstring_ints(value, ctx);
            let foldable = rust_spec.is_empty() && !*of_float && concat_operand_is_int(value, ctx);
            if foldable {
                let taken = std::mem::replace(value.as_mut(), Expr::Unit);
                *e = Expr::ToStr {
                    value: Box::new(taken),
                    of_float: false,
                };
            }
        }
        _ => {}
    }
}

/// Rewrite one `Concat` operand: a nested `Concat` recurses (its own operands
/// get rewritten); an int-valued operand is wrapped in `str(int)`; anything
/// else recurses in case it nests a `Concat` (e.g. a str-valued `IfExpr`
/// branch). A str-valued operand is left untouched — the classifier only
/// matches positively-int shapes, so it is never mis-wrapped.
fn normalize_concat_operand_fstring_int(op: &mut Expr, ctx: &HashMap<String, Type>) {
    if matches!(op, Expr::Concat { .. }) {
        normalize_expr_fstring_ints(op, ctx);
        return;
    }
    if concat_operand_is_int(op, ctx) {
        let taken = std::mem::replace(op, Expr::Unit);
        *op = Expr::ToStr {
            value: Box::new(taken),
            of_float: false,
        };
        return;
    }
    normalize_expr_fstring_ints(op, ctx);
}

/// `true` if `e` is a value the WASM lane emits as an `i64` (so `str(e)` is the
/// supported int→decimal materialisation). Conservative: only positively-int
/// shapes — an int literal, an `I64`/`CLong`-typed name, a `len(...)` /
/// `ord(...)` (both yield an int count / code point), an integer-arithmetic
/// `BinOp` (whose result is `i64` by construction; comparison / logical ops are
/// excluded — those are `bool`, which the frontend has already lowered to a
/// str-valued `IfExpr` in a format position), or an int-valued UNARY op
/// (`UnOp::Neg` / `UnOp::BitNot`) over an operand that itself classifies as int
/// (PMAT-1169 — `f"{-n}"` / `f"{~n}"`). Everything else returns `false` and is
/// left for `emit_str_expr` (str-valued → handled; otherwise refused).
fn concat_operand_is_int(e: &Expr, ctx: &HashMap<String, Type>) -> bool {
    match e {
        Expr::LitInt(_) => true,
        Expr::Ident(n) => matches!(ctx.get(n), Some(Type::I64) | Some(Type::CLong)),
        Expr::Len(_) | Expr::Ord { .. } => true,
        Expr::BinOp { op, .. } => concat_binop_is_int(*op),
        // PMAT-1169: an int-valued UNARY operator over an int operand is itself
        // int — Python `-x` (`UnOp::Neg`) and `~x` (`UnOp::BitNot`) are both
        // `I64 -> I64`, so `f"{-n}"` / `f"{~n}"` / `f"{-(a+b)}"` are `str(int)`
        // (the `$__wasm_int_to_str` helper is sign-aware, PMAT-1060, so the
        // leading `-` is rendered — `-42` -> "-42", `~5` == `-6` -> "-6"). The
        // operand MUST itself classify as int (recurse), so `-3.0` (a float
        // `LitFloat` operand) and any non-int shape stay unwrapped -> the honest
        // refusal at `emit_str_expr`. `UnOp::Not` (logical `not x`, `Bool ->
        // Bool`) is EXCLUDED — a bool in a format position is not int (the
        // frontend already lowers it to a str-valued `IfExpr`), so it must not
        // be mis-wrapped in `str(int)`.
        Expr::UnOp { op, operand } => {
            matches!(op, UnOp::Neg | UnOp::BitNot) && concat_operand_is_int(operand, ctx)
        }
        // The int-VALUED string methods the WASM lane already emits as `i64`:
        // `len(s)` (`CharCount`, PMAT-1148) and the search family (`find` /
        // `rfind` / `count` / `index` / `rindex`). Str-, bool-, and list-valued
        // methods (upper / startswith / split …) are NOT int and stay unwrapped.
        Expr::StrMethod { op, .. } => matches!(
            op,
            StrMethodOp::CharCount
                | StrMethodOp::Find
                | StrMethodOp::Rfind
                | StrMethodOp::Count
                | StrMethodOp::StrIndex
                | StrMethodOp::RIndex
        ),
        _ => false,
    }
}

/// The integer-arithmetic `BinOp`s whose result is `i64` (mirrors meta-HIR's
/// own `binop_is_int_arith`, kept local so the meta-HIR API stays unchanged).
fn concat_binop_is_int(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::FloorDiv
            | BinOp::Mod
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::Pow
    )
}

/// PMAT-1166: fold a bare-`{}` `str.format` / `%`-format template into the
/// left-nested `Expr::Concat` the WASM string lane already lowers, or return
/// `None` (leaving the `StrFormat` for the honest refusal) for anything the
/// simple fold does not cover.
///
/// The `fmt` field is the frontend's RUST-format template (`"{}={}"` for
/// `"%s=%d" % (a, b)`; `"{}-{}"` for `"{}-{}".format(x, y)`), with `{{` / `}}`
/// escapes for literal braces. The fold accepts ONLY automatic `{}` fields:
///   * `{{` → literal `{`, `}}` → literal `}`;
///   * `{}` → the next positional argument (in encounter order);
///   * ANY other `{…}` field (`{:spec}`, `{0}`, `{name}`) → `None`, so a
///     width / precision / alignment / positional / keyword template still
///     refuses honestly (their formatting is not modelled on the WASM lane).
///
/// The number of `{}` fields must EXACTLY equal `args.len()` (Rust `format!`
/// already requires this; the check guards a malformed template).
///
/// The produced `Concat` interleaves the literal chunks (`Expr::LitStr`) with
/// the argument expressions; the caller re-runs `normalize_expr_fstring_ints`
/// so an int arg auto-stringifies via `str(int)` (PMAT-1164) and `emit_concat`
/// lowers the rest. A non-str/int arg still refuses honestly at
/// `emit_str_expr` (never a crash).
fn try_fold_strformat_to_concat(fmt: &str, args: &[Expr]) -> Option<Expr> {
    // Interleaved pieces, in template order.
    enum Piece {
        Lit(String),
        Arg(usize),
    }
    let mut pieces: Vec<Piece> = Vec::new();
    let mut lit = String::new();
    let mut next_arg = 0usize;
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => match chars.peek() {
                Some('{') => {
                    chars.next();
                    lit.push('{');
                }
                // `{}` (automatic field) OR `{:}` (automatic field, EMPTY format
                // spec). PMAT-1167: a BARE f-string interpolation `f"{n}"` reaches
                // the WASM lane as `StrFormat { fmt: "{:}", args: [n] }` (the
                // frontend renders it `format!("{:}", n)`) — the colon introduces an
                // empty spec that is semantically identical to `{}`. Accept both;
                // treat as the next positional arg with no formatting.
                Some('}') | Some(':') => {
                    // consume the `}` or `:` we peeked.
                    let had_colon = chars.next() == Some(':');
                    if had_colon {
                        // an EMPTY spec (`{:}`) is fine — the next char must be `}`.
                        // A NON-empty spec (`{:>5}`, `{:03d}`) is real width /
                        // precision / alignment formatting the WASM lane does not
                        // model → refuse the fold (fall through to `emit_str_expr`'s
                        // honest refusal).
                        if chars.next() != Some('}') {
                            return None;
                        }
                    }
                    if !lit.is_empty() {
                        pieces.push(Piece::Lit(std::mem::take(&mut lit)));
                    }
                    pieces.push(Piece::Arg(next_arg));
                    next_arg += 1;
                }
                // `{0}`, `{name}` — a positional / named field. Refuse the fold.
                _ => return None,
            },
            '}' => {
                // A lone `}` is only valid as the `}}` escape in a Rust template.
                if chars.peek() == Some(&'}') {
                    chars.next();
                    lit.push('}');
                } else {
                    return None;
                }
            }
            other => lit.push(other),
        }
    }
    if !lit.is_empty() {
        pieces.push(Piece::Lit(lit));
    }
    if next_arg != args.len() {
        return None;
    }
    // Fold the pieces left-associatively (the frontend's `Concat` shape).
    let mut it = pieces.into_iter().map(|p| match p {
        Piece::Lit(s) => Expr::LitStr(s),
        Piece::Arg(i) => args[i].clone(),
    });
    let mut acc = it.next()?;
    for e in it {
        acc = Expr::Concat {
            lhs: Box::new(acc),
            rhs: Box::new(e),
        };
    }
    Some(acc)
}

// ─── WAT emission ───────────────────────────────────────────────────

/// Emit a full `(module …)` for `module`. Only [`Item::Function`]s are
/// emitted; struct definitions contribute layout + methods (PMAT-996/1023);
/// any other item kind is refused (no enum/const in the scalar/control
/// subset).
pub fn emit_module(module: &Module) -> Result<String, BackendError> {
    // PMAT-1030: rewrite `for x in xs` / `for ch in s` into the
    // Let+While+Index/StrCharAt subset FIRST, so every scan pass below and
    // the per-function emission see only statements they already handle.
    let desugared = desugar_module_foreach(module)?;
    // PMAT-1164: auto-stringify int operands of a format `Concat` (`f"n={n}"`)
    // into `str(int)` BEFORE the gate scans + emission, so the int→str helper
    // gates and `emit_concat` see ordinary string-valued operands.
    let normalized = normalize_module_fstring_ints(&desugared);
    let module = &normalized;
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
    // PMAT-1023: the module's method + associated-fn signature registries
    // (struct.method / Struct::__init__ → param/result WAT types), built once
    // so call sites can type their args + result.
    let (methods, assoc_fns) = build_method_registry(module)?;
    // PMAT-1024: the module's FREE-function signature registry — statement-
    // position plain calls (`bump(c)`, the mutating-helper idiom the
    // reference-semantics frontend passes through) need the callee's return
    // shape to know whether a result must be dropped.
    let mod_fns = build_module_fn_registry(module)?;
    // PMAT-1028: the str-returning callables, so a call may feed a string
    // position (`s: str = build(5)`, a concat operand) with proven str-ness.
    let str_rets = build_str_returners(module);
    let regs = Registries {
        literals: &literals,
        structs: &structs,
        methods: &methods,
        assoc_fns: &assoc_fns,
        mod_fns: &mod_fns,
        str_rets: &str_rets,
    };
    let needs_str_eq = module_needs_str_eq(module, &str_rets) || dict_str_keys;
    // PMAT-1059: a string ORDERING compare (`<`/`<=`/`>`/`>=`) reads the str
    // bytes via `$__wasm_str_cmp` — it needs linear memory declared (to load
    // the payload) but NOT the bump allocator (it allocates nothing).
    let needs_str_cmp = module_needs_str_cmp(module, &str_rets);
    // PMAT-1126: `s.startswith(p)` / `s.endswith(p)` — non-allocating byte
    // prefix/suffix compares (like `$__wasm_str_cmp`, they read memory but
    // allocate nothing). Each gates its own helper and pulls in the
    // `(memory …)` declaration its loads need (a str operand already forces it
    // via the param/literal/heap gates, but assert it here too, mirroring
    // `needs_str_cmp`).
    let needs_startswith = module_uses_str_method(module, StrMethodOp::StartsWith);
    let needs_endswith = module_uses_str_method(module, StrMethodOp::EndsWith);
    // PMAT-1127: `x in s` over strings (`Expr::StrContains`) — a non-allocating
    // byte SUBSTRING search (`$__wasm_str_contains`), the sliding generalisation
    // of `$__wasm_str_startswith`. Reads the two str payloads, allocates nothing.
    let needs_contains = module_uses_str_contains(module);
    // PMAT-1128: `s.count(p)` over strings (`Expr::StrMethod`, op `Count`) — a
    // non-allocating byte OCCURRENCE count (`$__wasm_str_count`), the counting
    // generalisation of `$__wasm_str_contains`. Reads the two str payloads,
    // allocates nothing; like the prefix/suffix ops it forces the `(memory …)`.
    let needs_count = module_uses_str_method(module, StrMethodOp::Count);
    // PMAT-1136: `s.find(p)` over strings (`Expr::StrMethod`, op `Find`) — a
    // non-allocating byte SEARCH returning the CODE-POINT index of the first
    // match (`$__wasm_str_find`), the index-returning sibling of
    // `$__wasm_str_contains`. Reads the two str payloads, allocates nothing.
    let needs_find = module_uses_str_method(module, StrMethodOp::Find);
    // PMAT-1143: `s.rfind(p)` over strings (`Expr::StrMethod`, op `Rfind`) — the
    // reverse-scan sibling of `find`, returning the CODE-POINT index of the LAST
    // match (`$__wasm_str_rfind`), or -1. Reads the two str payloads, allocates
    // nothing (its empty-needle answer calls `$__wasm_str_charlen`, co-emitted by
    // `module_touches_str`).
    let needs_rfind = module_uses_str_method(module, StrMethodOp::Rfind);
    // PMAT-1144: `s.index(p)` / `s.rindex(p)` over strings (`Expr::StrMethod`, ops
    // `StrIndex` / `RIndex`) — the TRAPPING siblings of `find` / `rfind`: identical
    // on a present needle, but an ABSENT needle is Python `ValueError`, lowered to a
    // WASM `unreachable`. Each wrapper (`$__wasm_str_index` / `$__wasm_str_rindex`)
    // calls the matching search helper, so `index` FORCES `$__wasm_str_find` and
    // `rindex` FORCES `$__wasm_str_rfind` (folded into `needs_find`/`needs_rfind`
    // below). Reads the two str payloads, allocates nothing.
    let needs_index = module_uses_str_method(module, StrMethodOp::StrIndex);
    let needs_rindex = module_uses_str_method(module, StrMethodOp::RIndex);
    // `$__wasm_str_index` wraps `$__wasm_str_find`, `$__wasm_str_rindex` wraps
    // `$__wasm_str_rfind` — so pull the search helper in whenever its trapping
    // sibling is used, even if the module never calls the plain search directly.
    let needs_find = needs_find || needs_index;
    let needs_rfind = needs_rfind || needs_rindex;
    // PMAT-1153: `s.removeprefix(p)` / `s.removesuffix(p)` (`Expr::StrMethod`, ops
    // `RemovePrefix` / `RemoveSuffix`) — allocating string-RETURNING ops that copy
    // the retained byte range into a fresh heap string. Each wraps the matching
    // byte prefix/suffix test: `$__wasm_str_removeprefix` calls
    // `$__wasm_str_startswith` and `$__wasm_str_removesuffix` calls
    // `$__wasm_str_endswith`, so `removeprefix` FORCES `$__wasm_str_startswith`
    // and `removesuffix` FORCES `$__wasm_str_endswith` (folded below), just as
    // `index`/`rindex` force `find`/`rfind`.
    let needs_removeprefix = module_uses_str_method(module, StrMethodOp::RemovePrefix);
    let needs_removesuffix = module_uses_str_method(module, StrMethodOp::RemoveSuffix);
    let needs_startswith = needs_startswith || needs_removeprefix;
    let needs_endswith = needs_endswith || needs_removesuffix;
    // PMAT-1159: `s.replace(old, new)` (`Expr::StrMethod`, op `Replace`) — an
    // allocating string-RETURNING op (a fresh heap string with every
    // non-overlapping `old` replaced by `new`). Rides `needs_heap` (set via
    // `expr_has_heap_op`, like removeprefix/removesuffix). Its empty-`old` regime
    // calls `$__wasm_str_charlen` / `$__wasm_str_char_width` (co-emitted for any
    // str-touching module), so — unlike removeprefix (which FORCES a predicate) —
    // it forces no extra helper: the char family is already present.
    // PMAT-1161: the 3-arg `.replace(old, new, count)` (op `ReplaceN`) shares the
    // SAME `$__wasm_str_replace` helper (count -1 = the 2-arg replace-all), so
    // either op present must emit it.
    let needs_replace = module_uses_str_method(module, StrMethodOp::Replace)
        || module_uses_str_method(module, StrMethodOp::ReplaceN);
    // PMAT-1173: `s.zfill(width)` (`Expr::StrMethod`, op `ZFill`) — an allocating
    // string-RETURNING op (a fresh heap string left-padded with `'0'` to `width`
    // code points). Rides `needs_heap` (set via `expr_has_heap_op`, like
    // removeprefix/replace). Its width math calls `$__wasm_str_charlen`
    // (co-emitted for any str-touching module via `module_touches_str`), so — like
    // replace — it forces no extra helper beyond the always-present char family.
    let needs_zfill = module_uses_str_method(module, StrMethodOp::ZFill);
    // PMAT-1185: `s.upper()` / `s.lower()` (`Expr::StrMethod`, ops `Upper` /
    // `Lower`) — allocating string-RETURNING ops (a fresh heap string with every
    // ASCII letter case-flipped). Both share the single `$__wasm_str_upper_lower`
    // helper (an `up` i32 flag selects the direction), so either op present must
    // emit it. Rides `needs_heap` (set via `expr_has_heap_op`, like
    // removeprefix/replace/zfill). Byte-parallel (no charlen math — all survivors
    // are 1-byte ASCII, non-ASCII bytes trap), so it forces no extra helper.
    let needs_upper_lower = module_uses_str_method(module, StrMethodOp::Upper)
        || module_uses_str_method(module, StrMethodOp::Lower);
    if module_uses_list_param(module)
        || needs_heap
        || !literals.is_empty()
        || needs_str_eq
        || needs_str_cmp
        || needs_startswith
        || needs_endswith
        || needs_contains
        || needs_count
        || needs_find
        || needs_rfind
    {
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
    // PMAT-1059: emit the string-ordering helper once, when any function
    // compares two strings with `<`/`<=`/`>`/`>=`. Byte-wise lexicographic
    // compare == Python code-point order for UTF-8 — reads memory, allocates
    // nothing (independent of the bump-heap gate).
    if needs_str_cmp {
        out.push_str(STR_CMP_HELPER);
    }
    // PMAT-1126: emit the string PREFIX/SUFFIX helpers once, when any function
    // calls `s.startswith(p)` / `s.endswith(p)`. Byte prefix/suffix compare ==
    // code-point compare for valid UTF-8 — reads memory, allocates nothing
    // (independent of the bump-heap gate, like `$__wasm_str_cmp`). Each is gated
    // separately so a module using only one carries no dead helper.
    if needs_startswith {
        out.push_str(STR_STARTSWITH_HELPER);
    }
    if needs_endswith {
        out.push_str(STR_ENDSWITH_HELPER);
    }
    // PMAT-1127: emit the string SUBSTRING-search helper once, when any function
    // uses `x in s` over strings (`Expr::StrContains`). A byte substring search
    // == a code-point substring search for valid UTF-8 — reads memory, allocates
    // nothing (independent of the bump-heap gate, like `$__wasm_str_startswith`).
    if needs_contains {
        out.push_str(STR_CONTAINS_HELPER);
    }
    // PMAT-1128: emit the string OCCURRENCE-count helper once, when any function
    // uses `s.count(p)` over strings (`Expr::StrMethod`, op `Count`). Same byte
    // slide as `$__wasm_str_contains` but counts non-overlapping matches; the
    // empty-needle case calls `$__wasm_str_charlen` (emitted below via
    // `module_touches_str`, which a `StrMethod` always sets). Reads memory,
    // allocates nothing (a Python int, not a new string).
    if needs_count {
        out.push_str(STR_COUNT_HELPER);
    }
    // PMAT-1136: emit the string FIND helper once, when any function uses
    // `s.find(p)` over strings (`Expr::StrMethod`, op `Find`). Same byte slide as
    // `$__wasm_str_contains` but returns the CODE-POINT index of the first match
    // (or -1) — the ONE search op that must convert the byte offset to a char
    // index (Python find is char-indexed). The conversion counts non-continuation
    // bytes in `h[0..start]`; the empty-needle answer is 0 (no char walk). Reads
    // memory, allocates nothing (a Python int, not a new string).
    if needs_find {
        out.push_str(STR_FIND_HELPER);
    }
    // PMAT-1163: emit the start-bounded FIND helper once, when any function uses
    // the 2-arg `s.find(p, start)` form (`Expr::StrMethod`, op `Find`, 2 args).
    // The start-bounded generalisation of `$__wasm_str_find`: same byte slide +
    // byte→char-index conversion, begun at the byte offset of the start-th code
    // point, with Python's negative/overflow start clamp and empty-needle-at-start
    // semantics. Its empty-needle/clamp path calls `$__wasm_str_charlen` (emitted
    // below via `module_touches_str`, which a `StrMethod` always sets). Gated on an
    // ACTUAL 2-arg find so a plain 1-arg `.find(p)` module carries no dead helper.
    if module_uses_str_find2(module) {
        out.push_str(STR_FIND_FROM_HELPER);
    }
    // PMAT-1143: emit the string RFIND helper once, when any function uses
    // `s.rfind(p)` over strings (`Expr::StrMethod`, op `Rfind`). The reverse-scan
    // sibling of `$__wasm_str_find`: same byte match + byte→char-index conversion,
    // but the outer slide runs from the last candidate offset DOWN to 0 (first
    // match = last occurrence). The empty-needle answer is charlen(h) — the code
    // point length (Python `"abc".rfind("")` == 3), calling `$__wasm_str_charlen`
    // (emitted below via `module_touches_str`, which a `StrMethod` always sets).
    // Reads memory, allocates nothing (a Python int, not a new string).
    if needs_rfind {
        out.push_str(STR_RFIND_HELPER);
    }
    // PMAT-1165: emit the start-bounded RFIND helper once, when any function uses
    // the 2-arg `s.rfind(p, start)` form (`Expr::StrMethod`, op `Rfind`, 2 args).
    // The reverse-scan sibling of `$__wasm_str_find_from`: find-from's start clamp +
    // code-point→byte decode, but the candidate slide runs DOWN from the last
    // fitting offset to the start byte (first match = rightmost ≥ start). Its
    // empty-needle answer is `charlen(h)` (found at the END); its clamp/charlen path
    // calls `$__wasm_str_charlen` (emitted below via `module_touches_str`, which a
    // `StrMethod` always sets). Gated on an ACTUAL 2-arg rfind so a plain 1-arg
    // `.rfind(p)` module carries no dead helper.
    if module_uses_str_rfind2(module) {
        out.push_str(STR_RFIND_FROM_HELPER);
    }
    // PMAT-1144: emit the string INDEX / RINDEX helpers once, when any function
    // uses `s.index(p)` / `s.rindex(p)` (`Expr::StrMethod`, ops `StrIndex` /
    // `RIndex`). Each is a thin TRAPPING wrapper over the matching search helper
    // (`$__wasm_str_find` / `$__wasm_str_rfind`, already emitted above via the
    // `needs_find |= needs_index` / `needs_rfind |= needs_rindex` fold): identical
    // to find/rfind on a present needle, but an absent needle (search → -1) is
    // Python `ValueError`, lowered to `unreachable`. Reads memory (through the
    // wrapped helper), allocates nothing.
    if needs_index {
        out.push_str(STR_INDEX_HELPER);
    }
    if needs_rindex {
        out.push_str(STR_RINDEX_HELPER);
    }
    // PMAT-1032: emit the CHAR-semantics helper family once, when any function
    // touches strings — Python-visible len/index/ord/chr are CHAR-oriented
    // (code points) over the byte-oriented UTF-8 ABI. The non-allocating half
    // (charlen/width/char_addr/ord_at) suffices for read-only str modules; the
    // allocating half (char_at/chr) calls `$__alloc` so it rides the heap gate
    // (any materialising `s[i]`/`chr(n)` already sets `needs_heap`).
    if module_touches_str(module) {
        out.push_str(STR_CHAR_HELPERS);
        if needs_heap {
            out.push_str(STR_CHAR_ALLOC_HELPERS);
            // PMAT-1058: the string-SLICE helper (`s[lo:hi]`) — allocating, so
            // it rides `needs_heap`; gated further on an actual slice use so a
            // heap-string module with no slice carries no dead helper.
            if module_uses_str_slice(module) {
                out.push_str(STR_SLICE_HELPER);
            }
        }
    }
    // PMAT-1060: emit the int→str helper once, when any function uses
    // `str(int)`. Allocating (calls `$__alloc`), so it rides `needs_heap` —
    // a `str(int)` sets the heap gate via `expr_has_heap_op`. Independent of
    // `module_touches_str` (an int→decimal-string module need not otherwise
    // touch a str name), so it is emitted on its own gate here.
    if needs_heap && module_needs_int_to_str(module) {
        out.push_str(INT_TO_STR_HELPER);
    }
    // PMAT-1142: emit the string-REPEAT helper once, when any function uses
    // `s * n` over a str (`Expr::Repeat { of_str: true }`). Allocating (calls
    // `$__alloc` + `memory.copy`), so it rides `needs_heap` — a str repeat sets
    // the heap gate via `expr_has_heap_op`. Gated further on an actual str-repeat
    // use so a heap-string module with no repeat carries no dead helper.
    if needs_heap && module_uses_str_repeat(module) {
        out.push_str(STR_REPEAT_HELPER);
    }
    // PMAT-1153: emit the string REMOVEPREFIX / REMOVESUFFIX helpers once, when any
    // function uses `s.removeprefix(p)` / `s.removesuffix(p)` (`Expr::StrMethod`, ops
    // `RemovePrefix` / `RemoveSuffix`). Allocating (call `$__alloc` + `memory.copy`),
    // so each rides `needs_heap` — a remove op sets the heap gate via
    // `expr_has_heap_op`. Each also calls the matching byte prefix/suffix test
    // (`$__wasm_str_startswith` / `$__wasm_str_endswith`), already emitted above via
    // the `needs_startswith |= needs_removeprefix` / `needs_endswith |=
    // needs_removesuffix` fold. Gated on an actual use so an unrelated heap-string
    // module carries no dead helper.
    if needs_heap && needs_removeprefix {
        out.push_str(STR_REMOVEPREFIX_HELPER);
    }
    if needs_heap && needs_removesuffix {
        out.push_str(STR_REMOVESUFFIX_HELPER);
    }
    // PMAT-1159: emit the string REPLACE helper once, when any function uses
    // `s.replace(old, new)` (`Expr::StrMethod`, op `Replace`). Allocating (calls
    // `$__alloc` + `memory.copy`), so it rides `needs_heap` — a replace sets the
    // heap gate via `expr_has_heap_op`. Its empty-`old` regime uses the char
    // helpers (`$__wasm_str_charlen` / `$__wasm_str_char_width`) emitted above via
    // `module_touches_str`. Gated on an actual use so an unrelated heap-string
    // module carries no dead helper.
    if needs_heap && needs_replace {
        out.push_str(STR_REPLACE_HELPER);
    }
    // PMAT-1173: emit the string ZFILL helper once, when any function uses
    // `s.zfill(width)` (`Expr::StrMethod`, op `ZFill`). Allocating (calls
    // `$__alloc` + `memory.fill` + `memory.copy`), so it rides `needs_heap` — a
    // zfill sets the heap gate via `expr_has_heap_op`. Its width math uses
    // `$__wasm_str_charlen` (emitted above via `module_touches_str`). Gated on an
    // actual use so an unrelated heap-string module carries no dead helper.
    if needs_heap && needs_zfill {
        out.push_str(STR_ZFILL_HELPER);
    }
    // PMAT-1185: emit the string UPPER/LOWER helper once, when any function uses
    // `s.upper()` or `s.lower()` (`Expr::StrMethod`, ops `Upper` / `Lower`). The
    // single `$__wasm_str_upper_lower` helper serves both (an `up` i32 flag picks
    // the direction). Allocating (calls `$__alloc` + `i32.store8`), so it rides
    // `needs_heap` — an upper/lower sets the heap gate via `expr_has_heap_op`.
    // Byte-parallel (no char helper — a non-ASCII byte traps rather than folding).
    // Gated on an actual use so an unrelated heap-string module carries no dead
    // helper.
    if needs_heap && needs_upper_lower {
        out.push_str(STR_UPPER_LOWER_HELPER);
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
                let f_wat = emit_function(f, &regs, &f.name)?;
                out.push_str(&f_wat);
            }
            Item::Const { name, .. } => {
                return Err(unsupported(&format!(
                    "module-level const `{name}` (only scalar/control functions are in the WASM subset)"
                )));
            }
            // PMAT-996 (slice 4): a struct DEFINITION emits no WAT of its own —
            // it is pure layout (recorded in `structs`). PMAT-1023: its METHODS
            // now DO emit, each as an ordinary WAT function `$<Struct>.<method>`
            // whose `self` receiver is the instance's i32 base-pointer (the
            // struct-param path emit_function already handles). A self-mutating
            // method stores through that pointer, so the mutation is visible to
            // every binding of the record — Python reference semantics, native.
            Item::Struct {
                name, methods: ms, ..
            } => {
                for m in ms {
                    let has_self = m.params.first().is_some_and(
                        |p| matches!((&p.name, &p.ty), (n, Type::Struct(s)) if n == "self" && s == name),
                    );
                    // Instance methods mangle `<Struct>.<method>`; associated
                    // fns (the desugared explicit `__init__`) mangle
                    // `<Struct>::<method>` — the EXACT callee string their
                    // `Expr::Call` sites carry, so the generic `call $<callee>`
                    // emission resolves without a rename map. Both `.` and `:`
                    // are legal WAT id characters.
                    let wat_name = if has_self {
                        format!("{name}.{}", m.name)
                    } else {
                        format!("{name}::{}", m.name)
                    };
                    let m_wat = emit_function(m, &regs, &wat_name)?;
                    out.push_str(&m_wat);
                }
            }
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
    module_functions(module).any(|f| {
        f.params
            .iter()
            // PMAT-996: a struct param is also an i32 base-pointer into linear
            // memory, so it likewise needs the `(memory …)` declaration.
            // (PMAT-1023: this scan covers struct METHODS too — their `self`
            // receiver is a struct param, so any module with a method gets
            // the `(memory …)` its field loads/stores need.)
            .any(|p| matches!(p.ty, Type::List(_) | Type::Str | Type::Struct(_)))
    })
}

/// PMAT-1023: every lowered function in `module` — the free `Item::Function`s
/// AND each `Item::Struct`'s methods (which emit as ordinary WAT functions
/// named `$<Struct>.<method>`). Module-level scans (literals, heap ops, dict
/// kinds, str-eq) MUST use this so a construct inside a METHOD body pulls in
/// the same helpers/memory it would in a free function.
fn module_functions(module: &Module) -> impl Iterator<Item = &Function> {
    module.items.iter().flat_map(|item| match item {
        Item::Function(f) => std::slice::from_ref(f).iter(),
        Item::Struct { methods, .. } => methods.iter(),
        _ => [].iter(),
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
        || module_functions(module)
            .any(|f| matches!(f.return_type, Type::Str) || block_has_heap_op(&f.body))
}

/// PMAT-1032: `true` when any function in `module` TOUCHES strings — a `str`
/// param/return/local, or any string-carrying expression (`LitStr`, `Concat`,
/// `Chr`, `StrCharAt`, `StrChars`, `Ord`, `StrMethod`). Gates the emission of
/// the CHAR-semantics helper family ([`STR_CHAR_HELPERS`]): every VALID
/// str-touching module also declares the `(memory …)` (a str name is a param
/// — memory via the param scan — or a local fed by a literal/heap-op/
/// str-returning call, each of which pulls the memory in), so the helpers'
/// loads always validate. An INVALID use (e.g. `ord` of an int name) refuses
/// during function emission and the module is never returned.
fn module_touches_str(module: &Module) -> bool {
    module_functions(module).any(|f| {
        matches!(f.return_type, Type::Str)
            || f.params.iter().any(|p| matches!(p.ty, Type::Str))
            || block_touches_str(&f.body)
    })
}

fn block_touches_str(block: &Block) -> bool {
    block.stmts.iter().any(stmt_touches_str) || expr_touches_str(&block.trailing_return)
}

fn stmt_touches_str(s: &Stmt) -> bool {
    match s {
        Stmt::Let { ty, value, .. } => matches!(ty, Type::Str) || expr_touches_str(value),
        Stmt::Assign { value, .. } => expr_touches_str(value),
        Stmt::Return(e) => expr_touches_str(e),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_touches_str(cond)
                || then_body.iter().any(stmt_touches_str)
                || else_body.iter().any(stmt_touches_str)
        }
        Stmt::While { cond, body } => expr_touches_str(cond) || body.iter().any(stmt_touches_str),
        Stmt::FieldAssign { value, .. } => expr_touches_str(value),
        // PMAT-1151: write-side gate holes (see `stmt_has_int_to_str`) — a
        // DictSet/SetAdd key/value/elem (`d[chr(n)] = v` over a str-keyed dict)
        // or an index can be the SOLE str-touching site in a function, gating
        // the char-helper family + `(memory)`; scan them so the helpers' loads
        // always validate.
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(expr_touches_str) || expr_touches_str(value)
        }
        Stmt::DictSet { key, value, .. } => expr_touches_str(key) || expr_touches_str(value),
        Stmt::SetAdd { elem, .. } => expr_touches_str(elem),
        Stmt::SideEffectCall { call } => expr_touches_str(call),
        _ => false,
    }
}

fn expr_touches_str(e: &Expr) -> bool {
    match e {
        Expr::LitStr(_)
        | Expr::Concat { .. }
        | Expr::Chr { .. }
        | Expr::StrCharAt { .. }
        | Expr::StrChars { .. }
        | Expr::Ord { .. }
        | Expr::StrMethod { .. }
        // PMAT-1060: `str(int)` yields a heap string, so a `len`/`s[i]` over
        // it needs the CHAR-semantics helpers gated by this predicate.
        | Expr::ToStr { .. } => true,
        Expr::FieldAccess { obj, .. } => expr_touches_str(obj),
        Expr::BinOp { lhs, rhs, .. } | Expr::FloatBinOp { lhs, rhs, .. } => {
            expr_touches_str(lhs) || expr_touches_str(rhs)
        }
        Expr::UnOp { operand, .. } => expr_touches_str(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => expr_touches_str(cond) || expr_touches_str(then_expr) || expr_touches_str(else_expr),
        Expr::Call { args, .. } => args.iter().any(expr_touches_str),
        Expr::Index { collection, index } => {
            expr_touches_str(collection) || expr_touches_str(index)
        }
        Expr::Len(c) => expr_touches_str(c),
        Expr::MethodCall { obj, args, .. } => {
            expr_touches_str(obj) || args.iter().any(expr_touches_str)
        }
        Expr::Slice { collection, .. } => expr_touches_str(collection),
        // PMAT-1127: `x in s` — a heap-string operand (Concat / Chr / s[i] /
        // slice / str(int)) needs the CHAR-semantics helper family gated here.
        Expr::StrContains { haystack, needle } => {
            expr_touches_str(haystack) || expr_touches_str(needle)
        }
        // PMAT-1142: a STRING repeat `s * n` yields a heap string, so `len` /
        // `s[i]` over it needs the CHAR-semantics helper family gated here.
        Expr::Repeat { seq, n, of_str } => {
            *of_str || expr_touches_str(seq) || expr_touches_str(n)
        }
        // PMAT-1150: a str-keyed dict/set op (`d["hello"[1:4]]`, `s[0:2] in q`)
        // materialises a string in its KEY/elem — so the CHAR-semantics helper
        // family (and, via `module_uses_str_slice` beneath this gate, the SLICE
        // helper) must be emitted. This is the OUTER gate hole behind the
        // per-helper scan holes: with no str NAME to short-circuit
        // `module_touches_str`, a literal-collection slice used as a dict/set key
        // (`"hello"[1:4]`) left `$__wasm_str_slice` undeclared even after the
        // per-helper scans gained their DictGet arms. Recurse into both operands.
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            expr_touches_str(dict) || expr_touches_str(key)
        }
        Expr::SetContains { set, elem } => expr_touches_str(set) || expr_touches_str(elem),
        _ => false,
    }
}

/// PMAT-1058: `true` when any function in `module` uses a supported string
/// slice `s[lo:hi]` (`Expr::Slice { of_str: true, step: None }`) — the gate for
/// emitting [`STR_SLICE_HELPER`]. A stepped string slice or a list slice is
/// refused at lowering (not counted here), so the helper is emitted only for a
/// module that actually materialises a substring.
fn module_uses_str_slice(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_str_slice(&f.body))
}

fn block_has_str_slice(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_str_slice) || expr_has_str_slice(&block.trailing_return)
}

fn stmt_has_str_slice(s: &Stmt) -> bool {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => {
            expr_has_str_slice(value)
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_has_str_slice(cond)
                || then_body.iter().any(stmt_has_str_slice)
                || else_body.iter().any(stmt_has_str_slice)
        }
        Stmt::While { cond, body } => {
            expr_has_str_slice(cond) || body.iter().any(stmt_has_str_slice)
        }
        Stmt::FieldAssign { value, .. } => expr_has_str_slice(value),
        // PMAT-1151: write-side gate holes (see `stmt_has_int_to_str`) — an
        // index (`xs[len(s[1:4])] = v`) and a DictSet/SetAdd key/elem
        // (`d[s[1:4]] = v`, `q.add(s[1:4])`) can host the slice helper.
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(expr_has_str_slice) || expr_has_str_slice(value)
        }
        Stmt::DictSet { key, value, .. } => expr_has_str_slice(key) || expr_has_str_slice(value),
        Stmt::SetAdd { elem, .. } => expr_has_str_slice(elem),
        Stmt::SideEffectCall { call } => expr_has_str_slice(call),
        _ => false,
    }
}

fn expr_has_str_slice(e: &Expr) -> bool {
    match e {
        // this node IS a supported string slice — no need to recurse further.
        Expr::Slice {
            of_str: true,
            step: None,
            ..
        } => true,
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            expr_has_str_slice(collection)
                || lo.as_deref().is_some_and(expr_has_str_slice)
                || hi.as_deref().is_some_and(expr_has_str_slice)
        }
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => expr_has_str_slice(lhs) || expr_has_str_slice(rhs),
        Expr::UnOp { operand, .. } => expr_has_str_slice(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_str_slice(cond)
                || expr_has_str_slice(then_expr)
                || expr_has_str_slice(else_expr)
        }
        Expr::Call { args, .. } => args.iter().any(expr_has_str_slice),
        Expr::MethodCall { obj, args, .. } => {
            expr_has_str_slice(obj) || args.iter().any(expr_has_str_slice)
        }
        // PMAT-1150: a str slice can be the KEY of a str-keyed dict subscript
        // (`d[s[1:4]]`), whose key is lowered via `emit_str_expr` — the SOLE
        // slice site in a module. The MISS here was a latent gate hole (the
        // sibling walkers `expr_has_int_to_str` / `expr_uses_str_method` /
        // `_str_repeat` / `_str_contains` all carry this Index arm): a
        // subscript-hosted `s[1:4]` this gate skips leaves `$__wasm_str_slice`
        // UNDECLARED (a hard wat2wasm failure). Recurse into both operands.
        Expr::Index { collection, index } => {
            expr_has_str_slice(collection) || expr_has_str_slice(index)
        }
        // PMAT-1148: `len(<str temporary>)` synthesises `StrMethod{CharCount,
        // recv}` (Python len is a CODE-POINT count), so a slice inside the recv
        // (`len(s[1:4] + t)`) reaches the slice helper only by recursing here —
        // mirrors the `expr_has_heap_op` / `expr_has_str_repeat` StrMethod arms.
        Expr::StrMethod { recv, args, .. } => {
            expr_has_str_slice(recv) || args.iter().any(expr_has_str_slice)
        }
        Expr::Len(c) => expr_has_str_slice(c),
        Expr::Ord { value } | Expr::Chr { value } => expr_has_str_slice(value),
        Expr::StrCharAt { string, index } => {
            expr_has_str_slice(string) || expr_has_str_slice(index)
        }
        Expr::FieldAccess { obj, .. } => expr_has_str_slice(obj),
        // PMAT-1150: `str(...)` / `repr(...)` wraps its arg in `Expr::ToStr`,
        // whose value can host a slice reached only by an intermediate node
        // (`str(len(s[1:4]))` is ToStr→Len→Slice) — the ToStr arm was the other
        // half of this gate hole, so the walk stopped at ToStr and never reached
        // the Len→Slice below it. Recurse into the wrapped value.
        Expr::ToStr { value, .. } => expr_has_str_slice(value),
        // PMAT-1127: `x in s` — a slice operand (`s[1:4] in t`) needs the slice
        // helper gated here.
        Expr::StrContains { haystack, needle } => {
            expr_has_str_slice(haystack) || expr_has_str_slice(needle)
        }
        // PMAT-1142: a repeat operand (`s[1:4] * n`) needs the slice helper.
        Expr::Repeat { seq, n, .. } => expr_has_str_slice(seq) || expr_has_str_slice(n),
        // PMAT-1150: a str slice can be the computed KEY of a str-keyed dict/set
        // op (`d[s[1:3]]`, `s[0:2] in q`). The subscript lowers to `DictGet` /
        // `DictContains` / `SetContains` (NOT `Expr::Index`), whose key/elem is
        // materialised via `emit_str_expr` → `call $__wasm_str_slice`. No walker
        // recursed into these dict/set nodes, so the slice helper stayed
        // UNDECLARED (a hard wat2wasm failure). Recurse into both operands.
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            expr_has_str_slice(dict) || expr_has_str_slice(key)
        }
        Expr::SetContains { set, elem } => expr_has_str_slice(set) || expr_has_str_slice(elem),
        _ => false,
    }
}

/// PMAT-1060: `true` when any function in `module` uses `str(int)` /
/// `repr(int)` (`Expr::ToStr { of_float: false }`) — the gate for emitting
/// [`INT_TO_STR_HELPER`]. `str(float)` (`of_float: true`) is refused at
/// lowering, not counted here, so the helper is emitted only for a module that
/// actually materialises a decimal int string.
fn module_needs_int_to_str(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_int_to_str(&f.body))
}

fn block_has_int_to_str(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_int_to_str) || expr_has_int_to_str(&block.trailing_return)
}

fn stmt_has_int_to_str(s: &Stmt) -> bool {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => {
            expr_has_int_to_str(value)
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_has_int_to_str(cond)
                || then_body.iter().any(stmt_has_int_to_str)
                || else_body.iter().any(stmt_has_int_to_str)
        }
        Stmt::While { cond, body } => {
            expr_has_int_to_str(cond) || body.iter().any(stmt_has_int_to_str)
        }
        Stmt::FieldAssign { value, .. } => expr_has_int_to_str(value),
        // PMAT-1151: the INDEX of `xs[i] = v` (not only the value) can host a
        // str-materialising subexpr (`xs[len(str(n))] = v` — the index is
        // `len(str(n))`, whose `str(n)` emits `call $__wasm_int_to_str`) — scan
        // the indices too.
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(expr_has_int_to_str) || expr_has_int_to_str(value)
        }
        // PMAT-1151: `d[k] = v` (`Stmt::DictSet`) and `s.add(e)` (`Stmt::SetAdd`)
        // are LOWERED by the WASM lane — `emit_dict_set` / `emit_set_add` route a
        // str key/elem through `emit_dict_key` → `emit_str_expr`, whose `ToStr`
        // arm emits `call $__wasm_int_to_str`. These two STATEMENT forms are the
        // WRITE-side siblings of PMAT-1150's DictGet/DictContains/SetContains
        // read-side EXPR arms, and NO helper-gate stmt-walker scanned them: a
        // str-keyed `d[str(n)] = 5` left `$__wasm_int_to_str` called-but-
        // UNDECLARED (a hard wat2wasm failure the value-only scan never caught).
        // Scan both the key and value, and the elem.
        Stmt::DictSet { key, value, .. } => expr_has_int_to_str(key) || expr_has_int_to_str(value),
        Stmt::SetAdd { elem, .. } => expr_has_int_to_str(elem),
        Stmt::SideEffectCall { call } => expr_has_int_to_str(call),
        _ => false,
    }
}

fn expr_has_int_to_str(e: &Expr) -> bool {
    match e {
        // this node IS a supported int→str — no need to recurse further.
        Expr::ToStr {
            of_float: false, ..
        } => true,
        // a refused str(float) still gets scanned for a nested str(int).
        Expr::ToStr { value, .. } => expr_has_int_to_str(value),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => expr_has_int_to_str(lhs) || expr_has_int_to_str(rhs),
        Expr::UnOp { operand, .. } => expr_has_int_to_str(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_int_to_str(cond)
                || expr_has_int_to_str(then_expr)
                || expr_has_int_to_str(else_expr)
        }
        Expr::Call { args, .. } => args.iter().any(expr_has_int_to_str),
        Expr::MethodCall { obj, args, .. } => {
            expr_has_int_to_str(obj) || args.iter().any(expr_has_int_to_str)
        }
        // PMAT-1148: `len(str(n))` synthesises `StrMethod{CharCount, recv:
        // ToStr}`, so the int→str helper is reached only by recursing into recv.
        Expr::StrMethod { recv, args, .. } => {
            expr_has_int_to_str(recv) || args.iter().any(expr_has_int_to_str)
        }
        Expr::Len(c) => expr_has_int_to_str(c),
        Expr::Ord { value } | Expr::Chr { value } => expr_has_int_to_str(value),
        Expr::StrCharAt { string, index } => {
            expr_has_int_to_str(string) || expr_has_int_to_str(index)
        }
        Expr::Index { collection, index } => {
            expr_has_int_to_str(collection) || expr_has_int_to_str(index)
        }
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            expr_has_int_to_str(collection)
                || lo.as_deref().is_some_and(expr_has_int_to_str)
                || hi.as_deref().is_some_and(expr_has_int_to_str)
        }
        Expr::FieldAccess { obj, .. } => expr_has_int_to_str(obj),
        // PMAT-1127: `x in s` — a `str(int)` operand (`str(n) in s`) needs the
        // int→str helper gated here.
        Expr::StrContains { haystack, needle } => {
            expr_has_int_to_str(haystack) || expr_has_int_to_str(needle)
        }
        // PMAT-1149: a `str(int)` can be the SEQ of a string repeat (`str(n) *
        // k`), the SOLE int→str site in a module. Every sibling helper walker
        // (`expr_has_str_slice` / `_str_contains` / `_str_repeat` /
        // `expr_uses_str_method`) already carries this Repeat arm; the MISS here
        // was a latent gate hole — `emit_repeat` lowers the seq via
        // `emit_str_expr`, whose `ToStr` arm emits `call $__wasm_int_to_str`, so
        // a repeat-hosted `str(n)` that this gate skips leaves the helper
        // UNDECLARED (a hard wat2wasm failure). Recurse into both operands.
        Expr::Repeat { seq, n, .. } => expr_has_int_to_str(seq) || expr_has_int_to_str(n),
        // PMAT-1150: `str(n)` can be the computed KEY of a str-keyed dict/set op
        // (`d[str(n)]`, `str(n) in q`) — lowered to `DictGet`/`DictContains`/
        // `SetContains`, whose key rides `emit_str_expr` → `call
        // $__wasm_int_to_str`. Recurse so the helper is declared.
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            expr_has_int_to_str(dict) || expr_has_int_to_str(key)
        }
        Expr::SetContains { set, elem } => expr_has_int_to_str(set) || expr_has_int_to_str(elem),
        _ => false,
    }
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
    for f in module_functions(module) {
        scan_block_dict_kinds(&f.body, &mut need_int, &mut need_str);
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
/// is a string-VALUED expression (a str-name `Ident`, a literal, a `Concat`,
/// a `Chr`, or PMAT-1028 a str-returning call) needs the content-compare
/// helper. The str-name set is computed per-function — str PARAMS plus
/// (PMAT-1028) str-annotated LET locals — so `str == str` over either kind
/// of name is detected. UNDER-detection here is a hard wat2wasm failure
/// (a `call $__wasm_str_eq` against a helper never emitted), so the scan
/// over-approximates where it lacks scope context (see [`StrEqScan`]).
fn module_needs_str_eq(module: &Module, rets: &StrReturners) -> bool {
    module_functions(module).any(|f| {
        let mut names: Vec<&str> = f
            .params
            .iter()
            .filter(|p| matches!(p.ty, Type::Str))
            .map(|p| p.name.as_str())
            .collect();
        collect_str_let_names(&f.body.stmts, &mut names);
        let scan = StrEqScan {
            names,
            rets,
            ops: &[BinOp::Eq, BinOp::NotEq],
        };
        block_has_str_eq(&f.body, &scan)
    })
}

/// PMAT-1059: `true` if any function performs a string ORDERING compare
/// (`<` / `<=` / `>` / `>=`) over string-valued operands — gates the
/// `$__wasm_str_cmp` helper. Mirrors [`module_needs_str_eq`]'s pre-scan
/// (params + let-bound str locals + str returners); the only difference is the
/// op set it hunts (ordering, not equality). A str-keyed dict does NOT pull it
/// in (its key compare is content EQUALITY via `$__wasm_str_eq`, never ordering).
fn module_needs_str_cmp(module: &Module, rets: &StrReturners) -> bool {
    module_functions(module).any(|f| {
        let mut names: Vec<&str> = f
            .params
            .iter()
            .filter(|p| matches!(p.ty, Type::Str))
            .map(|p| p.name.as_str())
            .collect();
        collect_str_let_names(&f.body.stmts, &mut names);
        let scan = StrEqScan {
            names,
            rets,
            ops: &[BinOp::Lt, BinOp::LtEq, BinOp::Gt, BinOp::GtEq],
        };
        block_has_str_eq(&f.body, &scan)
    })
}

/// PMAT-1126: `true` when any function in `module` calls the string METHOD `op`
/// — gates the matching non-allocating WAT helper (`$__wasm_str_startswith` /
/// `$__wasm_str_endswith`). A single generic walk parameterised by `op` (unlike
/// the per-concept `expr_has_*` scans), since `StartsWith`/`EndsWith` share the
/// same node shape — an `Expr::StrMethod { op, .. }`. A MISS here would emit a
/// `call` against a helper never declared (a hard wat2wasm failure), so the
/// walk recurses through every compound expression that can host the call; the
/// executed witness (which assembles via WABT) is the backstop.
fn module_uses_str_method(module: &Module, op: StrMethodOp) -> bool {
    module_functions(module).any(|f| block_uses_str_method(&f.body, op))
}

fn block_uses_str_method(block: &Block, op: StrMethodOp) -> bool {
    block.stmts.iter().any(|s| stmt_uses_str_method(s, op))
        || expr_uses_str_method(&block.trailing_return, op)
}

fn stmt_uses_str_method(s: &Stmt, op: StrMethodOp) -> bool {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => {
            expr_uses_str_method(value, op)
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_uses_str_method(cond, op)
                || then_body.iter().any(|s| stmt_uses_str_method(s, op))
                || else_body.iter().any(|s| stmt_uses_str_method(s, op))
        }
        Stmt::While { cond, body } => {
            expr_uses_str_method(cond, op) || body.iter().any(|s| stmt_uses_str_method(s, op))
        }
        Stmt::FieldAssign { value, .. } => expr_uses_str_method(value, op),
        // PMAT-1151: write-side gate holes (see `stmt_has_int_to_str`) — a
        // DictSet/SetAdd key/value/elem (`d[k] = s.count("x")`) or an index can
        // host a str-method call, keeping the gate exhaustive.
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(|i| expr_uses_str_method(i, op)) || expr_uses_str_method(value, op)
        }
        Stmt::DictSet { key, value, .. } => {
            expr_uses_str_method(key, op) || expr_uses_str_method(value, op)
        }
        Stmt::SetAdd { elem, .. } => expr_uses_str_method(elem, op),
        Stmt::SideEffectCall { call } => expr_uses_str_method(call, op),
        _ => false,
    }
}

fn expr_uses_str_method(e: &Expr, op: StrMethodOp) -> bool {
    match e {
        Expr::StrMethod {
            recv,
            op: found,
            args,
        } => {
            *found == op
                || expr_uses_str_method(recv, op)
                || args.iter().any(|a| expr_uses_str_method(a, op))
        }
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => {
            expr_uses_str_method(lhs, op) || expr_uses_str_method(rhs, op)
        }
        Expr::UnOp { operand, .. } => expr_uses_str_method(operand, op),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_uses_str_method(cond, op)
                || expr_uses_str_method(then_expr, op)
                || expr_uses_str_method(else_expr, op)
        }
        Expr::Call { args, .. } => args.iter().any(|a| expr_uses_str_method(a, op)),
        Expr::MethodCall { obj, args, .. } => {
            expr_uses_str_method(obj, op) || args.iter().any(|a| expr_uses_str_method(a, op))
        }
        Expr::Index { collection, index } => {
            expr_uses_str_method(collection, op) || expr_uses_str_method(index, op)
        }
        Expr::Len(c) => expr_uses_str_method(c, op),
        Expr::Ord { value } | Expr::Chr { value } => expr_uses_str_method(value, op),
        Expr::StrCharAt { string, index } => {
            expr_uses_str_method(string, op) || expr_uses_str_method(index, op)
        }
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            expr_uses_str_method(collection, op)
                || lo.as_deref().is_some_and(|e| expr_uses_str_method(e, op))
                || hi.as_deref().is_some_and(|e| expr_uses_str_method(e, op))
        }
        Expr::ToStr { value, .. } => expr_uses_str_method(value, op),
        Expr::FieldAccess { obj, .. } => expr_uses_str_method(obj, op),
        // PMAT-1127: `x in s` — its str operands can host a nested method call
        // (`("a" + b).startswith(c) in s` is degenerate, but recursing keeps the
        // gate exhaustive and future-proof).
        Expr::StrContains { haystack, needle } => {
            expr_uses_str_method(haystack, op) || expr_uses_str_method(needle, op)
        }
        // PMAT-1142: a repeat operand can host a nested method call
        // (`(s.count("x") ...) `) — recurse to keep the gate exhaustive.
        Expr::Repeat { seq, n, .. } => expr_uses_str_method(seq, op) || expr_uses_str_method(n, op),
        // PMAT-1150: a str-keyed dict/set op (`DictGet`/`DictContains`/
        // `SetContains`) can host a method call in its key/elem — recurse so the
        // method helper stays gated (over-approximate, exhaustive).
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            expr_uses_str_method(dict, op) || expr_uses_str_method(key, op)
        }
        Expr::SetContains { set, elem } => {
            expr_uses_str_method(set, op) || expr_uses_str_method(elem, op)
        }
        _ => false,
    }
}

/// PMAT-1127: `true` when any function in `module` uses `x in s` over strings
/// (`Expr::StrContains`) — gates [`STR_CONTAINS_HELPER`] and the `(memory …)`
/// declaration its byte loads need. A MISS here emits a `call
/// $__wasm_str_contains` against a helper never declared (a hard wat2wasm
/// failure), so the walk recurses through every compound node that can host the
/// expression; the executed WABT witness (`str_contains_witness`) is the
/// backstop. Mirrors [`module_uses_str_slice`]'s shape.
fn module_uses_str_contains(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_str_contains(&f.body))
}

fn block_has_str_contains(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_str_contains) || expr_has_str_contains(&block.trailing_return)
}

fn stmt_has_str_contains(s: &Stmt) -> bool {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => {
            expr_has_str_contains(value)
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_has_str_contains(cond)
                || then_body.iter().any(stmt_has_str_contains)
                || else_body.iter().any(stmt_has_str_contains)
        }
        Stmt::While { cond, body } => {
            expr_has_str_contains(cond) || body.iter().any(stmt_has_str_contains)
        }
        Stmt::FieldAssign { value, .. } => expr_has_str_contains(value),
        // PMAT-1151: write-side gate holes (see `stmt_has_int_to_str`) — a
        // DictSet/SetAdd key/value/elem or an index can host `x in s` (e.g.
        // `d[k] = 1 if "a" in s else 0`), keeping the gate exhaustive.
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(expr_has_str_contains) || expr_has_str_contains(value)
        }
        Stmt::DictSet { key, value, .. } => {
            expr_has_str_contains(key) || expr_has_str_contains(value)
        }
        Stmt::SetAdd { elem, .. } => expr_has_str_contains(elem),
        Stmt::SideEffectCall { call } => expr_has_str_contains(call),
        _ => false,
    }
}

fn expr_has_str_contains(e: &Expr) -> bool {
    match e {
        // this node IS the `x in s` we gate — no need to recurse further.
        Expr::StrContains { .. } => true,
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => {
            expr_has_str_contains(lhs) || expr_has_str_contains(rhs)
        }
        Expr::UnOp { operand, .. } => expr_has_str_contains(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_str_contains(cond)
                || expr_has_str_contains(then_expr)
                || expr_has_str_contains(else_expr)
        }
        Expr::Call { args, .. } => args.iter().any(expr_has_str_contains),
        Expr::MethodCall { obj, args, .. } => {
            expr_has_str_contains(obj) || args.iter().any(expr_has_str_contains)
        }
        Expr::Index { collection, index } => {
            expr_has_str_contains(collection) || expr_has_str_contains(index)
        }
        Expr::Len(c) => expr_has_str_contains(c),
        Expr::Ord { value } | Expr::Chr { value } => expr_has_str_contains(value),
        Expr::StrCharAt { string, index } => {
            expr_has_str_contains(string) || expr_has_str_contains(index)
        }
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            expr_has_str_contains(collection)
                || lo.as_deref().is_some_and(expr_has_str_contains)
                || hi.as_deref().is_some_and(expr_has_str_contains)
        }
        Expr::FieldAccess { obj, .. } => expr_has_str_contains(obj),
        Expr::ToStr { value, .. } => expr_has_str_contains(value),
        // PMAT-1142: a repeat operand can host `x in s` (`(a in b ...) `) —
        // recurse to keep the contains gate exhaustive.
        Expr::Repeat { seq, n, .. } => expr_has_str_contains(seq) || expr_has_str_contains(n),
        // PMAT-1148: `len(...)` wraps its str arg in `StrMethod{CharCount, recv}`,
        // whose recv can host `x in s` (e.g. `len("a" if x in s else "b")` via a
        // str-valued `if` cond) — recurse so the contains helper is declared.
        Expr::StrMethod { recv, args, .. } => {
            expr_has_str_contains(recv) || args.iter().any(expr_has_str_contains)
        }
        // PMAT-1150: a str-keyed dict/set op can host `x in s` in its key/elem —
        // recurse so `$__wasm_str_contains` stays gated (exhaustive).
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            expr_has_str_contains(dict) || expr_has_str_contains(key)
        }
        Expr::SetContains { set, elem } => {
            expr_has_str_contains(set) || expr_has_str_contains(elem)
        }
        _ => false,
    }
}

/// PMAT-1142: `true` when any function in `module` uses a STRING repeat `s * n`
/// (`Expr::Repeat { of_str: true }`) — the gate for emitting
/// [`STR_REPEAT_HELPER`]. A LIST repeat (`of_str: false`) is refused at lowering
/// (not counted here), so the helper is emitted only for a module that actually
/// materialises a repeated string. A MISS here would emit a `call
/// $__wasm_str_repeat` against a helper never declared (a hard wat2wasm
/// failure), so the walk recurses through every compound node that can host the
/// expression; the executed WABT witness (`str_repeat_witness`) is the backstop.
/// Mirrors [`module_uses_str_contains`]'s shape.
fn module_uses_str_repeat(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_str_repeat(&f.body))
}

fn block_has_str_repeat(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_str_repeat) || expr_has_str_repeat(&block.trailing_return)
}

fn stmt_has_str_repeat(s: &Stmt) -> bool {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => {
            expr_has_str_repeat(value)
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_has_str_repeat(cond)
                || then_body.iter().any(stmt_has_str_repeat)
                || else_body.iter().any(stmt_has_str_repeat)
        }
        Stmt::While { cond, body } => {
            expr_has_str_repeat(cond) || body.iter().any(stmt_has_str_repeat)
        }
        Stmt::FieldAssign { value, .. } => expr_has_str_repeat(value),
        // PMAT-1151: write-side gate holes (see `stmt_has_int_to_str`) — a
        // DictSet/SetAdd key/value/elem (`d[s * n] = v`) or an index can host a
        // str repeat, keeping the gate exhaustive.
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(expr_has_str_repeat) || expr_has_str_repeat(value)
        }
        Stmt::DictSet { key, value, .. } => expr_has_str_repeat(key) || expr_has_str_repeat(value),
        Stmt::SetAdd { elem, .. } => expr_has_str_repeat(elem),
        Stmt::SideEffectCall { call } => expr_has_str_repeat(call),
        _ => false,
    }
}

fn expr_has_str_repeat(e: &Expr) -> bool {
    match e {
        // this node IS a str repeat we gate — no need to recurse further.
        Expr::Repeat { of_str: true, .. } => true,
        // a LIST repeat is refused at lowering; still recurse into its operands
        // (a str repeat could be nested inside, degenerate but exhaustive).
        Expr::Repeat { seq, n, .. } => expr_has_str_repeat(seq) || expr_has_str_repeat(n),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => expr_has_str_repeat(lhs) || expr_has_str_repeat(rhs),
        Expr::UnOp { operand, .. } => expr_has_str_repeat(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_str_repeat(cond)
                || expr_has_str_repeat(then_expr)
                || expr_has_str_repeat(else_expr)
        }
        Expr::Call { args, .. } => args.iter().any(expr_has_str_repeat),
        Expr::MethodCall { obj, args, .. } => {
            expr_has_str_repeat(obj) || args.iter().any(expr_has_str_repeat)
        }
        Expr::Index { collection, index } => {
            expr_has_str_repeat(collection) || expr_has_str_repeat(index)
        }
        Expr::Len(c) => expr_has_str_repeat(c),
        Expr::Ord { value } | Expr::Chr { value } => expr_has_str_repeat(value),
        Expr::StrCharAt { string, index } => {
            expr_has_str_repeat(string) || expr_has_str_repeat(index)
        }
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            expr_has_str_repeat(collection)
                || lo.as_deref().is_some_and(expr_has_str_repeat)
                || hi.as_deref().is_some_and(expr_has_str_repeat)
        }
        Expr::StrMethod { recv, args, .. } => {
            expr_has_str_repeat(recv) || args.iter().any(expr_has_str_repeat)
        }
        Expr::StrContains { haystack, needle } => {
            expr_has_str_repeat(haystack) || expr_has_str_repeat(needle)
        }
        Expr::FieldAccess { obj, .. } => expr_has_str_repeat(obj),
        Expr::ToStr { value, .. } => expr_has_str_repeat(value),
        // PMAT-1150: a str-keyed dict/set op can host `s * n` in its key/elem
        // (`d[s * 2]`) — the subscript lowers to `DictGet`/`DictContains`/
        // `SetContains`, whose key rides `emit_str_expr` → `call
        // $__wasm_str_repeat`. Recurse so the helper is declared.
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            expr_has_str_repeat(dict) || expr_has_str_repeat(key)
        }
        Expr::SetContains { set, elem } => expr_has_str_repeat(set) || expr_has_str_repeat(elem),
        _ => false,
    }
}

/// PMAT-1163/1165: does any function use the TWO-arg form of str method `target`
/// (`Expr::StrMethod`, op `target`, `args.len() >= 2`)? Gates the start-bounded
/// helper for `find` (`$__wasm_str_find_from`) / `rfind` (`$__wasm_str_rfind_from`)
/// so a plain 1-arg `.find(p)` / `.rfind(p)` module carries no dead helper.
/// Exhaustive over the expr/stmt tree like the other str-op gate walkers
/// (`expr_has_str_repeat` &c.): a missed sub-expression would leave the helper
/// undeclared at the 2-arg call site — a hard wat2wasm failure (the recurring
/// gate-hole class). The thin `module_uses_str_find2` / `module_uses_str_rfind2`
/// wrappers pin the op so call sites read as before.
fn module_uses_str_method_2arg(module: &Module, target: StrMethodOp) -> bool {
    module_functions(module).any(|f| block_has_str_method_2arg(&f.body, target))
}

/// PMAT-1163: the 2-arg `s.find(sub, start)` gate (→ `$__wasm_str_find_from`).
fn module_uses_str_find2(module: &Module) -> bool {
    module_uses_str_method_2arg(module, StrMethodOp::Find)
}

/// PMAT-1165: the 2-arg `s.rfind(sub, start)` gate (→ `$__wasm_str_rfind_from`).
fn module_uses_str_rfind2(module: &Module) -> bool {
    module_uses_str_method_2arg(module, StrMethodOp::Rfind)
}

fn block_has_str_method_2arg(block: &Block, target: StrMethodOp) -> bool {
    block
        .stmts
        .iter()
        .any(|s| stmt_has_str_method_2arg(s, target))
        || expr_has_str_method_2arg(&block.trailing_return, target)
}

fn stmt_has_str_method_2arg(s: &Stmt, target: StrMethodOp) -> bool {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => {
            expr_has_str_method_2arg(value, target)
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_has_str_method_2arg(cond, target)
                || then_body
                    .iter()
                    .any(|s| stmt_has_str_method_2arg(s, target))
                || else_body
                    .iter()
                    .any(|s| stmt_has_str_method_2arg(s, target))
        }
        Stmt::While { cond, body } => {
            expr_has_str_method_2arg(cond, target)
                || body.iter().any(|s| stmt_has_str_method_2arg(s, target))
        }
        Stmt::FieldAssign { value, .. } => expr_has_str_method_2arg(value, target),
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(|e| expr_has_str_method_2arg(e, target))
                || expr_has_str_method_2arg(value, target)
        }
        Stmt::DictSet { key, value, .. } => {
            expr_has_str_method_2arg(key, target) || expr_has_str_method_2arg(value, target)
        }
        Stmt::SetAdd { elem, .. } => expr_has_str_method_2arg(elem, target),
        Stmt::SideEffectCall { call } => expr_has_str_method_2arg(call, target),
        _ => false,
    }
}

fn expr_has_str_method_2arg(e: &Expr, target: StrMethodOp) -> bool {
    match e {
        // this node IS a 2-arg (or 3-arg — only 2 is lowered) call of `target`;
        // otherwise recurse into the receiver + args.
        Expr::StrMethod { recv, op, args } => {
            (*op == target && args.len() >= 2)
                || expr_has_str_method_2arg(recv, target)
                || args.iter().any(|a| expr_has_str_method_2arg(a, target))
        }
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => {
            expr_has_str_method_2arg(lhs, target) || expr_has_str_method_2arg(rhs, target)
        }
        Expr::UnOp { operand, .. } => expr_has_str_method_2arg(operand, target),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_str_method_2arg(cond, target)
                || expr_has_str_method_2arg(then_expr, target)
                || expr_has_str_method_2arg(else_expr, target)
        }
        Expr::Call { args, .. } => args.iter().any(|a| expr_has_str_method_2arg(a, target)),
        Expr::MethodCall { obj, args, .. } => {
            expr_has_str_method_2arg(obj, target)
                || args.iter().any(|a| expr_has_str_method_2arg(a, target))
        }
        Expr::Index { collection, index } => {
            expr_has_str_method_2arg(collection, target) || expr_has_str_method_2arg(index, target)
        }
        Expr::Len(c) => expr_has_str_method_2arg(c, target),
        Expr::Ord { value } | Expr::Chr { value } => expr_has_str_method_2arg(value, target),
        Expr::StrCharAt { string, index } => {
            expr_has_str_method_2arg(string, target) || expr_has_str_method_2arg(index, target)
        }
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            expr_has_str_method_2arg(collection, target)
                || lo
                    .as_deref()
                    .is_some_and(|e| expr_has_str_method_2arg(e, target))
                || hi
                    .as_deref()
                    .is_some_and(|e| expr_has_str_method_2arg(e, target))
        }
        Expr::StrContains { haystack, needle } => {
            expr_has_str_method_2arg(haystack, target) || expr_has_str_method_2arg(needle, target)
        }
        Expr::FieldAccess { obj, .. } => expr_has_str_method_2arg(obj, target),
        Expr::ToStr { value, .. } => expr_has_str_method_2arg(value, target),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            expr_has_str_method_2arg(dict, target) || expr_has_str_method_2arg(key, target)
        }
        Expr::SetContains { set, elem } => {
            expr_has_str_method_2arg(set, target) || expr_has_str_method_2arg(elem, target)
        }
        Expr::Repeat { seq, n, .. } => {
            expr_has_str_method_2arg(seq, target) || expr_has_str_method_2arg(n, target)
        }
        _ => false,
    }
}

/// PMAT-1028/1059: the per-function context for a string-COMPARISON pre-scan —
/// the function's str NAMES (params + let-bound locals), the module's
/// str-RETURNING callables, and the comparison OPS to hunt for. The pre-scan
/// has no lowering scope, so its method-call check keys on the method NAME
/// alone (over-approximate; a spurious hit merely emits the helper unused,
/// while a miss would emit a call against a missing helper — a hard downstream
/// failure). PMAT-1059 generalised `ops`: `[Eq, NotEq]` gates `$__wasm_str_eq`,
/// `[Lt, LtEq, Gt, GtEq]` gates `$__wasm_str_cmp` — same walk, different op set.
struct StrEqScan<'a> {
    names: Vec<&'a str>,
    rets: &'a StrReturners,
    ops: &'a [BinOp],
}

/// PMAT-1028: collect the names of str-annotated `Let` locals anywhere in
/// `stmts` (including nested `If`/`While` bodies) into `out` — the local half
/// of the pre-scan str-name set (params are gathered by the caller).
fn collect_str_let_names<'a>(stmts: &'a [Stmt], out: &mut Vec<&'a str>) {
    for s in stmts {
        match s {
            Stmt::Let {
                name,
                ty: Type::Str,
                ..
            } => out.push(name.as_str()),
            Stmt::While { body, .. } => collect_str_let_names(body, out),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_str_let_names(then_body, out);
                collect_str_let_names(else_body, out);
            }
            _ => {}
        }
    }
}

fn block_has_str_eq(block: &Block, scan: &StrEqScan<'_>) -> bool {
    block.stmts.iter().any(|s| stmt_has_str_eq(s, scan))
        || expr_has_str_eq(&block.trailing_return, scan)
}

fn stmt_has_str_eq(s: &Stmt, scan: &StrEqScan<'_>) -> bool {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } => expr_has_str_eq(value, scan),
        Stmt::Return(e) => expr_has_str_eq(e, scan),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_has_str_eq(cond, scan)
                || then_body.iter().any(|s| stmt_has_str_eq(s, scan))
                || else_body.iter().any(|s| stmt_has_str_eq(s, scan))
        }
        Stmt::While { cond, body } => {
            expr_has_str_eq(cond, scan) || body.iter().any(|s| stmt_has_str_eq(s, scan))
        }
        // PMAT-1023: field-write values and statement method-call args.
        Stmt::FieldAssign { value, .. } => expr_has_str_eq(value, scan),
        // PMAT-1151: write-side gate holes (see `stmt_has_int_to_str`) — a
        // DictSet/SetAdd key/value/elem (`d[k] = 1 if a == b else 2`) or an
        // index can host a str comparison, keeping the eq/cmp gate exhaustive.
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(|i| expr_has_str_eq(i, scan)) || expr_has_str_eq(value, scan)
        }
        Stmt::DictSet { key, value, .. } => {
            expr_has_str_eq(key, scan) || expr_has_str_eq(value, scan)
        }
        Stmt::SetAdd { elem, .. } => expr_has_str_eq(elem, scan),
        Stmt::SideEffectCall { call } => expr_has_str_eq(call, scan),
        _ => false,
    }
}

/// `true` if `e` (or any sub-expression) is a string-valued comparison whose
/// op is in `scan.ops` — a content compare a helper backs (`$__wasm_str_eq` for
/// `Eq`/`NotEq`, `$__wasm_str_cmp` for the ordering ops, PMAT-1059). A binop
/// qualifies iff its op is in `scan.ops` and either operand is a string-valued
/// `Expr`: a `LitStr` / `Concat` / `Chr` / bare `StrCharAt` (structural), or a
/// str-name `Ident` (param or PMAT-1028 let-bound local) (looked up in `str_names`).
fn expr_has_str_eq(e: &Expr, scan: &StrEqScan<'_>) -> bool {
    match e {
        Expr::BinOp { op, lhs, rhs } => {
            (scan.ops.contains(op)
                && (expr_is_str_valued(lhs, scan) || expr_is_str_valued(rhs, scan)))
                || expr_has_str_eq(lhs, scan)
                || expr_has_str_eq(rhs, scan)
        }
        Expr::FloatBinOp { lhs, rhs, .. } | Expr::Concat { lhs, rhs } => {
            expr_has_str_eq(lhs, scan) || expr_has_str_eq(rhs, scan)
        }
        Expr::UnOp { operand, .. } => expr_has_str_eq(operand, scan),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_str_eq(cond, scan)
                || expr_has_str_eq(then_expr, scan)
                || expr_has_str_eq(else_expr, scan)
        }
        Expr::Call { args, .. } => args.iter().any(|a| expr_has_str_eq(a, scan)),
        Expr::Index { collection, index } => {
            expr_has_str_eq(collection, scan) || expr_has_str_eq(index, scan)
        }
        Expr::Len(c) => expr_has_str_eq(c, scan),
        Expr::Ord { value } | Expr::Chr { value } => expr_has_str_eq(value, scan),
        Expr::StrCharAt { string, index } => {
            expr_has_str_eq(string, scan) || expr_has_str_eq(index, scan)
        }
        // PMAT-1023: method-call args may carry a string equality.
        Expr::MethodCall { obj, args, .. } => {
            expr_has_str_eq(obj, scan) || args.iter().any(|a| expr_has_str_eq(a, scan))
        }
        // PMAT-1142: a repeat's operands may host a string equality (exhaustive).
        Expr::Repeat { seq, n, .. } => expr_has_str_eq(seq, scan) || expr_has_str_eq(n, scan),
        // PMAT-1148: `len(...)` wraps its str arg in `StrMethod{CharCount, recv}`,
        // whose recv/args may host a string equality (e.g. `len("a" if s == t
        // else "b")`) — recurse so `$__wasm_str_eq` is declared.
        Expr::StrMethod { recv, args, .. } => {
            expr_has_str_eq(recv, scan) || args.iter().any(|a| expr_has_str_eq(a, scan))
        }
        // PMAT-1150: three latent gate holes in this shared eq/cmp walker (it
        // gates BOTH `$__wasm_str_eq` for `[Eq, NotEq]` and `$__wasm_str_cmp` for
        // `[Lt, LtEq, Gt, GtEq]`). A str comparison can hide under:
        //   • `Expr::ToStr` — `str(1 if a == b else 2)` is ToStr→IfExpr→BinOp;
        //   • `Expr::Slice`  — `s[1 if a < b else 0:]` puts it in a slice bound;
        //   • `Expr::FieldAccess` — `(p1 if a == b else p2).x` in the receiver.
        // Each was terminating at `_ => false`, leaving `$__wasm_str_eq` /
        // `$__wasm_str_cmp` called-but-undeclared (a hard wat2wasm failure).
        // Recurse through all three, matching the exhaustiveness of the sibling
        // helper walkers.
        Expr::ToStr { value, .. } => expr_has_str_eq(value, scan),
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            expr_has_str_eq(collection, scan)
                || lo.as_deref().is_some_and(|e| expr_has_str_eq(e, scan))
                || hi.as_deref().is_some_and(|e| expr_has_str_eq(e, scan))
        }
        Expr::FieldAccess { obj, .. } => expr_has_str_eq(obj, scan),
        // PMAT-1150: a str-keyed dict/set op can host a string comparison inside
        // its key/elem (e.g. `d[str(1 if a == b else 2)]`) — recurse so
        // `$__wasm_str_eq` / `$__wasm_str_cmp` stays gated (exhaustive).
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            expr_has_str_eq(dict, scan) || expr_has_str_eq(key, scan)
        }
        Expr::SetContains { set, elem } => {
            expr_has_str_eq(set, scan) || expr_has_str_eq(elem, scan)
        }
        _ => false,
    }
}

/// `true` if `e` is a string-valued expression: a `LitStr` / `Concat` / `Chr`
/// / bare `StrCharAt` (structural), a str-name `Ident` (param or let-bound
/// local), or PMAT-1028 a call of a str-returning callable (free/assoc fn by
/// key; a method by NAME alone — over-approximate, see [`StrEqScan`]).
fn expr_is_str_valued(e: &Expr, scan: &StrEqScan<'_>) -> bool {
    match e {
        Expr::LitStr(_) | Expr::Concat { .. } | Expr::Chr { .. } | Expr::StrCharAt { .. } => true,
        // PMAT-1060: `str(int)` materialises a decimal-int heap string.
        Expr::ToStr {
            of_float: false, ..
        } => true,
        // PMAT-1142: `s * n` (a STRING repeat) materialises a heap string, so
        // `s * n == t` compares as strings via the content-compare helper.
        Expr::Repeat { of_str: true, .. } => true,
        Expr::Ident(name) => scan.names.contains(&name.as_str()),
        Expr::Call { callee, .. } => scan.rets.keys.iter().any(|k| k == callee),
        Expr::MethodCall { method, .. } => scan.rets.methods.iter().any(|(_, m)| m == method),
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
        // PMAT-1023: a field write's value / a statement method-call's args may
        // materialise (e.g. `c.label = "a" + s`, `c.set(Point(1, 2))`).
        Stmt::FieldAssign { value, .. } => expr_has_heap_op(value),
        // PMAT-1151: write-side gate holes (see `stmt_has_int_to_str`) — a
        // DictSet/SetAdd key/value/elem (`d["a" + s] = v`, `q.add(chr(n))`) or an
        // index can heap-construct, pulling in `$__alloc`; scan them so the
        // allocator + `(memory)` stay gated.
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(expr_has_heap_op) || expr_has_heap_op(value)
        }
        Stmt::DictSet { key, value, .. } => expr_has_heap_op(key) || expr_has_heap_op(value),
        Stmt::SetAdd { elem, .. } => expr_has_heap_op(elem),
        Stmt::SideEffectCall { call } => expr_has_heap_op(call),
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
        // PMAT-1033: a `ListLit` likewise bump-allocates its length-prefixed
        // record.
        Expr::Concat { .. }
        | Expr::Chr { .. }
        | Expr::StrCharAt { .. }
        | Expr::StructLit { .. }
        | Expr::ListLit(_) => true,
        // PMAT-1058: a string slice `s[lo:hi]` materialises a fresh heap
        // substring (calls `$__alloc`), so it pulls in the bump heap.
        Expr::Slice { of_str: true, .. } => true,
        // PMAT-1060: `str(int)` bump-allocates its decimal-ASCII string, so it
        // pulls in the allocator + `(memory)` like any materialising op.
        Expr::ToStr {
            of_float: false, ..
        } => true,
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
        // PMAT-1023: a method CALL allocates nothing at the call site (the
        // called body is scanned separately via `module_functions`), but its
        // args may (`c.set(Point(1, 2))`).
        Expr::MethodCall { obj, args, .. } => {
            expr_has_heap_op(obj) || args.iter().any(expr_has_heap_op)
        }
        // PMAT-1127: `x in s` allocates nothing itself (a bool), but a
        // heap-constructed operand (`("a" + b) in s`) pulls in the allocator.
        Expr::StrContains { haystack, needle } => {
            expr_has_heap_op(haystack) || expr_has_heap_op(needle)
        }
        // PMAT-1128: a WASM-lane string METHOD (CharCount/StartsWith/EndsWith/
        // Count) allocates nothing itself (a len/bool/int), but a heap-constructed
        // receiver or arg (`s.count("a" + b)`) pulls in the allocator — so recurse
        // into both. (Before this arm a heap method arg fell through to `_ =>
        // false`, so `$__alloc` would be emitted against an undeclared allocator —
        // a hard wat2wasm failure the param-only witnesses never triggered.)
        // PMAT-1153: `s.removeprefix(p)` / `s.removesuffix(p)` (ops `RemovePrefix` /
        // `RemoveSuffix`) THEMSELVES bump-allocate a fresh heap string (copy the
        // retained byte range), so the op ITSELF pulls in the allocator — not only
        // a heap-constructed recv/arg. A miss here would emit `$__wasm_str_remove*`
        // against an undeclared `$__alloc` (a hard wat2wasm failure), the exact
        // gate-hole class the string-op scans keep closing.
        // PMAT-1159/1161: `s.replace(old, new[, count])` (ops `Replace` /
        // `ReplaceN`) likewise allocates its (substituted) result, so each sets
        // the heap gate on the op itself — a miss would emit `$__wasm_str_replace`
        // against an undeclared `$__alloc` (hard wat2wasm fail, the gate-hole
        // class). `ReplaceN`'s count arg is an int (never heap), but the recurse
        // into `args` covers a heap-constructed old/new either way.
        // PMAT-1173: `s.zfill(width)` (op `ZFill`) THEMSELVES bump-allocate a
        // fresh padded heap string, so the op ITSELF sets the heap gate — a miss
        // would emit `$__wasm_str_zfill` against an undeclared `$__alloc` (a hard
        // wat2wasm failure, the recurring gate-hole class). Its width arg is an
        // int (never heap), but the recurse into `args` covers a heap-constructed
        // receiver either way.
        // PMAT-1185: `s.upper()` / `s.lower()` (ops `Upper` / `Lower`) LIKEWISE
        // bump-allocate their case-flipped result, so the op ITSELF sets the heap
        // gate — a miss would emit `$__wasm_str_upper_lower` against an undeclared
        // `$__alloc` (the same hard wat2wasm gate-hole). They take no args, so the
        // recurse only ever fires on a heap-constructed receiver.
        Expr::StrMethod { recv, args, op } => {
            matches!(
                op,
                StrMethodOp::RemovePrefix
                    | StrMethodOp::RemoveSuffix
                    | StrMethodOp::Replace
                    | StrMethodOp::ReplaceN
                    | StrMethodOp::ZFill
                    | StrMethodOp::Upper
                    | StrMethodOp::Lower
            ) || expr_has_heap_op(recv)
                || args.iter().any(expr_has_heap_op)
        }
        // PMAT-1142: a STRING repeat `s * n` bump-allocates its replicated
        // result (calls `$__alloc`), so it pulls in the allocator + `(memory)`
        // like any materialising op. A list repeat (`of_str: false`) is refused
        // at lowering; recurse into its operands regardless (a heap-constructed
        // `seq`/`n` still pulls in the allocator).
        Expr::Repeat { seq, n, of_str } => *of_str || expr_has_heap_op(seq) || expr_has_heap_op(n),
        // PMAT-1150: a str-keyed dict/set op allocates nothing itself, but a
        // heap-constructed key/elem (`d["a" + s]`, `s[0:2] in q`) pulls in the
        // allocator — recurse into both operands so `$__alloc` stays gated.
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            expr_has_heap_op(dict) || expr_has_heap_op(key)
        }
        Expr::SetContains { set, elem } => expr_has_heap_op(set) || expr_has_heap_op(elem),
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
    /// PMAT-986: names that are `str` base-pointers into linear memory (i32
    /// byte count @ base+0, UTF-8 bytes @ base+8). `len(s)` reads the header;
    /// `ord(s[i])` does a bounds-checked `i32.load8_u` of byte `i`. Str
    /// PARAMS land here at scope construction; PMAT-1028 adds str-annotated
    /// LET locals (`s: str = …`, registered by `collect_let_locals_stmts`) —
    /// a local holds the same length-prefixed base-pointer a param does, so
    /// every read path (len/ord/concat/eq/s[i]) is shared. Str LITERALS are
    /// separate (resolved via [`Scope::literals`]).
    str_names: Vec<String>,
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
    /// PMAT-1023: the module's method-signature registry (struct.method →
    /// non-self param WAT types + result), shared across every function so
    /// `obj.method(args)` call sites type their args and result.
    methods: &'a MethodRegistry,
    /// PMAT-1023: the module's associated-fn registry (`<Struct>::<name>` →
    /// param WAT types + result) — the frontend's desugared explicit
    /// `__init__` constructors land here, so `Counter(0)` call sites
    /// (`Expr::Call { callee: "Counter::__init__" }`) type exactly.
    assoc_fns: &'a AssocFnRegistry,
    /// PMAT-1024: the module's FREE-function registry (same tuple shape,
    /// keyed by the plain fn name) — statement-position calls (`bump(c)`)
    /// type their args and know whether a result needs dropping.
    mod_fns: &'a AssocFnRegistry,
    /// PMAT-1028: the module's str-RETURNING callables (free/assoc fns by
    /// call key, methods by `(struct, method)`). A call in a STRING position
    /// (`s: str = build(5)`, a concat operand, a str return) must be
    /// verified to actually produce a str pointer — its i32 result alone is
    /// ambiguous (bool and struct returns are i32 too).
    str_rets: &'a StrReturners,
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

    /// PMAT-1023: the call signature — (non-self param WAT types, result;
    /// `None` result = unit/void) — of `sname.mname`, if the module defines it.
    fn method_sig(&self, sname: &str, mname: &str) -> Option<(&[WatTy], Option<WatTy>)> {
        self.methods
            .iter()
            .find(|(s, m, _, _)| s == sname && m == mname)
            .map(|(_, _, p, r)| (p.as_slice(), *r))
    }

    /// PMAT-1023: the call signature of the associated fn registered under
    /// the exact callee string `key` (e.g. `"Counter::__init__"`), if any.
    fn assoc_sig(&self, key: &str) -> Option<(&[WatTy], Option<WatTy>)> {
        self.assoc_fns
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, p, r)| (p.as_slice(), *r))
    }

    /// PMAT-1024: a FREE module function's signature, by plain name.
    fn mod_fn_sig(&self, key: &str) -> Option<(&[WatTy], Option<WatTy>)> {
        self.mod_fns
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, p, r)| (p.as_slice(), *r))
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

    /// PMAT-986: `true` if `name` is a `str` param or (PMAT-1028) str-annotated
    /// local base-pointer (a length-prefixed UTF-8 byte region in linear
    /// memory). Drives `len(s)`, `ord(s[i])`, and string-position lowering.
    fn is_str_name(&self, name: &str) -> bool {
        self.str_names.iter().any(|n| n == name)
    }

    /// PMAT-1028: `true` if the callable registered under call key `key` (a
    /// plain free-fn name or a `<Struct>::<name>` assoc key) returns a `str`.
    fn call_returns_str(&self, key: &str) -> bool {
        self.str_rets.keys.iter().any(|k| k == key)
    }

    /// PMAT-1028: `true` if `<sname>.<mname>` is a str-returning method.
    fn method_returns_str(&self, sname: &str, mname: &str) -> bool {
        self.str_rets
            .methods
            .iter()
            .any(|(s, m)| s == sname && m == mname)
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

/// Emit one `(func …)` for `f`. `wat_name` is the WAT symbol + export name —
/// `f.name` for a free function, `<Struct>.<method>` for a struct method
/// (PMAT-1023; both are legal WAT id characters, and Python identifiers can
/// never collide with the dotted form).
#[allow(clippy::too_many_arguments)]
/// The module-wide lookup tables `emit_function` lowers against, built once
/// per module by `emit_module` and shared (immutably) by every function and
/// method body. Bundled so the per-function entry point stays a small
/// signature as registries accrete (PMAT-1028 added `str_rets`).
struct Registries<'a> {
    literals: &'a StrLiterals,
    structs: &'a StructRegistry,
    methods: &'a MethodRegistry,
    assoc_fns: &'a AssocFnRegistry,
    mod_fns: &'a AssocFnRegistry,
    str_rets: &'a StrReturners,
}

fn emit_function(
    f: &Function,
    regs: &Registries<'_>,
    wat_name: &str,
) -> Result<String, BackendError> {
    let Registries {
        literals,
        structs,
        methods,
        assoc_fns,
        mod_fns,
        str_rets,
    } = *regs;
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
    } else if matches!(f.return_type, Type::Struct(_)) {
        // PMAT-1023: a struct result rides an i32 base-pointer (the heap
        // record) — required by the desugared explicit `__init__` ctor
        // (`-> Self { Self { … } }`), and it upgrades the PMAT-996 posture
        // for free functions too (`def make(): return Point(1, 2)` lowers;
        // the trailing `StructLit` leaves exactly this pointer).
        WatTy::I32
    } else {
        map_type(&f.return_type)?
    };

    let mut scope = Scope {
        locals: Vec::new(),
        list_elem: Vec::new(),
        str_names: Vec::new(),
        heap_maps: Vec::new(),
        structs,
        struct_locals: Vec::new(),
        methods,
        assoc_fns,
        mod_fns,
        str_rets,
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
            scope.str_names.push(name.clone());
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
    write!(out, "  (func ${wat_name} ").expect("write");
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
    // PMAT-1002: the f64 float-divisor scratch (zero-divisor guard).
    if body.contains(&format!("${FDIV_SCRATCH}")) {
        writeln!(out, "    (local ${FDIV_SCRATCH} f64)").expect("write");
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
    // PMAT-1033: declare the list-construction scratch `i32` local iff a
    // `ListLit` actually used it (same body-driven detection).
    if body.contains(&format!("${LIST_DST_SCRATCH}")) {
        writeln!(out, "    (local ${LIST_DST_SCRATCH} i32)").expect("write");
    }

    out.push_str(&body);
    writeln!(out, "  )").expect("write");
    writeln!(out, "  (export \"{wat_name}\" (func ${wat_name}))").expect("write");
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
            // PMAT-1028: a str LET binds an `i32` base-pointer local AND
            // registers in the scope's str-name set, so every string position
            // (concat operand, `==` content compare, len/ord/s[i], a str
            // return) classifies the local exactly like a str param.
            // Intercepted before `map_type`, which refuses `Str`.
            Stmt::Let {
                name,
                ty: Type::Str,
                ..
            } => {
                scope.declare(name, WatTy::I32);
                scope.str_names.push(name.clone());
            }
            // PMAT-1033: a `list[scalar]` LET binds an `i32` base-pointer
            // local AND records its element type in `scope.list_elem` — the
            // SAME registry a list param uses, so `xs[i]` reads/writes,
            // `len(xs)`, and the PMAT-1030 ForEach desugar treat the local
            // exactly like a param (bounds guards + typed loads verbatim).
            // Intercepted before `map_type`, which refuses `List`.
            Stmt::Let {
                name,
                ty: Type::List(inner),
                ..
            } => {
                let elem = map_list_elem_type(inner)?;
                scope.declare(name, WatTy::I32);
                scope.list_elem.push((name.clone(), elem));
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
            // PMAT-1023: `obj.field = v` writes an EXISTING record's field and
            // a statement method call mutates through an existing pointer —
            // neither introduces a new local.
            Stmt::Assign { .. }
            | Stmt::IndexAssign { .. }
            | Stmt::DictSet { .. }
            | Stmt::SetAdd { .. }
            | Stmt::FieldAssign { .. }
            | Stmt::SideEffectCall { .. }
            | Stmt::Return(_)
            | Stmt::Break
            | Stmt::Continue => {}
            // PMAT-1034: a `raise` (→ `unreachable` trap) introduces no
            // locals — its message expression is never evaluated on WASM.
            Stmt::Raise { .. } => {}
            // PMAT-1033: growth stays REFUSED precisely — a fixed-size list
            // record cannot grow in place on the bump heap, and relocating it
            // would silently break every alias holding the old base-pointer
            // (the PMAT-999 relocation-hazard posture).
            Stmt::ListAppend { list_name, .. } => {
                return Err(unsupported(&format!(
                    "`{list_name}.append(…)` — list growth is outside the WASM \
                     subset (a fixed-size heap record cannot grow in place; \
                     relocation would break aliases). Pre-size the list and \
                     write `{list_name}[i] = …` instead"
                )));
            }
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
        Stmt::FieldAssign { .. } => "FieldAssign",
        Stmt::FieldIndexAssign { .. } => "FieldIndexAssign",
        Stmt::TryCatch { .. } => "TryCatch",
        Stmt::SideEffectCall { .. } => "SideEffectCall",
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
            // PMAT-1028: a str-name binding routes through the dedicated
            // string lowering — its value must be string-VALUED (a str name,
            // a literal, a Concat/Chr/s[i] result), NOT merely i32-typed (a
            // bool is i32 too; the generic typed path could silently bind a
            // 0/1 as a "pointer"). Strings are immutable in Python, so the
            // pointer copy IS reference semantics — no disposition needed.
            if scope.is_str_name(name) {
                emit_str_expr(value, scope, out, depth)?;
                indent(out, depth);
                writeln!(out, "local.set ${name}").expect("write");
                return Ok(());
            }
            // PMAT-1033: a list-name binding routes through the dedicated
            // list lowering — its value must be list-VALUED (a `ListLit`
            // materialised on the bump heap, or another list name: a pointer
            // copy, which IS Python's reference/sharing semantics — the
            // PMAT-1024 reference-native posture, no disposition needed).
            if let Some(elem) = scope.list_elem_of(name) {
                emit_list_expr(value, elem, scope, out, depth)?;
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
            // PMAT-1028: str reassignment — the string-ACCUMULATOR idiom
            // (`out = out + chr(…)` in a loop). Concat allocates a fresh
            // heap string each pass; rebinding the local to the new pointer
            // is exactly CPython's immutable-str rebind. Covers str PARAM
            // reassignment too (params are in the str-name set).
            if scope.is_str_name(name) {
                emit_str_expr(value, scope, out, depth)?;
                indent(out, depth);
                writeln!(out, "local.set ${name}").expect("write");
                return Ok(());
            }
            // PMAT-1033: list reassignment — `xs = [4, 5]` allocates a fresh
            // record and rebinds the local; `ys = xs` rebinds to the same
            // base-pointer (Python's rebind never mutates the old record, so
            // both are exact). Covers list PARAM reassignment too.
            if let Some(elem) = scope.list_elem_of(name) {
                emit_list_expr(value, elem, scope, out, depth)?;
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
        // PMAT-1023: `obj.field = value` — store through the struct
        // local/param's base-pointer (the OOP mutation primitive).
        Stmt::FieldAssign { obj, field, value } => {
            emit_field_assign(obj, field, value, scope, out, depth)
        }
        // PMAT-1023: a statement-position call evaluated for its side effect —
        // `c.incr()` / `acc.add(5)`. A STRUCT METHOD call's result type is
        // known from the method registry, so a unit method leaves nothing and
        // a value-returning method's result is dropped (Python's statement-
        // position discard). PMAT-1024: a PLAIN function-call statement
        // (`bump(c)` — the mutating-helper idiom the reference-semantics
        // frontend passes through as a bare heap pointer) resolves the same
        // way via the free-function registry.
        Stmt::SideEffectCall { call } => {
            match call {
                Expr::MethodCall { obj, method, args } => {
                    if emit_method_call(obj, method, args, scope, out, depth)?.is_some() {
                        indent(out, depth);
                        writeln!(out, "drop").expect("write");
                    }
                }
                Expr::Call { callee, args } => {
                    let Some((ptys, ret)) = scope.mod_fn_sig(callee).map(|(p, r)| (p.to_vec(), r))
                    else {
                        return Err(unsupported(&format!(
                            "statement-position call to `{callee}` — not a module \
                             function of this WASM module"
                        )));
                    };
                    if ptys.len() != args.len() {
                        return Err(unsupported(&format!(
                            "`{callee}` takes {} argument(s) but the call passes {}",
                            ptys.len(),
                            args.len()
                        )));
                    }
                    for (a, pt) in args.iter().zip(ptys.iter()) {
                        emit_expr_typed(a, scope, out, depth, *pt)?;
                    }
                    indent(out, depth);
                    writeln!(out, "call ${callee}").expect("write");
                    if ret.is_some() {
                        indent(out, depth);
                        writeln!(out, "drop").expect("write");
                    }
                }
                other => {
                    return Err(unsupported(&format!(
                        "statement-position {} — the WASM subset lowers \
                         `obj.method(…)` and `helper(…)` statements only",
                        expr_kind(other)
                    )));
                }
            }
            Ok(())
        }
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
        // PMAT-1034: `raise Exc("…")` — Python's raise is an error exit; the
        // WASM analogue is an `unreachable` trap, matching the existing
        // IndexError/TypeError-analogue trap posture (PMAT-968/1030). The
        // message expression is NOT evaluated or carried (a WAT trap has no
        // payload); the raise/no-raise boundary is what the lane preserves.
        // First producer: the empty-iterable loop-var-leak guard (the
        // UnboundLocalError analogue), which traps exactly where CPython
        // raises.
        Stmt::Raise { .. } => {
            indent(out, depth);
            writeln!(out, "unreachable ;; raise (Python exception analogue)").expect("write");
            Ok(())
        }
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
/// PMAT-1126: lower `s.startswith(p)` / `s.endswith(p)` to a bool (`i32`)
/// result. Both `recv` and `arg` are string-valued, so each lowers to an `i32`
/// base-pointer via [`emit_str_expr`] (which refuses a non-str operand — an
/// honest type mismatch at the typed site), then `$__wasm_str_<which>` does the
/// byte prefix/suffix compare. `which` is `"startswith"` or `"endswith"`; the
/// matching helper is emitted once per module (gated by the caller). No heap.
fn emit_str_prefix_op(
    recv: &Expr,
    arg: &Expr,
    which: &str,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    emit_str_expr(arg, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_str_{which}").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1153: lower `s.removeprefix(p)` / `s.removesuffix(p)` to a NEW heap
/// string (`i32` base-pointer). Both `recv` and `arg` are string-valued, so each
/// lowers to an `i32` base-pointer via [`emit_str_expr`] (which refuses a
/// non-str operand — an honest type mismatch at the typed site), then
/// `$__wasm_str_<which>` (`which` = `"removeprefix"` / `"removesuffix"`) copies
/// the retained byte range into a fresh allocation. The result is the new str
/// pointer (`WatTy::I32`), so it composes with `len` / `Concat` / equality / a
/// str return like any other heap string. The helper is emitted once per module
/// (gated by the caller on `needs_removeprefix` / `needs_removesuffix`).
fn emit_str_remove(
    recv: &Expr,
    arg: &Expr,
    which: &str,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    emit_str_expr(arg, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_str_{which}").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1159: lower `s.replace(old, new)` — a materialising op leaving the i32
/// base-pointer of a fresh heap string. All three operands are string-valued
/// (`emit_str_expr`, which refuses a non-str operand honestly); the allocating
/// `$__wasm_str_replace` helper does the two-pass (or empty-`old` interleave)
/// substitution. A heap-constructed operand (`("a"+b).replace(x, y)`) already
/// pulled in the allocator via `expr_has_heap_op`.
fn emit_str_replace(
    recv: &Expr,
    old: &Expr,
    new: &Expr,
    count: Option<&Expr>,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    emit_str_expr(old, scope, out, depth)?;
    emit_str_expr(new, scope, out, depth)?;
    // PMAT-1161: the 4th (i64) helper param is the replacement cap. The 2-arg
    // `.replace(old, new)` passes -1 (unlimited → replace-all, the prior
    // behaviour); the 3-arg `.replace(old, new, count)` passes the lowered count
    // expr (typed i64 in the frontend), coerced onto the stack as i64.
    match count {
        Some(c) => emit_expr_typed(c, scope, out, depth, WatTy::I64)?,
        None => {
            indent(out, depth);
            writeln!(out, "i64.const -1").expect("write");
        }
    }
    indent(out, depth);
    writeln!(out, "call $__wasm_str_replace").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1173: lower `s.zfill(width)` — a materialising op leaving the i32
/// base-pointer of a fresh heap string. The receiver is string-valued
/// (`emit_str_expr`, which refuses a non-str recv honestly); the width is the
/// int arg, coerced onto the stack as i64 (the helper's second param — wrapped
/// to i32 inside the helper). The allocating `$__wasm_str_zfill` helper does the
/// sign-aware zero-pad. A heap-constructed receiver (`(a + b).zfill(8)`) already
/// pulled in the allocator via `expr_has_heap_op`.
fn emit_str_zfill(
    recv: &Expr,
    width: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    emit_expr_typed(width, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_str_zfill").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1185: lower `s.upper()` (`up` = 1) / `s.lower()` (`up` = 0) — a
/// materialising op leaving the i32 base-pointer of a fresh case-flipped heap
/// string. The receiver is string-valued (`emit_str_expr`, which refuses a
/// non-str recv honestly); the `up` direction flag is an immediate i32 const.
/// The allocating `$__wasm_str_upper_lower` helper case-flips the ASCII letters
/// and TRAPS on a non-ASCII byte (the honest ASCII-only boundary — never a silent
/// un-folded pass-through). A heap-constructed receiver (`(a + b).upper()`)
/// already pulled in the allocator via `expr_has_heap_op`.
fn emit_str_case(
    recv: &Expr,
    up: bool,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "i32.const {}", i32::from(up)).expect("write");
    indent(out, depth);
    writeln!(out, "call $__wasm_str_upper_lower").expect("write");
    Ok(WatTy::I32)
}

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
            // PMAT-1023: a call to a REGISTERED associated fn (the frontend's
            // desugared explicit `__init__`: `Counter(0)` lowers to
            // `Call { callee: "Counter::__init__" }`) types exactly — each
            // arg against its declared param, the result from the registry
            // (an i32 heap pointer for a ctor). The WAT symbol IS the callee
            // string (`::` is a legal WAT id character).
            if let Some((ptys, ret)) = scope.assoc_sig(callee) {
                if ptys.len() != args.len() {
                    return Err(unsupported(&format!(
                        "`{callee}` takes {} argument(s) but the call passes {}",
                        ptys.len(),
                        args.len()
                    )));
                }
                for (a, pt) in args.iter().zip(ptys.iter()) {
                    emit_expr_typed(a, scope, out, depth, *pt)?;
                }
                indent(out, depth);
                writeln!(out, "call ${callee}").expect("write");
                return ret.ok_or_else(|| {
                    unsupported(&format!(
                        "`{callee}` returns no value (unit) — its call cannot be \
                         used in a value position"
                    ))
                });
            }
            // PMAT-1026: a call to a plain FREE module function types exactly
            // the same way via the PMAT-1024 registry — the FnSig knows a
            // `Struct`/`str` return rides an i32 base-pointer, so the factory
            // idiom (`def make() -> Counter`) and the returns-param identity
            // shape no longer mistype as a conservative i64 (which refused as
            // an i32/i64 mismatch at every struct-typed use site).
            if let Some((ptys, ret)) = scope.mod_fn_sig(callee) {
                if ptys.len() != args.len() {
                    return Err(unsupported(&format!(
                        "`{callee}` takes {} argument(s) but the call passes {}",
                        ptys.len(),
                        args.len()
                    )));
                }
                for (a, pt) in args.iter().zip(ptys.iter()) {
                    emit_expr_typed(a, scope, out, depth, *pt)?;
                }
                indent(out, depth);
                writeln!(out, "call ${callee}").expect("write");
                return ret.ok_or_else(|| {
                    unsupported(&format!(
                        "`{callee}` returns no value (unit) — its call cannot be \
                         used in a value position"
                    ))
                });
            }
            // Every module function is in the registry, so an unresolved
            // callee is NOT defined in this module — `call $<callee>` would
            // emit invalid WAT (the old conservative-i64 path deferred that
            // to a confusing wat2wasm failure). Refuse it by name, mirroring
            // the statement-position lowering.
            Err(unsupported(&format!(
                "call to `{callee}` — not a function of this WASM module"
            )))
        }
        Expr::Index { collection, index } => emit_index(collection, index, scope, out, depth),
        Expr::Len(collection) => emit_len(collection, scope, out, depth),
        // PMAT-1003: `len(s)` over a str is synthesized by the frontend as
        // StrMethod(CharCount) (Python counts Unicode code points, so a str len
        // must NOT reuse Expr::Len = byte length). Since PMAT-1032 emit_len
        // lowers a str name to the `$__wasm_str_charlen` helper — the REAL
        // code-point count, exact for non-ASCII input too. Other string
        // methods (upper/lower/strip/split/…) are refused honestly.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::CharCount,
            args,
        } if args.is_empty() => emit_len(recv, scope, out, depth),
        // PMAT-1126: `s.startswith(p)` / `s.endswith(p)` — a bool (i32) result
        // over a byte prefix/suffix compare of two length-prefixed UTF-8 strings.
        // Both operands lower to i32 base-pointers (`emit_str_expr`), then the
        // matching non-allocating helper. Byte prefix/suffix == code-point
        // prefix/suffix for valid UTF-8, so this IS Python's semantics; nothing
        // is allocated (a bool, not a new string).
        Expr::StrMethod {
            recv,
            op: StrMethodOp::StartsWith,
            args,
        } if args.len() == 1 => emit_str_prefix_op(recv, &args[0], "startswith", scope, out, depth),
        Expr::StrMethod {
            recv,
            op: StrMethodOp::EndsWith,
            args,
        } if args.len() == 1 => emit_str_prefix_op(recv, &args[0], "endswith", scope, out, depth),
        // PMAT-1128: `s.count(p)` — an int (i64) result: the count of
        // NON-OVERLAPPING occurrences of `p` in `s`. Both operands lower to i32
        // base-pointers (`emit_str_expr`), then `$__wasm_str_count` (the counting
        // generalisation of `$__wasm_str_contains`). A byte occurrence count IS a
        // code-point occurrence count for valid UTF-8 (`p[0]` is a lead byte); the
        // empty-needle case is charlen(s)+1 inside the helper. Allocates nothing.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::Count,
            args,
        } if args.len() == 1 => {
            emit_str_expr(recv, scope, out, depth)?;
            emit_str_expr(&args[0], scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_count").expect("write");
            Ok(WatTy::I64)
        }
        // PMAT-1136: `s.find(p)` — an int (i64) result: the CODE-POINT index of
        // the first occurrence of `p` in `s`, or -1 if absent. Both operands lower
        // to i32 base-pointers (`emit_str_expr`), then `$__wasm_str_find` (the
        // index-returning sibling of `$__wasm_str_contains`, converting the byte
        // offset to a char index since Python `find` is char-indexed). Allocates
        // nothing.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::Find,
            args,
        } if args.len() == 1 => {
            emit_str_expr(recv, scope, out, depth)?;
            emit_str_expr(&args[0], scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_find").expect("write");
            Ok(WatTy::I64)
        }
        // PMAT-1163: `s.find(p, start)` — the start-bounded form: the CODE-POINT
        // index of the first occurrence of `p` in `s` AT OR AFTER code-point index
        // `start`, or -1. The two str operands lower to i32 base-pointers
        // (`emit_str_expr`); `args[1]` (the start, typed `int` in the frontend) is
        // coerced onto the stack as i64, then `$__wasm_str_find_from` applies the
        // Python start clamp (negative → from-end, > len → -1, empty-needle → start)
        // and the byte-offset → char-index conversion. Allocates nothing. A 3-arg
        // `.find(p, start, end)` still falls through to the honest refusal below (no
        // end-bounded search yet).
        Expr::StrMethod {
            recv,
            op: StrMethodOp::Find,
            args,
        } if args.len() == 2 => {
            emit_str_expr(recv, scope, out, depth)?;
            emit_str_expr(&args[0], scope, out, depth)?;
            emit_expr_typed(&args[1], scope, out, depth, WatTy::I64)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_find_from").expect("write");
            Ok(WatTy::I64)
        }
        // PMAT-1143: `s.rfind(p)` — an int (i64) result: the CODE-POINT index of
        // the LAST occurrence of `p` in `s`, or -1 if absent. The reverse-scan
        // sibling of `.find(p)`: both operands lower to i32 base-pointers
        // (`emit_str_expr`), then `$__wasm_str_rfind` (which scans candidate
        // offsets from the right and converts the byte offset to a char index,
        // since Python `rfind` is char-indexed). Allocates nothing.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::Rfind,
            args,
        } if args.len() == 1 => {
            emit_str_expr(recv, scope, out, depth)?;
            emit_str_expr(&args[0], scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_rfind").expect("write");
            Ok(WatTy::I64)
        }
        // PMAT-1165: `s.rfind(p, start)` — the start-bounded reverse form: the
        // CODE-POINT index of the LAST occurrence of `p` in `s` whose match STARTS
        // at or after code-point index `start`, or -1. Mirrors the 2-arg `find`
        // lowering: the two str operands lower to i32 base-pointers
        // (`emit_str_expr`); `args[1]` (the start, typed `int` in the frontend) is
        // coerced onto the stack as i64, then `$__wasm_str_rfind_from` applies the
        // Python start clamp (negative → from-end, > len → -1, empty-needle → len)
        // and the byte-offset → char-index conversion. Allocates nothing. A 3-arg
        // `.rfind(p, start, end)` still falls through to the honest refusal below (no
        // end-bounded reverse search yet).
        Expr::StrMethod {
            recv,
            op: StrMethodOp::Rfind,
            args,
        } if args.len() == 2 => {
            emit_str_expr(recv, scope, out, depth)?;
            emit_str_expr(&args[0], scope, out, depth)?;
            emit_expr_typed(&args[1], scope, out, depth, WatTy::I64)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_rfind_from").expect("write");
            Ok(WatTy::I64)
        }
        // PMAT-1144: `s.index(p)` / `s.rindex(p)` — the TRAPPING siblings of
        // `.find(p)` / `.rfind(p)`: an int (i64) result, the CODE-POINT index of the
        // first / last occurrence of `p` in `s`, but a MISSING needle raises Python
        // `ValueError` — lowered here to a WASM trap inside the wrapper. Both
        // operands lower to i32 base-pointers (`emit_str_expr`), then
        // `$__wasm_str_index` / `$__wasm_str_rindex` (each `unreachable`s when its
        // wrapped search returns -1). Single-arg only, mirroring find/rfind; a
        // `.index(p, start[, end])` form falls through to the honest refusal below
        // (WASM has no start/end search yet). Allocates nothing.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::StrIndex,
            args,
        } if args.len() == 1 => {
            emit_str_expr(recv, scope, out, depth)?;
            emit_str_expr(&args[0], scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_index").expect("write");
            Ok(WatTy::I64)
        }
        Expr::StrMethod {
            recv,
            op: StrMethodOp::RIndex,
            args,
        } if args.len() == 1 => {
            emit_str_expr(recv, scope, out, depth)?;
            emit_str_expr(&args[0], scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_rindex").expect("write");
            Ok(WatTy::I64)
        }
        // PMAT-1153: `s.removeprefix(p)` / `s.removesuffix(p)` — a NEW heap string
        // (i32 base-pointer): `s` with a leading / trailing `p` removed when
        // present, else a fresh copy of `s`. Both operands lower to i32
        // base-pointers (`emit_str_expr`), then the allocating helper copies the
        // retained byte range (which starts/ends on a code-point boundary, so the
        // byte copy is char-exact). Single-arg only; a 2-arg form (there is none in
        // Python) would fall through to the refusal below.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::RemovePrefix,
            args,
        } if args.len() == 1 => emit_str_remove(recv, &args[0], "removeprefix", scope, out, depth),
        Expr::StrMethod {
            recv,
            op: StrMethodOp::RemoveSuffix,
            args,
        } if args.len() == 1 => emit_str_remove(recv, &args[0], "removesuffix", scope, out, depth),
        // PMAT-1159: `s.replace(old, new)` — a NEW heap string (i32 base-pointer)
        // with every non-overlapping `old` replaced by `new` (unlimited → the
        // helper's count = -1).
        Expr::StrMethod {
            recv,
            op: StrMethodOp::Replace,
            args,
        } if args.len() == 2 => emit_str_replace(recv, &args[0], &args[1], None, scope, out, depth),
        // PMAT-1161: `s.replace(old, new, count)` — the bounded form: only the
        // first `count` non-overlapping occurrences are replaced (count < 0 →
        // unlimited, matching Python). `args[2]` is the i64 cap threaded to the
        // helper's 4th param.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::ReplaceN,
            args,
        } if args.len() == 3 => {
            emit_str_replace(recv, &args[0], &args[1], Some(&args[2]), scope, out, depth)
        }
        Expr::StrMethod { op, .. } => Err(unsupported(&format!(
            "string method {op:?} on the WASM lane — only `len(s)` (CharCount), \
             `.startswith(p)`, `.endswith(p)`, `.count(p)`, `.find(p)`, \
             `.find(p, start)`, `.rfind(p)`, `.rfind(p, start)`, `.index(p)`, \
             `.rindex(p)`, `.removeprefix(p)`, `.removesuffix(p)`, \
             `.replace(old, new)`, and `.replace(old, new, count)` are supported; \
             upper/lower/strip/split/…, the 3-arg `.find`/`.rfind`(p, start, end), \
             and the start/end forms of index/rindex/count are refused"
        ))),
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
        // PMAT-1142: `s * n` MATERIALISES a new heap string = the source bytes
        // replicated max(n, 0) times, leaving its i32 base-pointer. A LIST
        // repeat (`of_str: false`) is refused inside `emit_repeat`.
        Expr::Repeat { seq, n, of_str } => {
            emit_repeat(seq, n, *of_str, scope, out, depth)?;
            Ok(WatTy::I32)
        }
        // PMAT-1058: `s[lo:hi]` — a char-exact string slice, materialised as a
        // NEW heap substring. The result is an i32 (the str pointer). A list
        // slice / stepped string slice is refused inside `emit_str_slice`.
        Expr::Slice {
            collection,
            lo,
            hi,
            of_str,
            step,
        } => {
            emit_str_slice(collection, lo, hi, *of_str, *step, scope, out, depth)?;
            Ok(WatTy::I32)
        }
        // PMAT-1060: `str(n)` / `repr(n)` over an `int` — materialise a NEW
        // decimal-ASCII heap string. The result is an i32 (the str pointer).
        // `str(float)` (`of_float: true`) and any non-int operand are refused
        // inside `emit_int_to_str` (an honest type mismatch at the typed site).
        Expr::ToStr { value, of_float } => {
            emit_int_to_str(value, *of_float, scope, out, depth)?;
            Ok(WatTy::I32)
        }
        // PMAT-995 (slice 3b): `d[k]` — keyed dict read; returns the i64 value
        // or TRAPS on an absent key (the Python KeyError analogue).
        Expr::DictGet { dict, key } => emit_dict_get(dict, key, scope, out, depth),
        // PMAT-995 (slice 3b): `k in d` / `x in s` — i32 bool membership.
        Expr::DictContains { dict, key } => emit_dict_contains(dict, key, scope, out, depth),
        Expr::SetContains { set, elem } => emit_dict_contains(set, elem, scope, out, depth),
        // PMAT-1127: `needle in haystack` over strings — an i32 bool substring
        // test via a non-allocating byte search. Both operands lower to i32
        // base-pointers (`emit_str_expr`); `$__wasm_str_contains` slides the
        // needle over the haystack. A byte-substring match IS a code-point
        // substring match for valid UTF-8 (the needle's lead byte forces the
        // compare onto a char boundary), so this IS Python's `in` — nothing is
        // allocated (a bool, not a new string). A non-str operand is refused by
        // `emit_str_expr` (an honest type mismatch).
        Expr::StrContains { haystack, needle } => {
            emit_str_expr(haystack, scope, out, depth)?;
            emit_str_expr(needle, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_contains").expect("write");
            Ok(WatTy::I32)
        }
        // PMAT-996 (slice 4): `Name(f=v, …)` — allocate + populate a plain-data
        // struct on the bump heap; leaves the instance's i32 base-pointer.
        Expr::StructLit { name, fields } => emit_struct_lit(name, fields, scope, out, depth),
        // PMAT-996 (slice 4): `obj.field` — load a field from a struct local/param.
        Expr::FieldAccess { obj, field } => emit_field_access(obj, field, scope, out, depth),
        // PMAT-1023: `obj.method(args)` in a VALUE position — the method must
        // return a value (a unit method's "result" cannot feed an expression).
        Expr::MethodCall { obj, method, args } => {
            emit_method_call(obj, method, args, scope, out, depth)?.ok_or_else(|| {
                unsupported(&format!(
                    "method `.{method}(…)` returns no value (Python `-> None`) — \
                     a unit method call cannot be used in a value position"
                ))
            })
        }
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
            "index over `{name}` which is not a `list[scalar]` param/local — \
             only a list (i32 base-pointer into linear memory) can be \
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
             not a `list[scalar]` param/local — only a list (i32 \
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
/// `collection` is either an [`Expr::Ident`] naming a list/str/dict/set
/// base-pointer (its `+0` count header, char-counted for a `str`), OR (PMAT-1148)
/// a string-VALUED temporary (`Concat` / `s * n` / `s[lo:hi]` / str-valued
/// `if`/`else` / `chr` / `s[i]` / str-returning call) — lowered via
/// [`emit_str_expr`] to a length-prefixed pointer, then `$__wasm_str_charlen`.
/// `len` over anything else (a scalar, a list/dict LITERAL or list temporary
/// — none of which carry a length header) is refused.
fn emit_len(
    collection: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let Expr::Ident(name) = collection else {
        // PMAT-1148: `len()` of a temporary STRING expression. Every
        // string-VALUED form (a `Concat`, an `s * n` `Repeat`, a `s[lo:hi]`
        // `Slice`, a `str`-valued `if`/`else` — the `str(bool)` desugar — a
        // `Chr`, an `s[i]`, or a str-returning `Call`/`MethodCall`) lowers via
        // `emit_str_expr` to an i32 base-pointer to a length-prefixed region, so
        // `len` over it is its CODE-POINT count (`$__wasm_str_charlen`), exactly
        // as for a `str` NAME. The helper-requirement scans (`module_touches_str`
        // for charlen, `expr_has_heap_op`/`_str_slice`/`_str_repeat`/
        // `_int_to_str` for the allocator + op helpers) all recurse into
        // `Expr::Len`, so the callee helpers are always declared. A NON-string
        // temporary (a list/dict literal) fails `emit_str_expr` and refuses
        // honestly — never a silent miscompile. Lower into a scratch buffer so a
        // mid-lowering refusal leaves `out` untouched.
        let mut scratch = String::new();
        return match emit_str_expr(collection, scope, &mut scratch, depth) {
            Ok(()) => {
                out.push_str(&scratch);
                indent(out, depth);
                writeln!(out, "call $__wasm_str_charlen").expect("write");
                indent(out, depth);
                writeln!(out, "i64.extend_i32_u").expect("write");
                Ok(WatTy::I64)
            }
            Err(_) => Err(unsupported(
                "len() of a non-name collection — the WASM subset takes len() of \
                 a `list[scalar]`/`str`/`dict`/`set` NAME, or a string-VALUED \
                 temporary (a `Concat` a+b, an `s * n` repeat, an `s[lo:hi]` \
                 slice, a str-valued `if`/`else`, a `chr(n)`, an `s[i]`, or a \
                 str-returning call); len of a list/dict literal or a list \
                 temporary carries no length header and is refused",
            )),
        };
    };
    if scope.list_elem_of(name).is_none()
        && !scope.is_str_name(name)
        && scope.heap_map_kind(name).is_none()
    {
        return Err(unsupported(&format!(
            "len() over `{name}` which is not a `list[scalar]` param/local, a \
             `str` param/local, or a `dict`/`set` local — only those carry the \
             i32 count header at base+0 in the WASM subset"
        )));
    }
    // PMAT-1032: a STR name's len is its CHAR count (Python counts code
    // points), computed by the `$__wasm_str_charlen` helper — the byte-count
    // header is the ABI, not the Python-visible length ("héllo" is 6 bytes
    // but len 5). O(bytes) per call, correctness over speed.
    if scope.is_str_name(name) {
        indent(out, depth);
        writeln!(out, "local.get ${name}").expect("write");
        indent(out, depth);
        writeln!(out, "call $__wasm_str_charlen").expect("write");
        indent(out, depth);
        writeln!(out, "i64.extend_i32_u").expect("write");
        return Ok(WatTy::I64);
    }
    // PMAT-995: list/dict/set len = (i32 header at base+0) zero-extended to
    // i64 — an element count (list) or live-entry count (dict/set); both share
    // the `+0` i32 count header.
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load").expect("write");
    indent(out, depth);
    writeln!(out, "i64.extend_i32_u").expect("write");
    Ok(WatTy::I64)
}

/// Emit `ord(…)` over the WASM str subset (PMAT-986, char-exact since
/// PMAT-1032) — the string-reading op that returns an `int` (a code point)
/// rather than a new string.
///
/// Two accepted shapes, both lowered through `$__wasm_str_ord_at` (a
/// CHAR-indexed walk + 1..4-byte UTF-8 decode — see [`STR_CHAR_HELPERS`]):
///   * `ord(s[i])` — `Expr::Ord { value: Expr::StrCharAt { Ident(s), index } }`,
///     the frontend's lowering of Python `ord(s[i])`. Consuming the
///     `StrCharAt` here avoids materialising the 1-char string. Negative
///     indices normalise Python-style and out-of-range traps (`IndexError`
///     analogue), both inside the helper.
///   * `ord(ch)` over a bare str NAME (PMAT-1030) — the for-loop desugar
///     binds the loop var as a 1-char str local, making this the natural
///     checksum shape. Python's `ord` raises TypeError unless the string has
///     length exactly 1, so guard `charlen(s) != 1 → unreachable` — the CHAR
///     count, so `ord("é")` (1 char, 2 bytes) decodes to 233 exactly where
///     the pre-PMAT-1032 byte guard wrongly trapped.
///
/// Any other `ord` operand is refused: `ord(chr(n))` / `ord` of a literal
/// needs a materialised char; an `s[i]` whose base is not a str name is
/// likewise refused.
fn emit_ord(
    value: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    if let Expr::Ident(name) = value {
        if !scope.is_str_name(name) {
            return Err(unsupported(&format!(
                "ord({name}) where `{name}` is not a `str` param or local — \
                 only a str name (i32 base-pointer into linear memory) \
                 supports ord() in the WASM subset"
            )));
        }
        indent(out, depth);
        writeln!(out, "local.get ${name}").expect("write");
        indent(out, depth);
        writeln!(out, "call $__wasm_str_charlen").expect("write");
        indent(out, depth);
        writeln!(out, "i32.const 1").expect("write");
        indent(out, depth);
        writeln!(out, "i32.ne").expect("write");
        indent(out, depth);
        writeln!(out, "if").expect("write");
        indent(out, depth + 1);
        writeln!(
            out,
            "unreachable ;; ord() of a non-1-char string (Python TypeError)"
        )
        .expect("write");
        indent(out, depth);
        writeln!(out, "end").expect("write");
        indent(out, depth);
        writeln!(out, "local.get ${name}").expect("write");
        indent(out, depth);
        writeln!(out, "i64.const 0").expect("write");
        indent(out, depth);
        writeln!(out, "call $__wasm_str_ord_at").expect("write");
        return Ok(WatTy::I64);
    }
    let Expr::StrCharAt { string, index } = value else {
        return Err(unsupported(
            "ord() of a non-`s[i]`/non-name operand — the WASM subset lowers \
             `ord(s[i])` over a `str` name to a char-indexed UTF-8 decode, \
             and `ord(ch)` over a 1-char str name to its code point; \
             ord() of `chr(n)` or of a literal is refused",
        ));
    };
    let Expr::Ident(name) = string.as_ref() else {
        return Err(unsupported(
            "ord(s[i]) where the indexed value is not a name — only `ord(s[i])` \
             over a `str` parameter (i32 base-pointer) is supported",
        ));
    };
    if !scope.is_str_name(name) {
        return Err(unsupported(&format!(
            "ord({name}[i]) where `{name}` is not a `str` param or local — \
             only a str name (i32 base-pointer into linear memory) supports \
             indexed ord() in the WASM subset"
        )));
    }
    // Stack discipline: push the base pointer (i32), then the CHAR index
    // (i64), then call — the helper owns the negative-index normalisation,
    // the bounds trap, and the UTF-8 decode.
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    emit_expr_typed(index, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_str_ord_at").expect("write");
    Ok(WatTy::I64)
}

/// PMAT-1060: lower `str(n)` / `repr(n)` over an `int` — evaluate the operand
/// (which must lower to an `i64`), then call `$__wasm_int_to_str`, which
/// materialises the decimal-ASCII string in the bump heap and leaves its i32
/// base-pointer.
///
/// `str(float)` (`of_float: true`) is refused up front — a float→decimal repr
/// is a separate, much larger job (Python's shortest-round-trip `repr`), not a
/// silent `str(int)` reuse. `str(bool)` lowers to an i32 (not an i64) and
/// `str(str)` to a pointer, so the `emit_expr_typed(_, I64)` type check rejects
/// them with an honest mismatch rather than a wrong conversion.
fn emit_int_to_str(
    value: &Expr,
    of_float: bool,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    if of_float {
        return Err(unsupported(
            "str(float) / repr(float) on the WASM lane — a float→decimal repr \
             (shortest round-trip) is refused; only str(int) is supported",
        ));
    }
    // The operand must be an int (i64). A bool (i32) / float (f64) / str (i32
    // pointer) operand is a type mismatch here — refused, never mis-converted.
    emit_expr_typed(value, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_int_to_str").expect("write");
    Ok(())
}

/// PMAT-1058: lower a string slice `s[lo:hi]` — a char-exact heap substring.
///
/// Pushes the base string's i32 base-pointer (`emit_str_expr` — a param/local/
/// literal/concat/… all work), then the `lo` and `hi` CHARACTER indices as
/// `i64` (a missing bound lowers to `0` for `lo` and `i64::MAX` for `hi`, both
/// clamped to `[0, len]` by the helper), then calls `$__wasm_str_slice`, which
/// materialises the substring in the bump heap and leaves its i32 base-pointer.
///
/// Only the unstepped `of_str` form is supported. A LIST slice (`of_str: false`
/// — lists are param-only base-pointers in the WASM subset, there is no
/// list-return/temporary shape) and a STEPPED string slice (`s[i:j:k]`, incl.
/// the `xs[::-1]` reverse the frontend lowers to a negative `step`) refuse
/// honestly, never a silent miscompile.
#[allow(clippy::too_many_arguments)]
fn emit_str_slice(
    collection: &Expr,
    lo: &Option<Box<Expr>>,
    hi: &Option<Box<Expr>>,
    of_str: bool,
    step: Option<i64>,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    if !of_str {
        return Err(unsupported(
            "a LIST slice `xs[i:j]` — the WASM list subset carries a \
             `list[scalar]` only as a PARAMETER base-pointer (no list \
             temporaries / returns to hold a sub-list); refused",
        ));
    }
    if step.is_some() {
        return Err(unsupported(
            "a STEPPED string slice `s[i:j:k]` (incl. the `s[::-1]` reverse \
             idiom) — the WASM string subset slices `s[lo:hi]` (step 1) only; \
             refused",
        ));
    }
    // base string pointer.
    emit_str_expr(collection, scope, out, depth)?;
    // lo (i64 char index) — a missing `lo` defaults to 0.
    match lo {
        Some(b) => emit_expr_typed(b, scope, out, depth, WatTy::I64)?,
        None => {
            indent(out, depth);
            writeln!(out, "i64.const 0").expect("write");
        }
    }
    // hi (i64 char index) — a missing `hi` defaults to i64::MAX, which the
    // helper clamps down to the string's char length.
    match hi {
        Some(b) => emit_expr_typed(b, scope, out, depth, WatTy::I64)?,
        None => {
            indent(out, depth);
            writeln!(out, "i64.const 9223372036854775807").expect("write");
        }
    }
    indent(out, depth);
    writeln!(out, "call $__wasm_str_slice").expect("write");
    Ok(())
}

/// PMAT-993: emit a string-VALUED expression, leaving its `i32` base-pointer
/// (into the length-prefixed linear-memory region) on the WASM stack.
///
/// The string-valued forms are: a `str` PARAMETER (`Expr::Ident` of a str
/// param — already a base-pointer), a string LITERAL (PMAT-994 `Expr::LitStr`,
/// a constant static-`(data)` base-pointer), a `Concat` (string `+`,
/// materialised in the heap), a `Chr` (a new 1-char string), a bare
/// `StrCharAt` (PMAT-994 `s[i]` as a new 1-char heap string), and (PMAT-1058)
/// a `Slice` (`s[lo:hi]` as a new heap substring). Any other expression in a
/// string position is refused.
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
        Expr::Ident(name) if scope.is_str_name(name) => {
            indent(out, depth);
            writeln!(out, "local.get ${name}").expect("write");
            Ok(())
        }
        Expr::Ident(name) => Err(unsupported(&format!(
            "string-position use of `{name}` which is not a `str` parameter or \
             str-annotated local — the WASM string subset carries str params, \
             str locals (PMAT-1028), string literals, and heap-constructed \
             Concat/Chr/s[i] results"
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
        // PMAT-1142: `s * n` in a string position — a fresh heap string (like
        // `Concat`/`Slice`, a materialising op). A list repeat refuses in
        // `emit_repeat`.
        Expr::Repeat { seq, n, of_str } => emit_repeat(seq, n, *of_str, scope, out, depth),
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
        // PMAT-1058: `s[lo:hi]` as a string value — a char-exact heap
        // substring. A list slice / stepped slice refuses in `emit_str_slice`.
        Expr::Slice {
            collection,
            lo,
            hi,
            of_str,
            step,
        } => {
            emit_str_slice(collection, lo, hi, *of_str, *step, scope, out, depth)?;
            Ok(())
        }
        // PMAT-1060: `str(n)` in a string position — materialise the decimal
        // int string (like `Chr`/`Slice`, a fresh heap string). Re-materialised
        // per call (a concat operand evaluates it once per length/copy pass, the
        // same accepted heap-waste pattern the other materialising operands use).
        Expr::ToStr { value, of_float } => {
            emit_int_to_str(value, *of_float, scope, out, depth)?;
            Ok(())
        }
        // PMAT-1028: a CALL of a PROVEN str-returning callable (free fn,
        // `Struct::__init__`-style assoc fn) in a string position — the
        // factory-composition idiom `s: str = build(5)`. Delegates to the
        // typed value-position lowering (PMAT-1026: arity-checked, exactly
        // typed from the registry). Gated on the str-returner set, NOT the
        // i32 result alone — a bool/struct-returning call is i32 too and
        // must keep refusing here.
        Expr::Call { callee, .. } if scope.call_returns_str(callee) => {
            emit_expr_typed(e, scope, out, depth, WatTy::I32)?;
            Ok(())
        }
        // PMAT-1028: same for a str-returning METHOD on a struct local/param
        // (`obj.render()` feeding a string position).
        Expr::MethodCall { obj, method, .. }
            if matches!(obj.as_ref(), Expr::Ident(o)
                if scope.struct_of(o).is_some_and(|s| scope.method_returns_str(&s, method))) =>
        {
            emit_expr_typed(e, scope, out, depth, WatTy::I32)?;
            Ok(())
        }
        // PMAT-1147: a string-valued conditional `x if c else y` in a string
        // position — a WASM `(if (result i32) <cond> (then <ptr>) (else <ptr>))`
        // choosing between the two arms' i32 base-pointers. This is precisely the
        // shape the frontend's `str(bool)` desugar produces (`"True" if b else
        // "False"`, PMAT-502ae), and any string-valued ternary. Both arms lower
        // via `emit_str_expr`, so each is an already-correct pointer to a
        // length-prefixed UTF-8 string — no byte/code-point reasoning is needed
        // here (unlike the byte-search ops), and a non-string arm refuses
        // honestly through the recursion (never a silent miscompile). The arm
        // literals were laid out by `collect_expr_literals` (it recurses into
        // `IfExpr`), so the `then`/`else` pointers resolve. `cond` is an i32
        // bool (a Python `bool` lowers to i32).
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            emit_expr_typed(cond, scope, out, depth, WatTy::I32)?;
            indent(out, depth);
            writeln!(out, "if (result i32)").expect("write");
            emit_str_expr(then_expr, scope, out, depth + 1)?;
            indent(out, depth);
            writeln!(out, "else").expect("write");
            emit_str_expr(else_expr, scope, out, depth + 1)?;
            indent(out, depth);
            writeln!(out, "end").expect("write");
            Ok(())
        }
        // PMAT-1153: `s.removeprefix(p)` / `s.removesuffix(p)` in a string position
        // — a fresh heap string (like `Concat`/`Slice`/`Repeat`, a materialising
        // op), so it belongs here alongside the other string-RETURNING ops. Both
        // operands lower via `emit_str_expr`; the allocating helper copies the
        // retained byte range. A non-1-arg form (there is none in Python) falls
        // through to the honest refusal below.
        Expr::StrMethod {
            recv,
            op: op @ (StrMethodOp::RemovePrefix | StrMethodOp::RemoveSuffix),
            args,
        } if args.len() == 1 => {
            let which = match op {
                StrMethodOp::RemovePrefix => "removeprefix",
                _ => "removesuffix",
            };
            emit_str_remove(recv, &args[0], which, scope, out, depth)?;
            Ok(())
        }
        // PMAT-1159: `s.replace(old, new)` in a string position — a fresh heap
        // string (like Concat/Slice/Repeat/removeprefix), materialised by the
        // allocating `$__wasm_str_replace` helper. The unlimited form → count -1.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::Replace,
            args,
        } if args.len() == 2 => {
            emit_str_replace(recv, &args[0], &args[1], None, scope, out, depth)?;
            Ok(())
        }
        // PMAT-1161: `s.replace(old, new, count)` in a string position — the same
        // fresh heap string, but only the first `count` occurrences replaced
        // (count < 0 → unlimited). `args[2]` is the i64 cap.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::ReplaceN,
            args,
        } if args.len() == 3 => {
            emit_str_replace(recv, &args[0], &args[1], Some(&args[2]), scope, out, depth)?;
            Ok(())
        }
        // PMAT-1173: `s.zfill(width)` in a string position — a fresh heap string
        // (like Concat/Slice/Repeat/removeprefix/replace, a materialising op),
        // left-padded with ASCII `'0'` to `width` code points (sign-aware). The
        // width is the sole int arg; the allocating `$__wasm_str_zfill` helper
        // does the pad. Char-exact for any UTF-8 (the `'0'` bytes land on a
        // code-point boundary; the rest is a byte copy).
        Expr::StrMethod {
            recv,
            op: StrMethodOp::ZFill,
            args,
        } if args.len() == 1 => {
            emit_str_zfill(recv, &args[0], scope, out, depth)?;
            Ok(())
        }
        // PMAT-1185: `s.upper()` / `s.lower()` in a string position — a fresh heap
        // string (like Concat/Slice/Repeat/zfill, a materialising op) with every
        // ASCII letter case-flipped. Both are 0-arg; the allocating
        // `$__wasm_str_upper_lower` helper does the flip and TRAPS on a non-ASCII
        // byte (the honest ASCII-only boundary — full Unicode case folding needs a
        // case table this scalar lane does not carry, so it refuses at runtime
        // rather than silently returning an un-folded string).
        Expr::StrMethod {
            recv,
            op: op @ (StrMethodOp::Upper | StrMethodOp::Lower),
            args,
        } if args.is_empty() => {
            emit_str_case(recv, matches!(op, StrMethodOp::Upper), scope, out, depth)?;
            Ok(())
        }
        // PMAT-1166: a `StrFormat` reaching HERE is one the bare-`{}` fold in
        // `try_fold_strformat_to_concat` declined — a template carrying a
        // format spec (`"{:>5}".format(x)`), a positional (`"{0}"`), a named
        // field (`"{k}"`), or an arg-count mismatch. The simple bare-`{}` case
        // (`"{}-{}".format(x, y)`, `"%s=%d" % (a, b)`) already folded to a
        // `Concat` in the pre-pass; alignment / width / precision / positional /
        // keyword formatting is not modelled on the WASM lane.
        Expr::StrFormat { .. } => Err(unsupported(
            "a `str.format` / `%`-format template with a format spec, positional \
             (`{0}`), or named (`{k}`) field on the WASM lane — the simple \
             bare-`{}` case (`\"{}-{}\".format(x, y)`, `\"%s=%d\" % (a, b)`) folds \
             to a `Concat` (its int operands auto-stringified via str(int), \
             PMAT-1164/1166), but a spec / positional / keyword template is not \
             modelled; drop the spec or build the string with an f-string / `+ \
             str(x)` concatenation",
        )),
        // PMAT-1164: a bare single-interpolation f-string (`f"{x}"`) or one
        // carrying a format spec (`f"{x:>5}"`) lowers to a `FormatSpec` — there
        // is no surrounding literal to anchor a `Concat` fold, and a real spec
        // (alignment / width / precision) is not modelled on the WASM lane.
        Expr::FormatSpec { .. } => Err(unsupported(
            "a bare single-interpolation f-string (`f\"{x}\"`, no surrounding \
             literal) or a format spec (`f\"{x:>5}\"`) on the WASM lane — a \
             spec-less bare interpolation has no literal to anchor the str(int) \
             fold (PMAT-1164) and alignment / width / precision specs are not \
             modelled; use `str(x)` or add surrounding literal text",
        )),
        other => Err(unsupported(&format!(
            "expression {} in a string position — the WASM string subset \
             returns a `str` name (param/local), a string literal, a `Concat` \
             (a + b, incl. format int operands auto-stringified via str(int)), \
             a `Chr` (chr(n)), `s[i]`, `s[lo:hi]`, a str-valued `if`/`else`, \
             `.removeprefix(p)` / `.removesuffix(p)`, `.replace(old, new[, \
             count])`, `.zfill(width)`, `.upper()` / `.lower()` (ASCII-only — a \
             non-ASCII byte traps), or a str-returning call; stepped slicing \
             / str(float) / bare f-strings are refused",
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

/// PMAT-1142: lower a string repeat `s * n` / `n * s` (`Expr::Repeat { of_str:
/// true }`) — an ALLOCATING op that materialises a NEW heap string = the source
/// bytes replicated `max(n, 0)` times. The source string pointer is pushed
/// (`emit_str_expr`, which refuses a non-str `seq`), then the i64 count, then
/// `$__wasm_str_repeat` does the clamp + alloc + byte-replication loop, leaving
/// the new string's i32 base-pointer on the stack.
///
/// Pure byte replication is char-EXACT for valid UTF-8 (a multi-byte code point
/// is copied whole each pass), so it IS Python `str * int` for ANY string — no
/// case/code-point transform, unlike `.upper()`/`.lower()`. A LIST repeat
/// (`of_str: false`) is REFUSED: the WASM list subset carries fixed-size list
/// literals/params with no growth/replication op.
fn emit_repeat(
    seq: &Expr,
    n: &Expr,
    of_str: bool,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    if !of_str {
        return Err(unsupported(
            "list repeat `[…] * n` (Expr::Repeat, of_str: false) — the WASM list \
             subset carries fixed-size list literals/params with no growth / \
             replication op; only STRING repeat `s * n` is supported",
        ));
    }
    // src string base-pointer, then the i64 repeat count, then the helper.
    emit_str_expr(seq, scope, out, depth)?;
    emit_expr_typed(n, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_str_repeat").expect("write");
    Ok(())
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
        // PMAT-999: consume the returned (possibly grown) pointer back into the
        // scratch. Construction pre-sizes cap = n + slack so it never grows here,
        // but the helper now returns i32 and the value must not leak on the stack.
        indent(out, depth);
        writeln!(out, "local.set ${DICT_DST_SCRATCH}").expect("write");
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
        // PMAT-999: consume the returned pointer (see emit_dict_lit).
        indent(out, depth);
        writeln!(out, "local.set ${DICT_DST_SCRATCH}").expect("write");
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
    // PMAT-999: the helper returns the (possibly grown) base-pointer — update
    // the dict local so later reads see the relocated region.
    indent(out, depth);
    writeln!(out, "local.set ${dict_name}").expect("write");
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
    // PMAT-999: update the set local from the returned (possibly grown) pointer.
    indent(out, depth);
    writeln!(out, "local.set ${set_name}").expect("write");
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

/// PMAT-1023: the module's struct-METHOD signature registry — one entry per
/// `self`-receiver method: `(struct_name, method_name, non-self param WAT
/// types, result)`. A `None` result is a unit/void method (Python `->
/// None`). Built once by [`build_method_registry`]; carried in
/// [`Scope::methods`] so call sites type their args + result.
type MethodRegistry = Vec<(String, String, Vec<WatTy>, Option<WatTy>)>;

/// PMAT-1023: the module's ASSOCIATED-function registry — struct methods
/// WITHOUT a `self` receiver, keyed by the exact `Expr::Call` callee string
/// the frontend produces (`"<Struct>::<name>"`, e.g. the PMAT-1016B explicit
/// constructor `Counter::__init__`, which returns the struct's i32
/// base-pointer). Entries: `(call_key, param WAT types, result)`. Lets the
/// generic `Call` lowering type these calls exactly instead of assuming the
/// conservative i64 default.
type AssocFnRegistry = Vec<(String, Vec<WatTy>, Option<WatTy>)>;

/// The WAT result shape of a method/assoc-fn return type. `Unit` is `None`;
/// a `str` OR `Struct` return rides an i32 base-pointer (heap string /
/// heap record).
fn callable_ret(owner: &str, mname: &str, ret: &Type) -> Result<Option<WatTy>, BackendError> {
    match ret {
        Type::Unit => Ok(None),
        Type::Str => Ok(Some(WatTy::I32)),
        Type::Struct(_) => Ok(Some(WatTy::I32)),
        other => map_type(other).map(Some).map_err(|e| {
            unsupported(&format!(
                "struct `{owner}` method `{mname}` return type: {e}"
            ))
        }),
    }
}

/// Build the module's method + associated-fn registries. A method whose
/// first param is a `self: <Struct>` receiver registers as an instance
/// method (`<Struct>.<name>`); a method WITHOUT one (the frontend's
/// desugared explicit `__init__` constructor, static methods) registers as
/// an associated function under the call key `<Struct>::<name>`. Params
/// must map to WAT types; unsupported shapes refuse with the offender named.
fn build_method_registry(
    module: &Module,
) -> Result<(MethodRegistry, AssocFnRegistry), BackendError> {
    let mut reg = MethodRegistry::new();
    let mut assoc = AssocFnRegistry::new();
    for item in &module.items {
        if let Item::Struct { name, methods, .. } = item {
            for m in methods {
                let has_self = m.params.first().is_some_and(
                    |p| matches!((&p.name, &p.ty), (n, Type::Struct(s)) if n == "self" && s == name),
                );
                let value_params = if has_self {
                    &m.params[1..]
                } else {
                    &m.params[..]
                };
                let ptys = value_params
                    .iter()
                    .map(|p| param_wat_type(&p.ty))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        unsupported(&format!(
                            "struct `{name}` method `{mname}` parameter: {e}",
                            mname = m.name
                        ))
                    })?;
                let ret = callable_ret(name, &m.name, &m.return_type)?;
                if has_self {
                    reg.push((name.clone(), m.name.clone(), ptys, ret));
                } else {
                    assoc.push((format!("{name}::{}", m.name), ptys, ret));
                }
            }
        }
    }
    Ok((reg, assoc))
}

/// PMAT-1028: the module's str-RETURNING callables. `keys` holds free-fn
/// names and `<Struct>::<name>` assoc-fn call keys; `methods` holds
/// `(struct, method)` pairs for `self`-receiver instance methods. Consulted
/// by string-position lowering (`emit_str_expr`): a call's i32 result alone
/// cannot prove str-ness (bool and struct returns are i32 too), so only
/// callables in this set may feed a string position.
#[derive(Default)]
struct StrReturners {
    keys: Vec<String>,
    methods: Vec<(String, String)>,
}

/// Build the module's str-returner set — every free function, associated
/// fn, and instance method whose declared return type is `Type::Str`.
fn build_str_returners(module: &Module) -> StrReturners {
    let mut out = StrReturners::default();
    for item in &module.items {
        match item {
            Item::Function(f) if matches!(f.return_type, Type::Str) => {
                out.keys.push(f.name.clone());
            }
            Item::Struct { name, methods, .. } => {
                for m in methods {
                    if !matches!(m.return_type, Type::Str) {
                        continue;
                    }
                    let has_self = m.params.first().is_some_and(
                        |p| matches!((&p.name, &p.ty), (n, Type::Struct(s)) if n == "self" && s == name),
                    );
                    if has_self {
                        out.methods.push((name.clone(), m.name.clone()));
                    } else {
                        out.keys.push(format!("{name}::{}", m.name));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// PMAT-1024: build the FREE-function signature registry (plain fn name →
/// param WAT types + result shape). Statement-position plain calls
/// (`Stmt::SideEffectCall { call: Expr::Call }` — the `bump(c)`
/// mutating-helper idiom) consult it to type args and to know whether the
/// callee leaves a result to drop. Uses the same type mappings as
/// `emit_function`, so a function this refuses would refuse at emission
/// anyway — no new refusal surface.
fn build_module_fn_registry(module: &Module) -> Result<AssocFnRegistry, BackendError> {
    // PMAT-1026: FREE functions only. `module_functions` also yields struct
    // methods, which emit under the mangled `$<Struct>.<name>`/`::` symbols —
    // a plain-name registry entry for one could shadow (or masquerade as) a
    // free function the plain `call $<name>` emission cannot actually reach.
    let mut reg = AssocFnRegistry::new();
    let free_fns = module.items.iter().filter_map(|item| match item {
        Item::Function(f) => Some(f),
        _ => None,
    });
    for f in free_fns {
        let ptys = f
            .params
            .iter()
            .map(|p| param_wat_type(&p.ty))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| unsupported(&format!("function `{}` parameter: {e}", f.name)))?;
        let ret = match &f.return_type {
            Type::Unit => None,
            Type::Str | Type::Struct(_) => Some(WatTy::I32),
            other => Some(
                map_type(other)
                    .map_err(|e| unsupported(&format!("function `{}` return type: {e}", f.name)))?,
            ),
        };
        reg.push((f.name.clone(), ptys, ret));
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

/// PMAT-1033: lower a list-VALUED expression binding a `list[scalar]`
/// local/param — leaves the record's `i32` base-pointer on the stack.
///
/// Accepted shapes:
/// * [`Expr::ListLit`] — materialise a fresh length-prefixed record on the
///   bump heap ([`emit_list_lit`]);
/// * a list-name [`Expr::Ident`] with the SAME element type — a bare
///   `local.get`: the pointer copy IS Python's aliasing (mutations through
///   either name hit the one record — the PMAT-1024 reference-native
///   posture linear memory gives for free; the Rust lane must clone/refuse
///   these same shapes).
///
/// Everything else (list-returning calls, slices, comprehensions) refuses
/// honestly — never a silent scalar bound as a "pointer".
fn emit_list_expr(
    value: &Expr,
    elem: WatTy,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    match value {
        Expr::ListLit(elems) => emit_list_lit(elems, elem, scope, out, depth),
        Expr::Ident(src) => {
            let Some(src_elem) = scope.list_elem_of(src) else {
                return Err(unsupported(&format!(
                    "binding a list local from `{src}` which is not a \
                     `list[scalar]` local/param in the WASM subset"
                )));
            };
            if src_elem != elem {
                return Err(unsupported(&format!(
                    "list alias from `{src}` changes element type \
                     ({} vs {}) — the WASM subset shares records of one \
                     element type only",
                    src_elem.keyword(),
                    elem.keyword()
                )));
            }
            indent(out, depth);
            writeln!(out, "local.get ${src}").expect("write");
            Ok(())
        }
        other => Err(unsupported(&format!(
            "binding a list local from {} — the WASM subset materialises a \
             list LITERAL or shares another named list local/param \
             (list-returning calls/slices are refused)",
            expr_kind(other)
        ))),
    }
}

/// PMAT-1033: lower an [`Expr::ListLit`] (`[e0, e1, …]`) onto the bump heap
/// under the SAME length-prefixed ABI a `list[scalar]` param rides
/// (PMAT-968): `$__alloc(8 + n*elem_size)`, an `i32` element count at
/// `base+0`, packed elements from `base + LIST_ELEMS_OFFSET` — so the
/// existing bounds-guarded `Index`/`IndexAssign`/`Len` lowerings work on the
/// record verbatim. Leaves the `i32` base-pointer on the stack. Element
/// expressions type through the ordinary scalar paths; the record is
/// fixed-size (growth refuses — the PMAT-999 relocation posture).
fn emit_list_lit(
    elems: &[Expr],
    elem: WatTy,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let n = i32::try_from(elems.len())
        .map_err(|_| unsupported("list literal longer than i32::MAX elements"))?;
    let size = LIST_ELEMS_OFFSET + n * elem.byte_size();
    // dst = __alloc(8 + n*elem_size)
    indent(out, depth);
    writeln!(out, "i32.const {size}").expect("write");
    indent(out, depth);
    writeln!(out, "call $__alloc").expect("write");
    indent(out, depth);
    writeln!(out, "local.set ${LIST_DST_SCRATCH}").expect("write");
    // Header: the i32 element count at base+0.
    indent(out, depth);
    writeln!(out, "local.get ${LIST_DST_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {n}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store").expect("write");
    // Each element at base + LIST_ELEMS_OFFSET + i*elem_size.
    for (i, e) in elems.iter().enumerate() {
        indent(out, depth);
        writeln!(out, "local.get ${LIST_DST_SCRATCH}").expect("write");
        emit_expr_typed(e, scope, out, depth, elem)?;
        indent(out, depth);
        writeln!(
            out,
            "{}.store offset={}",
            elem.keyword(),
            LIST_ELEMS_OFFSET + i as i32 * elem.byte_size()
        )
        .expect("write");
    }
    indent(out, depth);
    writeln!(out, "local.get ${LIST_DST_SCRATCH}").expect("write");
    Ok(())
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

/// PMAT-1023: lower a `Stmt::FieldAssign` (`obj.field = value`) — a
/// `*.store` at the field's 8-byte-slot offset through the struct local/
/// param's base-pointer. This is the WASM OOP mutation primitive:
/// `self.count = self.count + 1` inside a method and `p.x = 99` outside
/// both lower here, and because every binding of the record holds the SAME
/// i32 base-pointer, the write is visible through every alias — Python's
/// reference semantics are native to linear memory (no clone/refuse
/// disposition; the Rust lane must refuse shapes this lane executes
/// exactly). The value is typed against the field's declared WAT type (an
/// int value into a float field is an honest type-mismatch refusal, not a
/// silent widening).
fn emit_field_assign(
    obj: &str,
    field: &str,
    value: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let sname = scope.struct_of(obj).ok_or_else(|| {
        unsupported(&format!(
            "field assignment over `{obj}` which is not a struct local/param in \
             the WASM subset"
        ))
    })?;
    let layout = struct_layout(scope.structs, &sname)?;
    let (idx, fty) = layout
        .iter()
        .enumerate()
        .find(|(_, (fn_, _))| fn_ == field)
        .map(|(i, (_, t))| (i, *t))
        .ok_or_else(|| unsupported(&format!("struct `{sname}` has no field `{field}`")))?;
    indent(out, depth);
    writeln!(out, "local.get ${obj}").expect("write");
    emit_expr_typed(value, scope, out, depth, fty)?;
    indent(out, depth);
    writeln!(
        out,
        "{}.store offset={}",
        fty.keyword(),
        idx as i32 * STRUCT_FIELD_SIZE
    )
    .expect("write");
    Ok(())
}

/// PMAT-1023: lower an `Expr::MethodCall` (`obj.method(args)`) — push the
/// receiver's base-pointer, the args (each typed against the method's
/// declared param), and `call $<Struct>.<method>`. Returns the method's
/// result WAT type, or `None` for a unit/void method (the caller decides
/// whether `None` is legal in its position: a value position refuses it, a
/// statement position emits nothing to drop).
fn emit_method_call(
    obj: &Expr,
    method: &str,
    args: &[Expr],
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<Option<WatTy>, BackendError> {
    let Expr::Ident(oname) = obj else {
        return Err(unsupported(&format!(
            "method call `.{method}(…)` over a non-name receiver — the WASM \
             subset calls methods on a struct LOCAL/PARAM only (no chained or \
             temporary receivers)"
        )));
    };
    let Some(sname) = scope.struct_of(oname) else {
        return Err(unsupported(&format!(
            "method call `.{method}(…)` over `{oname}` which is not a struct \
             local/param — non-struct method receivers are outside the WASM \
             subset"
        )));
    };
    let Some((ptys, ret)) = scope.method_sig(&sname, method) else {
        return Err(unsupported(&format!(
            "struct `{sname}` has no method `{method}` in this module (the WASM \
             subset lowers a method only alongside its class definition)"
        )));
    };
    if ptys.len() != args.len() {
        return Err(unsupported(&format!(
            "method `{sname}.{method}` takes {} argument(s) but the call passes {}",
            ptys.len(),
            args.len()
        )));
    }
    indent(out, depth);
    writeln!(out, "local.get ${oname}").expect("write");
    for (a, pt) in args.iter().zip(ptys.iter()) {
        emit_expr_typed(a, scope, out, depth, *pt)?;
    }
    indent(out, depth);
    writeln!(out, "call ${sname}.{method}").expect("write");
    Ok(ret)
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
    // PMAT-1032: `chr(n)` delegates to the `$__wasm_chr` helper — a NEW heap
    // string holding the full 1..4-byte UTF-8 encoding of code point `n`,
    // with the `0..=0x10FFFF` range trap (the Python ValueError analogue).
    // The pre-PMAT-1032 lowering masked `n & 0xFF` into a single byte:
    // SILENTLY wrong for every n > 127 (chr(233) was the bare byte 0xE9 —
    // not even valid UTF-8, internally inconsistent with the 2-byte literal
    // encoding of "é"). No scratch local: the helper owns its state.
    emit_expr_typed(value, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_chr").expect("write");
    Ok(())
}

/// PMAT-994 (slice 3a, char-exact since PMAT-1032): lower `s[i]` used AS a
/// 1-char string (`Expr::StrCharAt` outside an `ord`) — materialise a NEW
/// heap string holding CHAR `i` of the string-valued base (its full 1..4-byte
/// UTF-8 encoding), and leave its `i32` base-pointer on the stack.
///
/// Delegates to `$__wasm_str_char_at` (see [`STR_CHAR_ALLOC_HELPERS`]), which
/// owns the Python negative-index normalisation (`s[-1]` indexes from the
/// end), the bounds trap (`IndexError` analogue), and the char walk. Works
/// over ANY string-valued base — a str param/local, a string literal, or a
/// heap string — since all share the length-prefixed ABI. The pre-PMAT-1032
/// lowering copied one BYTE (char-correct only for ASCII, shredding
/// multi-byte chars) and trapped on negative indices.
///
/// Stack discipline replaces the old scratch-local dance: the base pointer
/// stays ON the WASM stack while the index evaluates (a stack value cannot be
/// clobbered by a nested string op the way [`STR_LA_SCRATCH`] could).
fn emit_str_char_at(
    string: &Expr,
    index: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    emit_str_expr(string, scope, out, depth)?;
    emit_expr_typed(index, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_str_char_at").expect("write");
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

/// PMAT-994: `true` if `e` is a string-VALUED binop operand — a str-name
/// `Ident` (param or PMAT-1028 local), a string literal, a `Concat`, a `Chr`,
/// a bare `StrCharAt`, or PMAT-1028 a call/method-call of a PROVEN
/// str-returning callable. Such an operand is an i32 base-pointer, NOT an
/// arithmetic/bool value; a `==`/`!=` over it routes to the content-compare
/// helper, any other op is refused.
fn binop_operand_is_string(e: &Expr, scope: &Scope) -> bool {
    match e {
        Expr::Ident(name) => scope.is_str_name(name),
        Expr::LitStr(_) | Expr::Concat { .. } | Expr::Chr { .. } | Expr::StrCharAt { .. } => true,
        // PMAT-1060: `str(int)` is an i32 heap-string pointer, not an arithmetic
        // value — a `==`/`!=` over it routes to the content-compare helper.
        Expr::ToStr {
            of_float: false, ..
        } => true,
        // PMAT-1142: `s * n` (a STRING repeat) is an i32 heap-string pointer, so
        // a `==`/`!=`/ordering over it routes to the content-compare helper.
        Expr::Repeat { of_str: true, .. } => true,
        Expr::Call { callee, .. } => scope.call_returns_str(callee),
        Expr::MethodCall { obj, method, .. } => matches!(obj.as_ref(), Expr::Ident(o)
            if scope.struct_of(o).is_some_and(|s| scope.method_returns_str(&s, method))),
        _ => false,
    }
}

/// PMAT-1023: `true` if `e` is a STRUCT-valued binop operand — a struct
/// local/param `Ident` or a `StructLit`. A struct rides an i32 base-pointer,
/// indistinguishable from a bool i32 in the opcode table, so a naive `p == q`
/// would silently compare POINTERS — while Python `==` over dataclasses is
/// STRUCTURAL (and identity `is` is not modeled). Refused honestly.
fn binop_operand_is_struct(e: &Expr, scope: &Scope) -> bool {
    match e {
        Expr::Ident(name) => scope.struct_of(name).is_some(),
        Expr::StructLit { .. } => true,
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
    // PMAT-986/994/1059: a `str` lowers to an `i32` base-pointer, INDISTINGUISHABLE
    // from a bool `i32` in the opcode table below — so a naive `a < b` over two
    // strings would silently compare BASE-POINTERS (wrong code). PMAT-994 wires
    // string EQUALITY (`a == b` / `a != b`) via a real content-compare helper
    // (`$__wasm_str_eq`); PMAT-1059 wires ORDERING (`<`/`<=`/`>`/`>=`) via a
    // byte-wise lexicographic 3-way compare (`$__wasm_str_cmp`). Arithmetic
    // (other than `Concat`'s `+`) / methods over strings stay refused.
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
        // PMAT-1059: string ORDERING — `a < b` / `a <= b` / `a > b` / `a >= b`
        // over two string-valued operands. Lower to `$__wasm_str_cmp(a, b)` (a
        // byte-wise lexicographic 3-way compare → i32 <0/0/>0), then compare the
        // result against 0 with the matching signed op. Byte order == code-point
        // order for UTF-8, so this IS Python's str ordering (never a
        // base-pointer compare). Same mixed-operand guard as equality: `str < int`
        // is refused.
        if matches!(op, BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq) {
            if !(binop_operand_is_string(lhs, scope) && binop_operand_is_string(rhs, scope)) {
                return Err(unsupported(&format!(
                    "binary op {op:?} mixing a `str` operand with a non-`str` \
                     operand — string ordering compares two strings; a mixed \
                     comparison is refused"
                )));
            }
            emit_str_expr(lhs, scope, out, depth)?;
            emit_str_expr(rhs, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_cmp").expect("write");
            indent(out, depth);
            writeln!(out, "i32.const 0").expect("write");
            let cmp = match op {
                BinOp::Lt => "i32.lt_s",
                BinOp::LtEq => "i32.le_s",
                BinOp::Gt => "i32.gt_s",
                BinOp::GtEq => "i32.ge_s",
                _ => unreachable!("guarded by the matches! above"),
            };
            indent(out, depth);
            writeln!(out, "{cmp}").expect("write");
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
            "binary op {op:?} over `str` operand(s) — string METHODS / other \
             ops are not in the WASM string subset (supported: read-only \
             `len(s)` + `ord(s[i])` + heap `Concat`/`chr`/`s[i]`/slice + \
             content equality `==`/`!=` + PMAT-1059 ordering `<`/`<=`/`>`/`>=`); \
             this op needs logic not yet wired, refused honestly rather than \
             comparing base-pointers"
        )));
    }

    // PMAT-1023: a struct operand rides an i32 base-pointer — every binop
    // over one is refused (equality would be pointer identity, not Python's
    // structural `==`; ordering/arithmetic over pointers is meaningless).
    if binop_operand_is_struct(lhs, scope) || binop_operand_is_struct(rhs, scope) {
        return Err(unsupported(&format!(
            "binary op {op:?} over struct operand(s) — a struct rides an i32 \
             base-pointer, so a naive compare would be POINTER identity while \
             Python `==` over classes/dataclasses is structural (and `is` \
             identity is not modeled). Struct equality/ordering is refused \
             honestly; compare individual scalar fields instead"
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
    // PMAT-1002: float `/` guards the divisor against 0.0 — Python raises
    // ZeroDivisionError, where a bare f64.div would silently return IEEE
    // inf/nan (1.0/0.0 → +inf, 0.0/0.0 → nan). Stash the divisor (top of stack)
    // so the dividend stays put, trap if it is 0.0 (`-0.0 == 0.0` in IEEE, so
    // both signed zeros are caught), then divide. Found by the PMAT-1002
    // adversarial CPython-differential sweep.
    if matches!(op, FloatOp::Div) {
        indent(out, depth);
        writeln!(out, "local.set ${FDIV_SCRATCH}").expect("write"); // pop divisor; dividend stays
        indent(out, depth);
        writeln!(out, "local.get ${FDIV_SCRATCH}").expect("write");
        indent(out, depth);
        writeln!(out, "f64.const 0.0").expect("write");
        indent(out, depth);
        writeln!(out, "f64.eq").expect("write");
        indent(out, depth);
        writeln!(out, "if").expect("write");
        indent(out, depth + 1);
        writeln!(out, "unreachable").expect("write"); // ZeroDivisionError analogue
        indent(out, depth);
        writeln!(out, "end").expect("write");
        indent(out, depth);
        writeln!(out, "local.get ${FDIV_SCRATCH}").expect("write");
        indent(out, depth);
        writeln!(out, "f64.div").expect("write");
        return Ok(WatTy::F64);
    }
    indent(out, depth);
    let instr = match op {
        FloatOp::Add => "f64.add",
        FloatOp::Sub => "f64.sub",
        FloatOp::Mul => "f64.mul",
        FloatOp::Div => unreachable!("Div handled above with the zero-divisor guard"),
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
        Expr::Repeat { .. } => "Repeat (seq * n)",
        Expr::LitStr(_) => "LitStr",
        Expr::ListLit(_) => "ListLit",
        Expr::DictLit(_) => "DictLit",
        Expr::SetLit(_) => "SetLit",
        Expr::TupleLit(_) => "TupleLit",
        Expr::Len(_) => "Len",
        Expr::Index { .. } => "Index",
        Expr::StructLit { .. } => "StructLit",
        Expr::MethodCall { .. } => "MethodCall",
        Expr::Block(_) => "Block",
        _ => "<container/aggregate/builtin expression>",
    }
}

#[cfg(test)]
mod tests;
