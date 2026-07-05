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
    BinOp, Block, Expr, FloatOp, Function, Item, ListMutateOp, ListQueryOp, Module, PairIterKind,
    Param, SetOp, SetPredOp, SortKey, Stmt, StrMethodOp, Type, UnOp,
};

mod wasm_diffexec;
pub use wasm_diffexec::{
    general_module_wat, wasm_runtime_available, WasmDiffExecEngine, FIXTURE_INPUT,
};

/// The Layer-5 compile contract every emitted WAT function cites.
const CONTRACT_ID: &str = "C-COMPILE-RUST-TO-WASM";

/// PMAT-956: the Layer-5 contract governing the WASM bump-heap allocator (a
/// strict extension of [`CONTRACT_ID`]). Cited — structurally and in-text —
/// whenever the emitted module allocates (`module_needs_heap`), so heap-using
/// output is not uncited at Layer 5.
const HEAP_CONTRACT_ID: &str = "C-WASM-HEAP";

/// PMAT-968 list ABI / PMAT-986 str ABI: a `list[scalar]` base-pointer
/// points at an `i32` element-count header at `base+0`; the packed elements
/// start at this byte offset. The offset is 8 (not 4) so every `i64`/`f64`
/// element stays naturally aligned for `i64.load`/`f64.load`. The PMAT-986
/// `str` ABI is byte-identical — an `i32` UTF-8 **byte count** at `base+0`,
/// the raw bytes from `base+8` — so a str shares this same constant (its
/// per-byte `i32.load8_u` access needs no alignment, but reusing the layout
/// keeps the single list/str linear-memory ABI uniform).
const LIST_ELEMS_OFFSET: i32 = 8;

/// PMAT-1276: the i32 slot-**capacity** header a `list[scalar]` carries at
/// `base+4` — the same byte the dict/set bump heap uses for its capacity
/// ([`DICT_CAP_OFFSET`]); a list previously left this word unused. `len(xs)`
/// reads the live-element COUNT at `base+0`; `xs.append(v)` reads the capacity
/// here to bound the write (an append at `count == capacity` traps rather than
/// overrunning the record). The read-only list ops touch only `base+0` and the
/// elements at `base+8`, so recording a capacity here is transparent to them.
const LIST_CAP_OFFSET: i32 = 4;

/// PMAT-1276: spare element slots a `ListLit` over-allocates past its literal
/// entries, so a subsequent `xs.append(v)` has room in the (realloc-free) bump
/// heap. Mirrors [`DICT_GROWTH_SLACK`] exactly: the capacity is FIXED at
/// construction (`literal_count + LIST_GROWTH_SLACK`), and an append beyond it
/// TRAPS (`unreachable`) rather than relocating the record — the honest
/// bounded-capacity posture that keeps append ALIAS-SAFE (the base-pointer
/// never moves, so every alias holding it still observes the mutation, unlike
/// the relocation hazard the PMAT-1033 growth-refusal warned about).
const LIST_GROWTH_SLACK: i32 = 16;

/// PMAT-968: name of the per-function scratch `i64` local that holds an
/// evaluated `Index` index, reused by the bounds guard and the address
/// computation (so the index expression is evaluated exactly once). Prefixed
/// with `__wasm` to avoid colliding with a user local — meta-HIR identifiers
/// from the supported frontends never start `__wasm`.
const IDX_SCRATCH: &str = "__wasm_idx";

/// PMAT-1290: the name prefix of the synthetic non-negative loop counter the
/// single-var `for … in …` desugar ([`desugar_foreach_stmts`]) binds. It is the
/// ONLY legitimate index into a set: a user-written `s[i]` (a Python set is not
/// subscriptable — `TypeError`) stays refused, so set element access exists
/// solely as the internal per-element read of set iteration. Shared by the
/// desugar (which mints `{FOREACH_IDX_PREFIX}{k}`) and [`is_foreach_counter`]
/// (which gates the set-index emit), so the coupling is explicit, not a magic
/// string. Prefixed with `__wasm` like [`IDX_SCRATCH`] — never a user local.
const FOREACH_IDX_PREFIX: &str = "__wasm_fe_i_";

/// PMAT-1290: `true` if `e` is a synthetic foreach loop counter (see
/// [`FOREACH_IDX_PREFIX`]) — the sole legitimate set index.
fn is_foreach_counter(e: &Expr) -> bool {
    matches!(e, Expr::Ident(n) if n.starts_with(FOREACH_IDX_PREFIX))
}

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
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => collect_expr_literals(elem, out),
        // PMAT-1282: `xs.insert(i, v)` — BOTH the index and the value can carry a
        // str literal (`xs.insert(len("hi"), "ab"[1:])`) that must be laid out.
        Stmt::ListInsert { index, elem, .. } => {
            collect_expr_literals(index, out);
            collect_expr_literals(elem, out);
        }
        // PMAT-1023: a field write's VALUE and a statement-position method
        // call's ARGS may reference literals (`c.tag(ord("x"))`).
        Stmt::FieldAssign { value, .. } => collect_expr_literals(value, out),
        Stmt::SideEffectCall { call } => collect_expr_literals(call, out),
        // PMAT-1234: `del d[k]` — the dict KEY can carry a str literal
        // (`del d["ab"]`) that must be laid out into a (data) segment, exactly
        // like the `DictSet` write-side arm above.
        Stmt::DelItem { key, .. } => collect_expr_literals(key, out),
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
        // PMAT-1223: `d.get(k, default)` also lays out any str literals in its
        // dict/key/default (e.g. a str key or a str-touching default).
        Expr::DictGetOr { dict, key, default } => {
            collect_expr_literals(dict, out);
            collect_expr_literals(key, out);
            collect_expr_literals(default, out);
        }
        // PMAT-1225: `d.pop(k[, default])` — recurse into dict/key/optional
        // default (a str key literal or a literal-hosting default must be laid
        // into the static data table just like `d.get(k, default)`'s).
        Expr::ListPop { list, index } => {
            collect_expr_literals(list, out);
            if let Some(index) = index {
                collect_expr_literals(pop_index_scan_expr(index), out);
            }
        }
        Expr::DictPop { dict, key, default } => {
            collect_expr_literals(dict, out);
            collect_expr_literals(key, out);
            if let Some(default) = default {
                collect_expr_literals(default, out);
            }
        }
        // PMAT-1227: `d.setdefault(k, default)` — lay out any str literals in its
        // dict/key/default (a str key or a str-touching default), same as
        // `d.get(k, default)`'s.
        Expr::DictSetDefault { dict, key, default } => {
            collect_expr_literals(dict, out);
            collect_expr_literals(key, out);
            collect_expr_literals(default, out);
        }
        Expr::SetContains { set, elem } => {
            collect_expr_literals(set, out);
            collect_expr_literals(elem, out);
        }
        // PMAT-1262: `x in xs` — the list is a bare NAME (no literal to collect)
        // but the needle may host a str literal (`len("ab") in xs`), so recurse.
        Expr::ListContains { list, elem } => {
            collect_expr_literals(list, out);
            collect_expr_literals(elem, out);
        }
        // PMAT-1274: `xs.count(v)`/`xs.index(v)` — the list is a bare NAME but the
        // needle may host a str literal (`xs.count(len("ab"))`), so recurse.
        Expr::ListQuery { list, arg, .. } => {
            collect_expr_literals(list, out);
            collect_expr_literals(arg, out);
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

/// PMAT-1209: `$__wasm_str_pad(s, w, mode) -> i32` — the shared kernel for Python
/// `s.rjust(width)` (`mode` = 0), `s.ljust(width)` (`mode` = 1), and
/// `s.center(width)` (`mode` = 2): a NEW heap string equal to `s` padded with
/// ASCII space (`0x20`) to `width` CODE POINTS. Allocating (rides `needs_heap`,
/// calls `$__alloc`). Calls `$__wasm_str_charlen` (co-emitted for any str-touching
/// module) for the width math.
///
/// The total pad is `max(0, width - charlen(s))` — a `width` no larger than the
/// current code-point length is a plain COPY of `s` (`"ab".rjust(1)` == `"ab"`,
/// `"".ljust(0)` == `""`); a negative/overflow `width` wraps then clamps, so it
/// also copies. The pad splits by `mode`: rjust puts it all on the LEFT, ljust all
/// on the RIGHT, and center splits it with CPython's exact parity bias
/// `left = pad/2 + (pad & width & 1)` (so `"ab".center(5)` == `"  ab "`, matching
/// CPython's left-heavy-on-odd-width rule, NOT Rust `{:^}`'s right-bias),
/// `right = pad - left`.
///
/// **Char-exact for ANY valid UTF-8, no trap (like zfill, unlike upper/title).**
/// The pad bytes are pure 1-byte ASCII spaces inserted at code-point boundaries
/// (the very start and/or very end), and `s` is copied byte-for-byte, so no payload
/// byte is ever inspected or folded — `"café".rjust(6)` == `"  café"` and
/// `"é".center(3)` == `" é "` are byte-exact. `memory.fill` of 0 bytes and
/// `memory.copy` of 0 bytes are nops, so the `pad == 0` copy and the
/// empty-`s`/one-sided-pad boundaries fall out of the general path with no special
/// case.
const STR_PAD_HELPER: &str = "\
  ;; PMAT-1209 __wasm_str_pad(s, w, mode) = Python s.rjust(w) (mode 0) /
  ;; s.ljust(w) (mode 1) / s.center(w) (mode 2) — a NEW heap string padded with
  ;; ASCII space to `w` CODE POINTS. pad = max(0, w - charlen(s)); the space bytes
  ;; land on code-point boundaries and s is a byte copy, so it is char-exact for any
  ;; UTF-8 (no trap). center bias = CPython left = pad/2 + (pad & w & 1).
  (func $__wasm_str_pad (param $s i32) (param $w i64) (param $mode i32) (result i32)
    (local $slen i32)
    (local $n i32)
    (local $pad i32)
    (local $lpad i32)
    (local $rpad i32)
    (local $rlen i32)
    (local $dst i32)
    (local $wpos i32)
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
    ;; split pad by mode. default lpad = rpad = 0 (the pad == 0 copy).
    i32.const 0
    local.set $lpad
    i32.const 0
    local.set $rpad
    ;; mode 0 (rjust): all pad on the LEFT.
    local.get $mode
    i32.eqz
    if
      local.get $pad
      local.set $lpad
    end
    ;; mode 1 (ljust): all pad on the RIGHT.
    local.get $mode
    i32.const 1
    i32.eq
    if
      local.get $pad
      local.set $rpad
    end
    ;; mode 2 (center): lpad = pad/2 + (pad & wrap(w) & 1) ; rpad = pad - lpad.
    local.get $mode
    i32.const 2
    i32.eq
    if
      local.get $pad
      i32.const 1
      i32.shr_u
      local.get $pad
      local.get $w
      i32.wrap_i64
      i32.and
      i32.const 1
      i32.and
      i32.add
      local.set $lpad
      local.get $pad
      local.get $lpad
      i32.sub
      local.set $rpad
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
    ;; fill `lpad` ' ' (0x20) bytes at wpos ; wpos += lpad. (nop when lpad == 0.)
    local.get $wpos
    i32.const 0x20
    local.get $lpad
    memory.fill
    local.get $wpos
    local.get $lpad
    i32.add
    local.set $wpos
    ;; copy the `slen` source bytes (from s+8) to wpos ; wpos += slen. (nop when
    ;; slen == 0.)
    local.get $wpos
    local.get $s
    i32.const 8
    i32.add
    local.get $slen
    memory.copy
    local.get $wpos
    local.get $slen
    i32.add
    local.set $wpos
    ;; fill `rpad` ' ' (0x20) bytes at wpos. (nop when rpad == 0.)
    local.get $wpos
    i32.const 0x20
    local.get $rpad
    memory.fill
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

/// PMAT-1187: `$__wasm_str_capitalize(s) -> i32` — Python `s.capitalize()`: a NEW
/// heap string whose FIRST ASCII letter is upper-cased and every REMAINING ASCII
/// letter is lower-cased (`"heLLo".capitalize() == "Hello"`, `"".capitalize() ==
/// ""`). Allocating (rides the `needs_heap` gate, calls `$__alloc`).
///
/// **ASCII-only, with the same honest runtime boundary as `$__wasm_str_upper_
/// lower`.** Python's `str.capitalize()` does FULL Unicode case mapping (title-case
/// the first, lower-fold the rest), which needs a case table this scalar lane does
/// not carry. So the helper case-flips only the ASCII letters and, on the FIRST
/// byte `>= 0x80` (any byte of a non-ASCII code point in valid UTF-8), executes
/// `unreachable` — a TRAP, exactly like the `upper` / `lower` / `index` siblings.
/// It NEVER passes a non-ASCII byte through unchanged, so it never silently
/// diverges: a pure-ASCII `s` is char-exact, any non-ASCII `s` aborts. Because
/// every surviving byte is 1-byte ASCII, byte length == code-point length == the
/// result length, so `len` / `Concat` / a str RETURN compose uniformly.
///
/// One pass over the payload: `$__alloc(8 + slen)`, store the i32 BYTE-count header
/// (= `slen`, unchanged by case flipping), then for each byte: trap if `>= 0x80`,
/// else at `i == 0` upper-flip an `a`–`z` (subtract `0x20`) and at `i > 0` lower-
/// flip an `A`–`Z` (add `0x20`), and store it. A zero-length `s` writes no payload
/// (the loop guard is `i < slen`) and returns an empty heap string.
const STR_CAPITALIZE_HELPER: &str = "\
  ;; PMAT-1187 __wasm_str_capitalize(s) = Python s.capitalize() — a NEW heap string
  ;; with the FIRST ASCII letter upper-cased and every REMAINING ASCII letter
  ;; lower-cased. ASCII-only: a byte >= 0x80 (non-ASCII code point) TRAPS
  ;; (unreachable), never a silent un-folded pass-through, so the result is
  ;; char-exact for ASCII or aborts.
  (func $__wasm_str_capitalize (param $s i32) (result i32)
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
        ;; i == 0 (first char): upper-flip 'a'(0x61)..'z'(0x7a) -> c - 0x20
        local.get $i
        i32.eqz
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
          ;; i > 0 (rest): lower-flip 'A'(0x41)..'Z'(0x5a) -> c + 0x20
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

/// PMAT-1201: `$__wasm_str_swapcase(s) -> i32` — Python `s.swapcase()`: a NEW heap
/// string with the case of every ASCII letter flipped (`A`–`Z` → `a`–`z` AND
/// `a`–`z` → `A`–`Z`, in a single pass — the both-directions twin of
/// [`STR_UPPER_LOWER_HELPER`], which flips only ONE direction per `up` flag).
/// Allocating (rides the `needs_heap` gate, calls `$__alloc`).
///
/// **ASCII-only, with the same honest runtime boundary as `$__wasm_str_upper_
/// lower` / `$__wasm_str_capitalize`.** Python's `str.swapcase()` does FULL
/// Unicode case flipping (`"ß".swapcase() == "SS"`, `"É".swapcase() == "é"`),
/// which needs a case table this scalar lane does not carry. So the helper
/// case-flips only the ASCII letters and, on the FIRST byte `>= 0x80` (any byte
/// of a non-ASCII code point in valid UTF-8), executes `unreachable` — a TRAP,
/// exactly like the `upper` / `lower` / `capitalize` siblings. It NEVER passes a
/// non-ASCII byte through unchanged, so it never silently diverges from CPython:
/// for a pure-ASCII `s` the result is char-exact, and for any non-ASCII `s` it
/// traps rather than returning a wrongly-flipped string. Because every surviving
/// byte is 1-byte ASCII, byte length == code-point length == the result length,
/// so `len` / `Concat` / equality / a str RETURN compose uniformly.
///
/// One byte-parallel pass: `$__alloc(8 + slen)`, store the i32 BYTE-count header
/// (= `slen`, unchanged by case flipping), then for each payload byte: trap if
/// `>= 0x80`, else flip `'a'`–`'z'` UP (`c - 0x20`) OR `'A'`–`'Z'` DOWN
/// (`c + 0x20`) — a non-letter byte is stored unchanged. A zero-length `s` writes
/// no payload (the loop guard is `i < slen`) and returns an empty heap string.
const STR_SWAPCASE_HELPER: &str = "\
  ;; PMAT-1201 __wasm_str_swapcase(s) = Python s.swapcase() — a NEW heap string with
  ;; the case of every ASCII letter flipped BOTH ways ('a'..'z' -> upper AND
  ;; 'A'..'Z' -> lower) in one pass. ASCII-only: a byte >= 0x80 (non-ASCII code
  ;; point) TRAPS (unreachable), never a silent un-flipped pass-through, so the
  ;; result is char-exact for ASCII or aborts.
  (func $__wasm_str_swapcase (param $s i32) (result i32)
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
        ;; c in 'a'(0x61)..'z'(0x7a) (lowercase) -> upper: c - 0x20
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
        else
          ;; c in 'A'(0x41)..'Z'(0x5a) (uppercase) -> lower: c + 0x20
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

/// PMAT-1203: `$__wasm_str_title(s) -> i32` — Python `s.title()`: a NEW heap
/// string title-cased word-by-word — the FIRST ASCII letter of each word is
/// upper-cased and every REMAINING letter of the word is lower-cased, where any
/// NON-ALPHABETIC character (space, digit, `_`, punctuation) is a word boundary
/// (`"hello world".title() == "Hello World"`, `"it's".title() == "It'S"`,
/// `"a1b2".title() == "A1B2"` — a digit resets the word, so the letter after it
/// re-capitalises). Allocating (rides the `needs_heap` gate, calls `$__alloc`).
///
/// **Stateful, unlike the byte-parallel `$__wasm_str_swapcase`.** Title-casing is
/// NOT a per-byte function of the byte alone — it depends on whether the PREVIOUS
/// character was a cased (ASCII-letter) character. The helper carries a `$prev`
/// flag (`1` iff the last byte was an ASCII letter): a letter reached with
/// `$prev == 0` (word start) is upper-cased, a letter reached with `$prev == 1`
/// (mid-word) is lower-cased, and any non-letter passes through unchanged AND
/// clears `$prev`. This is exactly CPython's ASCII `do_title` loop, where
/// `is_cased == is_letter` for ASCII (so `"it's".title()` re-capitalises the `s`
/// after the un-cased `'`).
///
/// **ASCII-only, with the same honest runtime boundary as the case-fold siblings
/// (`upper`/`lower`/`capitalize`/`swapcase`).** Python's `str.title()` does FULL
/// Unicode title mapping, which needs a case table this scalar lane does not
/// carry. So the helper title-cases only the ASCII letters and, on the FIRST byte
/// `>= 0x80` (any byte of a non-ASCII code point in valid UTF-8), executes
/// `unreachable` — a TRAP, exactly like the case-fold siblings. It NEVER passes a
/// non-ASCII byte through unchanged, so it never silently diverges: a pure-ASCII
/// `s` is char-exact, any non-ASCII `s` aborts. Because every surviving byte is
/// 1-byte ASCII, byte length == code-point length == the result length, so `len` /
/// `Concat` / equality / a str RETURN compose uniformly.
///
/// One pass over the payload: `$__alloc(8 + slen)`, store the i32 BYTE-count header
/// (= `slen`, unchanged by case flipping), `$prev = 0`, then for each byte: trap if
/// `>= 0x80`; compute `$isU` (`'A'`–`'Z'`) and `$isL` (`'a'`–`'z'`); if it is a
/// letter, lower-flip when `$prev` else upper-flip, and set `$prev = 1`; else store
/// it unchanged and clear `$prev = 0`. A zero-length `s` writes no payload (the loop
/// guard is `i < slen`) and returns an empty heap string.
const STR_TITLE_HELPER: &str = "\
  ;; PMAT-1203 __wasm_str_title(s) = Python s.title() — a NEW heap string title-cased
  ;; word-by-word: the first ASCII letter of each word upper-cased, the rest of the
  ;; word lower-cased, any NON-letter a word boundary (resets $prev). Stateful (not a
  ;; per-byte fn): $prev = 1 iff the last byte was an ASCII letter. ASCII-only: a byte
  ;; >= 0x80 (non-ASCII code point) TRAPS (unreachable), never a silent un-titled
  ;; pass-through, so the result is char-exact for ASCII or aborts.
  (func $__wasm_str_title (param $s i32) (result i32)
    (local $slen i32)
    (local $dst i32)
    (local $i i32)
    (local $c i32)
    (local $prev i32)
    (local $isU i32)
    (local $isL i32)
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
    ;; prev = 0 (the first letter begins a word -> upper-cased).
    i32.const 0
    local.set $prev
    ;; for i in 0..slen: c = s[8+i]; trap if non-ASCII; title-flip; dst[8+i] = c.
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
        ;; isU = c in 'A'(0x41)..'Z'(0x5a)
        local.get $c
        i32.const 0x41
        i32.ge_u
        local.get $c
        i32.const 0x5a
        i32.le_u
        i32.and
        local.set $isU
        ;; isL = c in 'a'(0x61)..'z'(0x7a)
        local.get $c
        i32.const 0x61
        i32.ge_u
        local.get $c
        i32.const 0x7a
        i32.le_u
        i32.and
        local.set $isL
        ;; if letter (isU || isL): case depends on $prev; else non-letter boundary.
        local.get $isU
        local.get $isL
        i32.or
        if
          local.get $prev
          if
            ;; mid-word -> lower: uppercase letter (isU) gets +0x20
            local.get $isU
            if
              local.get $c
              i32.const 0x20
              i32.add
              local.set $c
            end
          else
            ;; word start -> upper: lowercase letter (isL) gets -0x20
            local.get $isL
            if
              local.get $c
              i32.const 0x20
              i32.sub
              local.set $c
            end
          end
          ;; this char is cased -> next letter is mid-word
          i32.const 1
          local.set $prev
        else
          ;; non-letter -> word boundary, next letter re-capitalises
          i32.const 0
          local.set $prev
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

/// PMAT-1205: `$__wasm_str_strip(s, left, right) -> i32` — Python `s.strip()`
/// (`left`=1, `right`=1) / `s.lstrip()` (`1`,`0`) / `s.rstrip()` (`0`,`1`): a NEW
/// heap string with the leading (`left`) and/or trailing (`right`) run of ASCII
/// whitespace removed, the retained byte range copied verbatim. Allocating (rides
/// the `needs_heap` gate, calls `$__alloc`). One helper serves all three (the
/// `left` / `right` i32 flags select which ends to trim), exactly like the shared
/// `$__wasm_str_upper_lower` `up` flag.
///
/// **Whitespace set = the isspace-family ASCII set** `(0x09..=0x0D) | (0x1C..=0x20)`
/// (tab/LF/VT/FF/CR, FS/GS/RS/US, space) — CPython's `str.strip()` and
/// `str.isspace()` share `Py_UNICODE_ISSPACE`, and the Rust/Ruchy lanes emit the
/// same set (`char::is_whitespace() || '\u{1c}'..='\u{1f}'`), so this is
/// byte-exact against CPython for ASCII.
///
/// **Boundary-only ASCII trap — MORE capable than the whole-string case-fold
/// posture.** The scans only ever READ the leading/trailing bytes they are
/// deciding whitespace-ness for. A read byte `< 0x80` that is NOT whitespace is a
/// definitive CONTENT boundary — the scan stops (correct). A read byte `>= 0x80`
/// (any byte of a non-ASCII code point) is UNDECIDABLE — it could be the lead of a
/// Unicode whitespace char CPython would strip (`" "`) or the lead/tail of a
/// non-whitespace char it would keep (`"é"`), and this scalar lane carries no
/// Unicode table — so it executes `unreachable` (a TRAP, like the case-fold
/// siblings), NEVER a silent wrong answer. INTERIOR bytes are copied verbatim and
/// never examined, so an interior non-ASCII char with ASCII ends does NOT trap
/// (`"a€b".strip() == "a€b"` is byte-exact). On any non-trapping run the
/// result is byte-exact with CPython: every boundary byte read was ASCII, so the
/// stop points are exactly Python's strip points, and the last retained char is
/// 1-byte ASCII (a multi-byte trailing char would have trapped) — so byte-len ==
/// code-point-len over the survivors and len/Concat/equality/a str RETURN compose
/// uniformly.
///
/// `left`/`right` guard their own scan, so `lstrip` never reads the tail (a
/// trailing non-ASCII byte cannot make `"x€".lstrip() == "x€"` trap) and `rstrip`
/// never reads the head. An all-whitespace `s` (`"   ".strip()`) yields the empty
/// string (`start` meets `end`); `memory.copy` of 0 bytes is a nop.
const STR_STRIP_HELPER: &str = "\
  ;; PMAT-1205 __wasm_str_strip(s, left, right) = Python s.strip()/.lstrip()/.rstrip()
  ;; — a NEW heap string with the leading (left) and/or trailing (right) run of ASCII
  ;; whitespace (0x09-0x0d | 0x1c-0x20) removed, the retained byte range copied
  ;; verbatim. Boundary-only ASCII: a non-ASCII (>= 0x80) BOUNDARY byte is undecidable
  ;; (could be Unicode whitespace) -> unreachable (trap); interior bytes are never
  ;; examined, so ASCII-ended strings with interior non-ASCII are byte-exact.
  (func $__wasm_str_strip (param $s i32) (param $left i32) (param $right i32) (result i32)
    (local $slen i32)
    (local $start i32)
    (local $end i32)
    (local $c i32)
    (local $rlen i32)
    (local $dst i32)
    ;; slen = byte length of s ; start = 0 ; end = slen.
    local.get $s
    i32.load
    local.set $slen
    i32.const 0
    local.set $start
    local.get $slen
    local.set $end
    ;; if left: advance `start` past the leading ASCII-whitespace run.
    local.get $left
    if
      block $ldone
        loop $lloop
          ;; stop if start >= end (empty / all whitespace).
          local.get $start
          local.get $end
          i32.ge_s
          br_if $ldone
          ;; c = s[8 + start]
          local.get $s
          i32.const 8
          i32.add
          local.get $start
          i32.add
          i32.load8_u
          local.set $c
          ;; non-ASCII boundary byte (>= 0x80) -> undecidable -> trap.
          local.get $c
          i32.const 0x80
          i32.ge_u
          if
            unreachable
          end
          ;; is_ws = (0x09 <= c <= 0x0d) | (0x1c <= c <= 0x20).
          local.get $c
          i32.const 0x09
          i32.ge_u
          local.get $c
          i32.const 0x0d
          i32.le_u
          i32.and
          local.get $c
          i32.const 0x1c
          i32.ge_u
          local.get $c
          i32.const 0x20
          i32.le_u
          i32.and
          i32.or
          ;; a definitively non-whitespace ASCII byte -> content boundary -> stop.
          i32.eqz
          br_if $ldone
          ;; whitespace -> start += 1.
          local.get $start
          i32.const 1
          i32.add
          local.set $start
          br $lloop
        end
      end
    end
    ;; if right: retreat `end` past the trailing ASCII-whitespace run.
    local.get $right
    if
      block $rdone
        loop $rloop
          ;; stop if end <= start (nothing left to trim).
          local.get $end
          local.get $start
          i32.le_s
          br_if $rdone
          ;; c = s[8 + end - 1]
          local.get $s
          i32.const 8
          i32.add
          local.get $end
          i32.add
          i32.const 1
          i32.sub
          i32.load8_u
          local.set $c
          ;; non-ASCII boundary byte (>= 0x80) -> undecidable -> trap.
          local.get $c
          i32.const 0x80
          i32.ge_u
          if
            unreachable
          end
          ;; is_ws = (0x09 <= c <= 0x0d) | (0x1c <= c <= 0x20).
          local.get $c
          i32.const 0x09
          i32.ge_u
          local.get $c
          i32.const 0x0d
          i32.le_u
          i32.and
          local.get $c
          i32.const 0x1c
          i32.ge_u
          local.get $c
          i32.const 0x20
          i32.le_u
          i32.and
          i32.or
          ;; a definitively non-whitespace ASCII byte -> content boundary -> stop.
          i32.eqz
          br_if $rdone
          ;; whitespace -> end -= 1.
          local.get $end
          i32.const 1
          i32.sub
          local.set $end
          br $rloop
        end
      end
    end
    ;; rlen = end - start (>= 0) ; dst = alloc(8 + rlen) ; header = rlen.
    local.get $end
    local.get $start
    i32.sub
    local.set $rlen
    local.get $rlen
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $rlen
    i32.store
    ;; copy rlen bytes from s+8+start to dst+8. (nop when rlen == 0.)
    local.get $dst
    i32.const 8
    i32.add
    local.get $s
    i32.const 8
    i32.add
    local.get $start
    i32.add
    local.get $rlen
    memory.copy
    local.get $dst
  )
";

/// PMAT-1213: `$__wasm_str_reverse(s) -> i32` — Python `s[::-1]`: a NEW heap string
/// with the CODE POINTS of `s` in reverse order. Allocating (rides the `needs_heap`
/// gate, calls `$__alloc`).
///
/// **UTF-8-aware and CHAR-EXACT with NO trap arm — strictly stronger than the
/// case-fold family.** Unlike `upper`/`lower`/`title`/`swapcase` (which need a
/// Unicode case table and TRAP on a non-ASCII byte), reversing by code point needs
/// NO table: the UTF-8 lead byte alone gives each code point's byte length (1 for
/// `< 0x80`, 2 for `0xC0`–`0xDF`, 3 for `0xE0`–`0xEF`, 4 for `>= 0xF0`), so the
/// helper copies each code point as an INTACT unit to a descending output position.
/// A multi-byte code point is moved WHOLE (its bytes kept in order), never
/// byte-reversed — which would corrupt its encoding. So the result is char-exact for
/// ANY valid UTF-8 (`"café"[::-1] == "éfac"`), matching CPython's code-point reversal
/// AND the rust / ruchy `.chars().rev().collect::<String>()` lane, with no runtime
/// refusal. (A stray `0x80`–`0xBF` continuation byte as a lead — which valid UTF-8
/// never produces — is copied as a 1-byte unit, a defensive no-overrun default.)
///
/// Reversal preserves the total byte count (every input byte belongs to exactly one
/// code point, so Σ lengths == `slen`), so the result header == the input header and
/// the descending write cursor lands exactly at 0. One pass over the payload:
/// `$__alloc(8 + slen)`, store the i32 BYTE-count header (= `slen`), then for each
/// code point (`i` steps by its lead-byte length `l`) `memory.copy` its `l` bytes to
/// `dst[8 + (outpos -= l)]`. A zero-length `s` writes no payload (the loop guard is
/// `i < slen`) and returns an empty heap string.
const STR_REVERSE_HELPER: &str = "\
  ;; PMAT-1213 __wasm_str_reverse(s) = Python s[::-1] — a NEW heap string with the
  ;; CODE POINTS of s in reverse order. UTF-8-aware and CHAR-EXACT with NO trap arm:
  ;; each code point (1-4 bytes, identified by its UTF-8 lead byte) is copied as an
  ;; intact unit to a descending output position, so a multi-byte code point moves
  ;; WHOLE (never byte-reversed, which would corrupt its encoding). Reversal preserves
  ;; the total byte count, so the result header == the input header. Matches CPython's
  ;; code-point reversal and the rust/ruchy `.chars().rev()` lane on ALL valid UTF-8.
  (func $__wasm_str_reverse (param $s i32) (result i32)
    (local $slen i32)
    (local $dst i32)
    (local $i i32)
    (local $outpos i32)
    (local $b i32)
    (local $l i32)
    ;; slen = byte length of s (unchanged by reversal).
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
    ;; i = 0 (input read cursor) ; outpos = slen (output write END, descends by l).
    i32.const 0
    local.set $i
    local.get $slen
    local.set $outpos
    block $done
      loop $loop
        ;; while i < slen
        local.get $i
        local.get $slen
        i32.ge_u
        br_if $done
        ;; b = lead byte s[8 + i]
        local.get $s
        i32.const 8
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        local.set $b
        ;; l = UTF-8 code-point byte length from the lead byte b. Default 1 (ASCII
        ;; b < 0x80 AND any stray 0x80-0xBF continuation byte, which valid UTF-8 never
        ;; leads with — copying it as 1 byte avoids an overrun on malformed input).
        i32.const 1
        local.set $l
        ;; 0xC0 <= b < 0xE0 -> 2-byte code point
        local.get $b
        i32.const 0xc0
        i32.ge_u
        local.get $b
        i32.const 0xe0
        i32.lt_u
        i32.and
        if
          i32.const 2
          local.set $l
        end
        ;; 0xE0 <= b < 0xF0 -> 3-byte code point
        local.get $b
        i32.const 0xe0
        i32.ge_u
        local.get $b
        i32.const 0xf0
        i32.lt_u
        i32.and
        if
          i32.const 3
          local.set $l
        end
        ;; b >= 0xF0 -> 4-byte code point
        local.get $b
        i32.const 0xf0
        i32.ge_u
        if
          i32.const 4
          local.set $l
        end
        ;; outpos -= l
        local.get $outpos
        local.get $l
        i32.sub
        local.set $outpos
        ;; copy the l bytes of the code point (kept in order) to dst[8 + outpos ..]:
        ;;   memory.copy  dest = dst+8+outpos  src = s+8+i  len = l
        local.get $dst
        i32.const 8
        i32.add
        local.get $outpos
        i32.add
        local.get $s
        i32.const 8
        i32.add
        local.get $i
        i32.add
        local.get $l
        memory.copy
        ;; i += l
        local.get $i
        local.get $l
        i32.add
        local.set $i
        br $loop
      end
    end
    local.get $dst
  )
";

/// PMAT-1219: `$__wasm_str_expandtabs(s, ts) -> i32` — Python
/// `s.expandtabs(tabsize)`: a NEW heap string with each tab (`\t`, `0x09`)
/// replaced by the ASCII spaces (`0x20`) needed to reach the next multiple of
/// `tabsize`, the COLUMN counted in **code points** and reset to `0` after each
/// `\n` (`0x0a`) or `\r` (`0x0d`). Allocating (rides the `needs_heap` gate, calls
/// `$__alloc`).
///
/// **Char-exact with NO trap arm — like reverse, unlike the case-fold family.**
/// Only the ASCII tab/newline bytes are ever interpreted; every other code point
/// (identified by its UTF-8 lead byte length `l`, as in `$__wasm_str_reverse`) is
/// copied VERBATIM and counts as ONE column, so a multibyte payload round-trips
/// unchanged (`"é\t".expandtabs(4)` → `"é   "`, `"日本\tx".expandtabs(4)` →
/// `"日本  x"`), matching CPython and the rust/ruchy `.chars()` walk — no Unicode
/// table, no non-ASCII trap. A lone continuation byte (`0x80`–`0xBF`, which valid
/// UTF-8 never leads with) is copied as a 1-byte unit, the same defensive default
/// as reverse.
///
/// **`tabsize <= 0` drops tabs (0 spaces), matching CPython** (`"a\tb".expandtabs(0)`
/// → `"ab"`): the per-tab space count guards `tsize > 0` before the
/// `col mod tsize` (so no divide-by-zero) and adds nothing when it fails. The
/// tabsize is `i32.wrap_i64`'d like the pad family's width — a realistic small
/// tabsize is exact; an out-of-i32-range value wraps then clamps.
///
/// The output byte length is not known a priori (tabs expand), so the helper makes
/// TWO passes over the payload with the identical column arithmetic: pass 1 sums
/// the output byte length `rlen`; pass 2 `$__alloc(8 + rlen)`, stores the header,
/// and fills the payload (`memory.fill` spaces for a tab, `i32.store8` the byte for
/// a newline/CR, `memory.copy` the `l` code-point bytes otherwise). A zero-length
/// `s` sizes to `rlen == 0` and returns an empty heap string.
const STR_EXPANDTABS_HELPER: &str = "\
  ;; PMAT-1219 __wasm_str_expandtabs(s, ts) = Python s.expandtabs(ts) — a NEW heap
  ;; string with each tab expanded to spaces to the next multiple of ts, the COLUMN
  ;; counted in CODE POINTS and reset on \\n/\\r. Char-exact with NO trap arm: only
  ;; the ASCII tab/newline bytes are interpreted; every other code point (length l
  ;; from its UTF-8 lead byte) is copied verbatim and counts as one column. ts<=0
  ;; drops tabs. Two passes (pass 1 sizes rlen, pass 2 fills) share the column math.
  (func $__wasm_str_expandtabs (param $s i32) (param $ts i64) (result i32)
    (local $slen i32)
    (local $tsize i32)
    (local $rlen i32)
    (local $i i32)
    (local $col i32)
    (local $b i32)
    (local $l i32)
    (local $k i32)
    (local $dst i32)
    (local $wpos i32)
    ;; slen = byte length of s ; tsize = wrap(ts) (i32; ts<=0 handled per tab).
    local.get $s
    i32.load
    local.set $slen
    local.get $ts
    i32.wrap_i64
    local.set $tsize
    ;; ---- pass 1: rlen = output byte length ----
    i32.const 0
    local.set $rlen
    i32.const 0
    local.set $i
    i32.const 0
    local.set $col
    block $d1
      loop $l1
        local.get $i
        local.get $slen
        i32.ge_u
        br_if $d1
        ;; b = lead byte s[8 + i].
        local.get $s
        i32.const 8
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        local.set $b
        ;; l = UTF-8 code-point byte length from b (default 1; same table as reverse).
        i32.const 1
        local.set $l
        local.get $b
        i32.const 0xc0
        i32.ge_u
        local.get $b
        i32.const 0xe0
        i32.lt_u
        i32.and
        if
          i32.const 2
          local.set $l
        end
        local.get $b
        i32.const 0xe0
        i32.ge_u
        local.get $b
        i32.const 0xf0
        i32.lt_u
        i32.and
        if
          i32.const 3
          local.set $l
        end
        local.get $b
        i32.const 0xf0
        i32.ge_u
        if
          i32.const 4
          local.set $l
        end
        ;; classify b (only 1-byte ASCII bytes can be \\t/\\n/\\r).
        local.get $b
        i32.const 0x09
        i32.eq
        if
          ;; tab: if tsize > 0, k = tsize - (col mod tsize); col += k; rlen += k.
          local.get $tsize
          i32.const 0
          i32.gt_s
          if
            local.get $tsize
            local.get $col
            local.get $tsize
            i32.rem_u
            i32.sub
            local.set $k
            local.get $col
            local.get $k
            i32.add
            local.set $col
            local.get $rlen
            local.get $k
            i32.add
            local.set $rlen
          end
        else
          local.get $b
          i32.const 0x0a
          i32.eq
          local.get $b
          i32.const 0x0d
          i32.eq
          i32.or
          if
            ;; newline / CR: 1 byte, col reset to 0.
            local.get $rlen
            i32.const 1
            i32.add
            local.set $rlen
            i32.const 0
            local.set $col
          else
            ;; ordinary code point: l bytes, col += 1.
            local.get $rlen
            local.get $l
            i32.add
            local.set $rlen
            local.get $col
            i32.const 1
            i32.add
            local.set $col
          end
        end
        ;; i += l
        local.get $i
        local.get $l
        i32.add
        local.set $i
        br $l1
      end
    end
    ;; ---- allocate: dst = alloc(8 + rlen) ; store the i32 header = rlen ----
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
    ;; ---- pass 2: fill the payload (same column math) ----
    i32.const 0
    local.set $i
    i32.const 0
    local.set $col
    block $d2
      loop $l2
        local.get $i
        local.get $slen
        i32.ge_u
        br_if $d2
        ;; b = lead byte s[8 + i].
        local.get $s
        i32.const 8
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        local.set $b
        ;; l = UTF-8 code-point byte length from b (default 1).
        i32.const 1
        local.set $l
        local.get $b
        i32.const 0xc0
        i32.ge_u
        local.get $b
        i32.const 0xe0
        i32.lt_u
        i32.and
        if
          i32.const 2
          local.set $l
        end
        local.get $b
        i32.const 0xe0
        i32.ge_u
        local.get $b
        i32.const 0xf0
        i32.lt_u
        i32.and
        if
          i32.const 3
          local.set $l
        end
        local.get $b
        i32.const 0xf0
        i32.ge_u
        if
          i32.const 4
          local.set $l
        end
        local.get $b
        i32.const 0x09
        i32.eq
        if
          ;; tab: if tsize > 0, fill k spaces at wpos; wpos += k; col += k.
          local.get $tsize
          i32.const 0
          i32.gt_s
          if
            local.get $tsize
            local.get $col
            local.get $tsize
            i32.rem_u
            i32.sub
            local.set $k
            local.get $wpos
            i32.const 0x20
            local.get $k
            memory.fill
            local.get $wpos
            local.get $k
            i32.add
            local.set $wpos
            local.get $col
            local.get $k
            i32.add
            local.set $col
          end
        else
          local.get $b
          i32.const 0x0a
          i32.eq
          local.get $b
          i32.const 0x0d
          i32.eq
          i32.or
          if
            ;; newline / CR: store the byte; wpos += 1; col reset to 0.
            local.get $wpos
            local.get $b
            i32.store8
            local.get $wpos
            i32.const 1
            i32.add
            local.set $wpos
            i32.const 0
            local.set $col
          else
            ;; ordinary code point: copy l bytes from s+8+i; wpos += l; col += 1.
            local.get $wpos
            local.get $s
            i32.const 8
            i32.add
            local.get $i
            i32.add
            local.get $l
            memory.copy
            local.get $wpos
            local.get $l
            i32.add
            local.set $wpos
            local.get $col
            i32.const 1
            i32.add
            local.set $col
          end
        end
        ;; i += l
        local.get $i
        local.get $l
        i32.add
        local.set $i
        br $l2
      end
    end
    local.get $dst
  )
";

/// PMAT-1189: `$__wasm_str_isdigit(s) -> i32` — Python `s.isdigit()` as a bool
/// (i32 0/1): `1` iff `s` is NON-EMPTY and every code point is an ASCII decimal
/// digit `'0'`–`'9'`, else `0`. Non-allocating (a single left-to-right scan of
/// the payload bytes with no heap use — it does NOT ride `needs_heap`).
///
/// **Empty string → `0`.** Python's `"".isdigit()` is `False` (a vacuous "all
/// chars are digits" is nonetheless `False`), so a zero-length `s` returns `0`
/// before the loop.
///
/// **ASCII-only, with the honest runtime boundary of the case-fold siblings —
/// but short-circuited on a definitive answer first.** Python's `str.isdigit()`
/// also accepts Unicode digit code points (`"²".isdigit()` is `True`),
/// which needs a Unicode table this scalar lane does not carry. The scan is
/// therefore ordered so a DEFINITIVE answer never traps:
///   * a NON-ASCII byte (`>= 0x80`) is reached only when every prior byte was an
///     ASCII digit — at that point the result is genuinely undecidable (the
///     trailing code point might or might not be a Unicode digit), so it executes
///     `unreachable` (a TRAP, like the `upper`/`lower`/`index` siblings) rather
///     than silently returning a wrong bool;
///   * a DEFINITIVELY non-digit ASCII byte (`< '0'` or `> '9'`) short-circuits to
///     `0` BEFORE any later non-ASCII byte is examined, so `"a²".isdigit()`
///     returns `0` (Python's answer) and never traps — the earlier ASCII byte
///     already forces `False` regardless of what follows.
///
/// So a pure-ASCII `s` is answer-exact; a non-ASCII `s` whose ASCII prefix is all
/// digits aborts; a non-ASCII `s` with any earlier non-digit ASCII byte returns
/// `0`. It never passes an unmapped non-ASCII byte off as a wrong `True`/`False`.
const STR_ISDIGIT_HELPER: &str = "\
  ;; PMAT-1189 __wasm_str_isdigit(s) = Python s.isdigit() (i32 bool). True iff s
  ;; is non-empty AND every code point is an ASCII decimal digit '0'..'9'. Empty
  ;; -> 0 (Python's vacuous-all is still False). ASCII-only honest boundary: a
  ;; byte >= 0x80 reached with every prior byte a digit is an undecidable
  ;; Unicode-digit case (\"\\u00b2\".isdigit() is True) -> unreachable (trap); a
  ;; definitively non-digit ASCII byte short-circuits to 0 BEFORE any later
  ;; non-ASCII byte, so \"a\\u00b2\" returns 0 (not a trap), matching Python.
  (func $__wasm_str_isdigit (param $s i32) (result i32)
    (local $slen i32)
    (local $i i32)
    (local $c i32)
    ;; slen = byte length of s
    local.get $s
    i32.load
    local.set $slen
    ;; empty string -> False (Python)
    local.get $slen
    i32.eqz
    if
      i32.const 0
      return
    end
    ;; for i in 0..slen
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
        ;; non-ASCII byte (all prior bytes were digits) -> undecidable -> trap
        local.get $c
        i32.const 0x80
        i32.ge_u
        if
          unreachable
        end
        ;; c < '0' (0x30) -> a non-digit ASCII byte -> definitively 0
        local.get $c
        i32.const 0x30
        i32.lt_u
        if
          i32.const 0
          return
        end
        ;; c > '9' (0x39) -> a non-digit ASCII byte -> definitively 0
        local.get $c
        i32.const 0x39
        i32.gt_u
        if
          i32.const 0
          return
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $loop
      end
    end
    ;; non-empty and every byte was an ASCII digit -> True
    i32.const 1
  )
";

/// PMAT-1191: `$__wasm_str_isalpha(s) -> i32` — Python `s.isalpha()` as a bool
/// (i32 0/1): `1` iff `s` is NON-EMPTY and every code point is an ASCII letter
/// `'A'`–`'Z'` or `'a'`–`'z'`, else `0`. Non-allocating (a single left-to-right
/// scan of the payload bytes with no heap use — it does NOT ride `needs_heap`).
/// The direct predicate twin of [`STR_ISDIGIT_HELPER`], differing only in the
/// per-byte ASCII-membership test (two letter ranges instead of one digit range).
///
/// **Empty string → `0`.** Python's `"".isalpha()` is `False` (a vacuous "all
/// chars are letters" is nonetheless `False`), so a zero-length `s` returns `0`
/// before the loop.
///
/// **ASCII-only, with the honest runtime boundary of the isdigit sibling — but
/// short-circuited on a definitive answer first.** Python's `str.isalpha()`
/// also accepts Unicode letter code points (`"é".isalpha()` is `True`), which
/// needs a Unicode table this scalar lane does not carry. The scan is therefore
/// ordered so a DEFINITIVE answer never traps:
///   * a NON-ASCII byte (`>= 0x80`) is reached only when every prior byte was an
///     ASCII letter — the result is then genuinely undecidable (the trailing code
///     point might or might not be a Unicode letter), so it executes `unreachable`
///     (a TRAP, like the `upper`/`lower`/`isdigit` siblings) rather than silently
///     returning a wrong bool;
///   * a DEFINITIVELY non-letter ASCII byte (below `'A'`, in the gap `'Z'`..`'a'`
///     — i.e. `[\]^_`` — or above `'z'`) short-circuits to `0` BEFORE any later
///     non-ASCII byte is examined, so `"1é".isalpha()` returns `0` (Python's
///     answer) and never traps — the earlier ASCII byte already forces `False`.
///
/// So a pure-ASCII `s` is answer-exact; a non-ASCII `s` whose ASCII prefix is all
/// letters aborts; a non-ASCII `s` with any earlier non-letter ASCII byte returns
/// `0`. It never passes an unmapped non-ASCII byte off as a wrong `True`/`False`.
const STR_ISALPHA_HELPER: &str = "\
  ;; PMAT-1191 __wasm_str_isalpha(s) = Python s.isalpha() (i32 bool). True iff s
  ;; is non-empty AND every code point is an ASCII letter 'A'..'Z' or 'a'..'z'.
  ;; Empty -> 0 (Python's vacuous-all is still False). ASCII-only honest
  ;; boundary: a byte >= 0x80 reached with every prior byte a letter is an
  ;; undecidable Unicode-letter case (\"\\u00e9\".isalpha() is True) -> unreachable
  ;; (trap); a definitively non-letter ASCII byte short-circuits to 0 BEFORE any
  ;; later non-ASCII byte, so \"1\\u00e9\" returns 0 (not a trap), matching Python.
  (func $__wasm_str_isalpha (param $s i32) (result i32)
    (local $slen i32)
    (local $i i32)
    (local $c i32)
    ;; slen = byte length of s
    local.get $s
    i32.load
    local.set $slen
    ;; empty string -> False (Python)
    local.get $slen
    i32.eqz
    if
      i32.const 0
      return
    end
    ;; for i in 0..slen
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
        ;; non-ASCII byte (all prior bytes were letters) -> undecidable -> trap
        local.get $c
        i32.const 0x80
        i32.ge_u
        if
          unreachable
        end
        ;; c < 'A' (0x41) -> a non-letter ASCII byte -> definitively 0
        local.get $c
        i32.const 0x41
        i32.lt_u
        if
          i32.const 0
          return
        end
        ;; 'Z' < c < 'a'  (0x5B..0x60: '[\\]^_`') -> a non-letter ASCII byte -> 0
        local.get $c
        i32.const 0x5A
        i32.gt_u
        local.get $c
        i32.const 0x61
        i32.lt_u
        i32.and
        if
          i32.const 0
          return
        end
        ;; c > 'z' (0x7A) -> a non-letter ASCII byte -> definitively 0
        local.get $c
        i32.const 0x7A
        i32.gt_u
        if
          i32.const 0
          return
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $loop
      end
    end
    ;; non-empty and every byte was an ASCII letter -> True
    i32.const 1
  )
";

/// PMAT-1193: `$__wasm_str_isspace(s) -> i32` — Python `s.isspace()` as a bool
/// (i32 0/1): `1` iff `s` is NON-EMPTY and every code point is ASCII whitespace,
/// else `0`. Non-allocating (a single left-to-right scan of the payload bytes
/// with no heap use — it does NOT ride `needs_heap`). The predicate twin of
/// [`STR_ISDIGIT_HELPER`] / [`STR_ISALPHA_HELPER`], differing only in the
/// per-byte ASCII-membership test — the ASCII WHITESPACE set, which is two
/// contiguous ranges: `0x09`–`0x0D` (`\t \n \v \f \r`) and `0x1C`–`0x20` (FS GS
/// RS US and the space `0x20`). Those four separators `0x1C`–`0x1F` ARE
/// whitespace to CPython's `str.isspace()` (verified vs python3).
///
/// **Empty string → `0`.** Python's `"".isspace()` is `False` (a vacuous "all
/// chars are whitespace" is nonetheless `False`), so a zero-length `s` returns
/// `0` before the loop.
///
/// **ASCII-only, with the honest runtime boundary of the isdigit/isalpha
/// siblings — but short-circuited on a definitive answer first.** Python's
/// `str.isspace()` also accepts non-ASCII Unicode whitespace (`" ".isspace()`
/// — a NBSP — is `True`), which needs a Unicode table this scalar lane does not
/// carry. The scan is therefore ordered so a DEFINITIVE answer never traps:
///   * a NON-ASCII byte (`>= 0x80`) is reached only when every prior byte was
///     ASCII whitespace — the result is then genuinely undecidable (the trailing
///     code point might or might not be Unicode whitespace), so it executes
///     `unreachable` (a TRAP, like the `isdigit` / `isalpha` siblings) rather
///     than silently returning a wrong bool;
///   * a DEFINITIVELY non-whitespace ASCII byte (any byte `< 0x80` outside the
///     two whitespace ranges) short-circuits to `0` BEFORE any later non-ASCII
///     byte is examined, so `"a ".isspace()` returns `0` (Python's answer)
///     and never traps — the earlier ASCII byte already forces `False`.
///
/// So a pure-ASCII `s` is answer-exact; a non-ASCII `s` whose ASCII prefix is all
/// whitespace aborts; a non-ASCII `s` with any earlier non-whitespace ASCII byte
/// returns `0`. It never passes an unmapped non-ASCII byte off as a wrong bool.
const STR_ISSPACE_HELPER: &str = "\
  ;; PMAT-1193 __wasm_str_isspace(s) = Python s.isspace() (i32 bool). True iff s
  ;; is non-empty AND every code point is ASCII whitespace: 0x09..0x0d (\\t\\n\\v\\f\\r)
  ;; or 0x1c..0x20 (FS GS RS US and space). Empty -> 0 (Python's vacuous-all is
  ;; still False). ASCII-only honest boundary: a byte >= 0x80 reached with every
  ;; prior byte whitespace is an undecidable Unicode-whitespace case
  ;; (\"\\u00a0\".isspace() is True) -> unreachable (trap); a definitively
  ;; non-whitespace ASCII byte short-circuits to 0 BEFORE any later non-ASCII
  ;; byte, so \"a\\u00a0\" returns 0 (not a trap), matching Python.
  (func $__wasm_str_isspace (param $s i32) (result i32)
    (local $slen i32)
    (local $i i32)
    (local $c i32)
    ;; slen = byte length of s
    local.get $s
    i32.load
    local.set $slen
    ;; empty string -> False (Python)
    local.get $slen
    i32.eqz
    if
      i32.const 0
      return
    end
    ;; for i in 0..slen
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
        ;; non-ASCII byte (all prior bytes were whitespace) -> undecidable -> trap
        local.get $c
        i32.const 0x80
        i32.ge_u
        if
          unreachable
        end
        ;; is_ws = (0x09 <= c <= 0x0d) | (0x1c <= c <= 0x20); NOT ws -> definitively 0
        ;; (0x09 <= c) & (c <= 0x0d)  -> tab/LF/VT/FF/CR
        local.get $c
        i32.const 0x09
        i32.ge_u
        local.get $c
        i32.const 0x0d
        i32.le_u
        i32.and
        ;; (0x1c <= c) & (c <= 0x20)  -> FS/GS/RS/US/space
        local.get $c
        i32.const 0x1c
        i32.ge_u
        local.get $c
        i32.const 0x20
        i32.le_u
        i32.and
        ;; is_ws = either range
        i32.or
        ;; a definitively non-whitespace ASCII byte -> 0
        i32.eqz
        if
          i32.const 0
          return
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $loop
      end
    end
    ;; non-empty and every byte was ASCII whitespace -> True
    i32.const 1
  )
";

/// PMAT-1195: `$__wasm_str_isalnum(s) -> i32` — Python `s.isalnum()` as a bool
/// (i32 0/1): `1` iff `s` is NON-EMPTY and every code point is ASCII
/// alphanumeric, else `0`. Non-allocating (a single left-to-right scan of the
/// payload bytes with no heap use — it does NOT ride `needs_heap`). The fourth
/// predicate twin of [`STR_ISDIGIT_HELPER`] / [`STR_ISALPHA_HELPER`] /
/// [`STR_ISSPACE_HELPER`], and the direct UNION of the isdigit and isalpha
/// membership tests — the ASCII ALPHANUMERIC set is three contiguous ranges:
/// `0x30`–`0x39` (`'0'`–`'9'`), `0x41`–`0x5A` (`'A'`–`'Z'`) and `0x61`–`0x7A`
/// (`'a'`–`'z'`).
///
/// **Empty string → `0`.** Python's `"".isalnum()` is `False` (a vacuous "all
/// chars are alphanumeric" is nonetheless `False`), so a zero-length `s` returns
/// `0` before the loop.
///
/// **ASCII-only, with the honest runtime boundary of the isdigit/isalpha/isspace
/// siblings — but short-circuited on a definitive answer first.** Python's
/// `str.isalnum()` also accepts non-ASCII Unicode alphanumerics (`"²".isalnum()`
/// — a superscript two — and `"é".isalnum()` are both `True`), which needs a
/// Unicode table this scalar lane does not carry. The scan is therefore ordered
/// so a DEFINITIVE answer never traps:
///   * a NON-ASCII byte (`>= 0x80`) is reached only when every prior byte was
///     ASCII alphanumeric — the result is then genuinely undecidable (the
///     trailing code point might or might not be a Unicode letter/digit), so it
///     executes `unreachable` (a TRAP, like the `isdigit` / `isalpha` /
///     `isspace` siblings) rather than silently returning a wrong bool;
///   * a DEFINITIVELY non-alphanumeric ASCII byte (any byte `< 0x80` outside the
///     three ranges) short-circuits to `0` BEFORE any later non-ASCII byte is
///     examined, so `"a!é".isalnum()` returns `0` (Python's answer) and never
///     traps — the earlier `!` already forces `False`.
///
/// So a pure-ASCII `s` is answer-exact; a non-ASCII `s` whose ASCII prefix is all
/// alphanumeric aborts; a non-ASCII `s` with any earlier non-alphanumeric ASCII
/// byte returns `0`. It never passes an unmapped non-ASCII byte off as a wrong
/// bool.
const STR_ISALNUM_HELPER: &str = "\
  ;; PMAT-1195 __wasm_str_isalnum(s) = Python s.isalnum() (i32 bool). True iff s
  ;; is non-empty AND every code point is ASCII alphanumeric: 0x30..0x39 ('0'..'9')
  ;; or 0x41..0x5a ('A'..'Z') or 0x61..0x7a ('a'..'z'). Empty -> 0 (Python's
  ;; vacuous-all is still False). ASCII-only honest boundary: a byte >= 0x80
  ;; reached with every prior byte alphanumeric is an undecidable Unicode
  ;; letter/digit case (\"\\u00b2\".isalnum() is True) -> unreachable (trap); a
  ;; definitively non-alphanumeric ASCII byte short-circuits to 0 BEFORE any later
  ;; non-ASCII byte, so \"a!\\u00e9\" returns 0 (not a trap), matching Python.
  (func $__wasm_str_isalnum (param $s i32) (result i32)
    (local $slen i32)
    (local $i i32)
    (local $c i32)
    ;; slen = byte length of s
    local.get $s
    i32.load
    local.set $slen
    ;; empty string -> False (Python)
    local.get $slen
    i32.eqz
    if
      i32.const 0
      return
    end
    ;; for i in 0..slen
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
        ;; non-ASCII byte (all prior bytes were alphanumeric) -> undecidable -> trap
        local.get $c
        i32.const 0x80
        i32.ge_u
        if
          unreachable
        end
        ;; is_alnum = (0x30<=c<=0x39) | (0x41<=c<=0x5a) | (0x61<=c<=0x7a);
        ;; NOT alnum -> definitively 0
        ;; (0x30 <= c) & (c <= 0x39)  -> digit
        local.get $c
        i32.const 0x30
        i32.ge_u
        local.get $c
        i32.const 0x39
        i32.le_u
        i32.and
        ;; (0x41 <= c) & (c <= 0x5a)  -> uppercase letter
        local.get $c
        i32.const 0x41
        i32.ge_u
        local.get $c
        i32.const 0x5a
        i32.le_u
        i32.and
        i32.or
        ;; (0x61 <= c) & (c <= 0x7a)  -> lowercase letter
        local.get $c
        i32.const 0x61
        i32.ge_u
        local.get $c
        i32.const 0x7a
        i32.le_u
        i32.and
        i32.or
        ;; a definitively non-alphanumeric ASCII byte -> 0
        i32.eqz
        if
          i32.const 0
          return
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $loop
      end
    end
    ;; non-empty and every byte was ASCII alphanumeric -> True
    i32.const 1
  )
";

/// PMAT-1197: `$__wasm_str_isupper_islower(s, want_upper) -> i32` — Python
/// `s.isupper()` (`want_upper` = 1) / `s.islower()` (`want_upper` = 0) as a bool
/// (i32 0/1). The FIFTH/SIXTH `str` `is*` predicates on the WASM lane, and the
/// first pair whose truth needs STATE across the scan rather than an
/// "every-char-matches" fold: Python's rule is "at least one CASED char AND no
/// cased char of the OPPOSITE case". Non-allocating (a single left-to-right scan
/// of the payload bytes with no heap use — it does NOT ride `needs_heap`). One
/// helper serves both directions (a `want_upper` i32 flag picks which ASCII
/// letter range is the "wanted case" and which is the "disqualifier"), exactly
/// like the `$__wasm_str_upper_lower` case-fold pair.
///
/// **No empty guard needed (unlike the isdigit-family predicates).** The result
/// falls through as `$has_cased`, which starts `0`, so an empty `s` (and any `s`
/// with no cased char, e.g. `"123"`) returns `0` without a special-case — Python
/// `"".isupper()` and `"123".isupper()` are both `False`, and uncased ASCII
/// (digits/space/punctuation) simply doesn't set `$has_cased`.
///
/// **ASCII-only, with the honest runtime boundary of the sibling predicates —
/// short-circuited on a definitive DISQUALIFIER first.** Python's `str.isupper()`
/// / `str.islower()` also decide over non-ASCII cased Unicode (`"Á".isupper()` is
/// `True`, `"Áb".isupper()` is `False`), which needs a case table this scalar
/// lane does not carry. The scan is therefore ordered so a DEFINITIVE `0` never
/// traps:
///   * a NON-ASCII byte (`>= 0x80`) is reached only when no opposite-case ASCII
///     letter has appeared yet — the trailing code point might be a same-case,
///     opposite-case, or uncased Unicode char, all three of which change the
///     answer, so it is genuinely undecidable and executes `unreachable` (a TRAP,
///     like the `isdigit` / `isalpha` / `isspace` / `isalnum` siblings) rather
///     than returning a wrong bool;
///   * an OPPOSITE-CASE ASCII letter (a lowercase letter for `isupper`, an
///     uppercase letter for `islower`) is a definitive disqualifier — it
///     short-circuits to `0` BEFORE any later non-ASCII byte is examined, so
///     `"aÁ".isupper()` returns `0` (Python's answer) and never traps.
///
/// So a pure-ASCII `s` is answer-exact; a non-ASCII `s` whose ASCII prefix has no
/// opposite-case letter aborts; a non-ASCII `s` with an earlier opposite-case
/// ASCII letter returns `0`. It never passes an unmapped non-ASCII byte off as a
/// wrong bool.
const STR_ISUPPER_ISLOWER_HELPER: &str = "\
  ;; PMAT-1197 __wasm_str_isupper_islower(s, want_upper) = Python s.isupper()
  ;; (want_upper=1) / s.islower() (want_upper=0) (i32 bool). True iff s has at
  ;; least one ASCII cased letter in the WANTED case and NO ASCII cased letter in
  ;; the opposite case. No empty guard: $has_cased starts 0, so \"\" / \"123\"
  ;; fall through to 0 (Python False). ASCII-only honest boundary: a byte >= 0x80
  ;; reached with no opposite-case letter yet is an undecidable Unicode-cased case
  ;; (\"\\u00c1b\".isupper() is False, \"\\u00c1\".isupper() is True) -> unreachable
  ;; (trap); an opposite-case ASCII letter short-circuits to 0 BEFORE any later
  ;; non-ASCII byte, so \"a\\u00c1\" returns 0 (not a trap), matching Python.
  (func $__wasm_str_isupper_islower (param $s i32) (param $want_upper i32) (result i32)
    (local $slen i32)
    (local $i i32)
    (local $c i32)
    (local $is_lower i32)
    (local $is_upper i32)
    (local $has_cased i32)
    ;; slen = byte length of s
    local.get $s
    i32.load
    local.set $slen
    ;; for i in 0..slen  ($has_cased defaults to 0 -> empty/uncased s returns 0)
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
        ;; non-ASCII byte (no opposite-case letter seen yet) -> undecidable -> trap
        local.get $c
        i32.const 0x80
        i32.ge_u
        if
          unreachable
        end
        ;; is_lower = (0x61 <= c) & (c <= 0x7a)
        local.get $c
        i32.const 0x61
        i32.ge_u
        local.get $c
        i32.const 0x7a
        i32.le_u
        i32.and
        local.set $is_lower
        ;; is_upper = (0x41 <= c) & (c <= 0x5a)
        local.get $c
        i32.const 0x41
        i32.ge_u
        local.get $c
        i32.const 0x5a
        i32.le_u
        i32.and
        local.set $is_upper
        ;; disqualifier = want_upper ? is_lower : is_upper -> definitively 0
        local.get $is_lower
        local.get $is_upper
        local.get $want_upper
        select
        if
          i32.const 0
          return
        end
        ;; target = want_upper ? is_upper : is_lower -> a wanted-case cased letter
        local.get $is_upper
        local.get $is_lower
        local.get $want_upper
        select
        if
          i32.const 1
          local.set $has_cased
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $loop
      end
    end
    ;; True iff >= 1 wanted-case cased letter and no opposite-case letter
    local.get $has_cased
  )
";

/// PMAT-1199: `$__wasm_str_isascii(s) -> i32` — Python `s.isascii()` as a bool
/// (i32 0/1): `1` iff every payload byte is in the ASCII range (`< 0x80`), else
/// `0`. Non-allocating (a single left-to-right scan of the payload bytes with no
/// heap use — it does NOT ride `needs_heap`). The seventh predicate in the `str`
/// `is*` family to reach the WASM lane (after PMAT-1189 isdigit / 1191 isalpha /
/// 1193 isspace / 1195 isalnum / 1197 isupper+islower).
///
/// **The odd one out of the family — FULLY DECIDABLE at the byte level, so it
/// NEVER traps AND needs NO empty guard.** Where the isdigit-family predicates
/// ask an undecidable Unicode-category question the moment a non-ASCII byte is
/// reached (and so must TRAP on it), `isascii()` asks *exactly* "is every byte
/// `< 0x80`" — a question UTF-8 answers directly (a byte `>= 0x80` is a non-ASCII
/// code point, definitively `False`; a byte `< 0x80` is an ASCII code point). So:
///   * a byte `>= 0x80` short-circuits to `0` (the DEFINITIVE answer, NOT a trap —
///     there is no `unreachable` arm at all, the distinguishing shape of this
///     helper);
///   * a zero-length `s` falls through the loop to `i32.const 1` — Python
///     `"".isascii()` is `True` (unlike the isdigit family's vacuous-`False`), so
///     NO empty guard precedes the loop.
///
/// Byte-exact against CPython for EVERY input (ASCII → the loop completes → `1`;
/// any non-ASCII → `0`); it is the one predicate whose non-ASCII inputs are
/// value-matched (not trapped) in the executed witness.
const STR_ISASCII_HELPER: &str = "\
  ;; PMAT-1199 __wasm_str_isascii(s) = Python s.isascii() (i32 bool). True iff
  ;; every payload byte is ASCII (< 0x80). Empty -> 1 (\"\".isascii() is True, so
  ;; NO empty guard — the loop falls through). FULLY DECIDABLE: a byte >= 0x80 is
  ;; the DEFINITIVE False (return 0), NOT a trap — isascii is exactly the \"are all
  ;; bytes < 0x80\" question, so unlike the isdigit family it carries no trap arm.
  (func $__wasm_str_isascii (param $s i32) (result i32)
    (local $slen i32)
    (local $i i32)
    ;; slen = byte length of s
    local.get $s
    i32.load
    local.set $slen
    ;; NO empty guard: \"\".isascii() is True, the loop below falls through to 1.
    ;; for i in 0..slen
    i32.const 0
    local.set $i
    block $done
      loop $loop
        local.get $i
        local.get $slen
        i32.ge_s
        br_if $done
        ;; a non-ASCII payload byte (>= 0x80) is the DEFINITIVE answer: False.
        ;; No trap arm — isascii is fully decidable at the byte level.
        local.get $s
        i32.const 8
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        i32.const 0x80
        i32.ge_u
        if
          i32.const 0
          return
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $loop
      end
    end
    ;; empty OR every byte < 0x80 -> True
    i32.const 1
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
/// PMAT-1247: emit the four SET-ALGEBRA runtime helpers for one key kind —
/// `$__wasm_set_union_<k>` / `_intersection_` / `_difference_` / `_symdiff_`,
/// each `(p, q) -> i32` returning a fresh keys-only set region (Python set
/// algebra yields a NEW set, never mutating an operand). Construction reuses the
/// update-or-insert dedup helper `$__wasm_dict_set_<k>` and, for the gated ops,
/// the never-trapping membership probe `$__wasm_dict_has_<k>` — so a set of this
/// kind forces NO helper beyond the ones a literal already carries. Cap is
/// pre-sized to the worst-case result (`|p|+|q|` for union/symdiff, `|p|` for
/// intersection/difference), so a construction insert never trips the 2x
/// realloc-grow path; each still consumes `dict_set`'s returned pointer, so a
/// (hypothetical) grow would stay correct. Emitted from [`dict_helpers_for`]
/// after the set predicates, gated on [`module_dict_key_kinds`].
fn emit_set_algebra_helpers(out: &mut String, kind: KeyKind) {
    // union: every key of p, then every key of q (dedup collapses the overlap).
    emit_set_alg_helper(
        out,
        kind,
        "union",
        true,
        &[("p", None), ("q", None)],
        "p ∪ q (insert all of p, then all of q; dict_set dedup drops shared keys)",
    );
    // intersection: only the keys of p that are ALSO in q.
    emit_set_alg_helper(
        out,
        kind,
        "intersection",
        false,
        &[("p", Some(("q", true)))],
        "p ∩ q (keys of p that are members of q)",
    );
    // difference: only the keys of p that are NOT in q.
    emit_set_alg_helper(
        out,
        kind,
        "difference",
        false,
        &[("p", Some(("q", false)))],
        "p − q (keys of p that are not members of q)",
    );
    // symmetric difference: (p − q) then (q − p) — the keys in exactly one side.
    emit_set_alg_helper(
        out,
        kind,
        "symdiff",
        true,
        &[("p", Some(("q", false))), ("q", Some(("p", false)))],
        "p △ q ((p − q) ∪ (q − p): keys in exactly one of the two sets)",
    );
}

/// Emit ONE set-algebra helper: allocate a fresh set sized to `cap`
/// (`|p|+|q|` if `cap_both` else `|p|`), write its `[count=0][cap]` header, run
/// each `(src, gate)` walk (see [`emit_set_alg_walk`]), then leave the new set's
/// base-pointer. `walks` lists the source region(s) to insert from in order.
fn emit_set_alg_helper(
    out: &mut String,
    kind: KeyKind,
    name: &str,
    cap_both: bool,
    walks: &[(&str, Option<(&str, bool)>)],
    desc: &str,
) {
    let s = kind.suffix();
    let kload = match kind {
        KeyKind::Int => "i64.load",
        KeyKind::Str => "i32.load",
    };
    writeln!(
        out,
        "  ;; __wasm_set_{name}_{s}(p, q) -> a NEW set = {desc}"
    )
    .expect("write");
    writeln!(
        out,
        "  (func $__wasm_set_{name}_{s} (param $p i32) (param $q i32) (result i32)"
    )
    .expect("write");
    writeln!(
        out,
        "    (local $r i32) (local $cap i32) (local $i i32) (local $n i32) (local $ea i32)"
    )
    .expect("write");
    // cap = |p| (+ |q| for union / symmetric difference)
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    i32.load").expect("write");
    if cap_both {
        writeln!(out, "    local.get $q").expect("write");
        writeln!(out, "    i32.load").expect("write");
        writeln!(out, "    i32.add").expect("write");
    }
    writeln!(out, "    local.set $cap").expect("write");
    // r = __alloc(LIST_ELEMS_OFFSET + cap*DICT_ENTRY_SIZE)
    writeln!(out, "    local.get $cap").expect("write");
    writeln!(out, "    i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "    i32.mul").expect("write");
    writeln!(out, "    i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "    i32.add").expect("write");
    writeln!(out, "    call $__alloc").expect("write");
    writeln!(out, "    local.set $r").expect("write");
    // header: count = 0 @ r+0 (each insert increments it)
    writeln!(out, "    local.get $r").expect("write");
    writeln!(out, "    i32.const 0").expect("write");
    writeln!(out, "    i32.store").expect("write");
    // header: capacity = cap @ r+DICT_CAP_OFFSET
    writeln!(out, "    local.get $r").expect("write");
    writeln!(out, "    local.get $cap").expect("write");
    writeln!(out, "    i32.store offset={DICT_CAP_OFFSET}").expect("write");
    for (src, gate) in walks {
        emit_set_alg_walk(out, s, kload, src, *gate);
    }
    // result = the new set's base-pointer.
    writeln!(out, "    local.get $r").expect("write");
    writeln!(out, "  )").expect("write");
}

/// Emit a `walk $src, (optionally gated) insert each key into $r` loop into a
/// set-algebra helper body. `gate = None` inserts unconditionally (union);
/// `Some((g, true))` inserts only when the key IS a member of set `$g`
/// (intersection); `Some((g, false))` inserts only when it is NOT (difference /
/// symmetric difference). Insertion is the dedup `$__wasm_dict_set_<k>` with a
/// `0` value sentinel, whose returned (possibly relocated) pointer is consumed
/// back into `$r`. The `$done<src>` / `$next<src>` labels are per-source so a
/// two-walk helper (union / symdiff) never collides.
fn emit_set_alg_walk(
    out: &mut String,
    s: &str,
    kload: &str,
    src: &str,
    gate: Option<(&str, bool)>,
) {
    // n = count(src); i = 0
    writeln!(out, "    local.get ${src}").expect("write");
    writeln!(out, "    i32.load").expect("write");
    writeln!(out, "    local.set $n").expect("write");
    writeln!(out, "    i32.const 0").expect("write");
    writeln!(out, "    local.set $i").expect("write");
    writeln!(out, "    (block $done{src}").expect("write");
    writeln!(out, "      (loop $next{src}").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        local.get $n").expect("write");
    writeln!(out, "        i32.ge_s").expect("write");
    writeln!(out, "        br_if $done{src}").expect("write");
    // $ea = src + LIST_ELEMS_OFFSET + i*DICT_ENTRY_SIZE (entry i's address).
    writeln!(out, "        local.get ${src}").expect("write");
    writeln!(out, "        i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "        i32.mul").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.set $ea").expect("write");
    let ind = if let Some((gset, want_present)) = gate {
        // gate on membership in $gset: has(gset, key@ea), negate if !want_present.
        writeln!(out, "        local.get ${gset}").expect("write");
        writeln!(out, "        local.get $ea").expect("write");
        writeln!(out, "        {kload}").expect("write");
        writeln!(out, "        call $__wasm_dict_has_{s}").expect("write");
        if !want_present {
            writeln!(out, "        i32.eqz").expect("write");
        }
        writeln!(out, "        if").expect("write");
        "          "
    } else {
        "        "
    };
    // r = dict_set(r, key@ea, 0) — update-or-insert (dedup), consume the pointer.
    writeln!(out, "{ind}local.get $r").expect("write");
    writeln!(out, "{ind}local.get $ea").expect("write");
    writeln!(out, "{ind}{kload}").expect("write");
    writeln!(out, "{ind}i64.const 0").expect("write");
    writeln!(out, "{ind}call $__wasm_dict_set_{s}").expect("write");
    writeln!(out, "{ind}local.set $r").expect("write");
    if gate.is_some() {
        writeln!(out, "        end").expect("write");
    }
    // i++
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const 1").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.set $i").expect("write");
    writeln!(out, "        br $next{src}").expect("write");
    writeln!(out, "      )").expect("write");
    writeln!(out, "    )").expect("write");
}

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

    // PMAT-1242: set equality — `set(p) == set(q)` ? 1 : 0. STRUCTURAL: sizes
    // match AND every key of p is a member of q. A set has no duplicate keys,
    // so equal size + (p ⊆ q) ⟺ p == q — no need to also walk q. Reuses the
    // never-trapping `$__wasm_dict_has_{s}` to probe q, so it introduces NO
    // helper a set of this kind does not already force. Order-INDEPENDENT: the
    // boolean result does not depend on the swap-into-hole storage order, so it
    // is CPython-exact even for sets that have had elements removed.
    writeln!(
        out,
        "  ;; __wasm_set_eq_{s}(p, q) = (set p == set q) ? 1 : 0 (|p|==|q| AND p ⊆ q)"
    )
    .expect("write");
    writeln!(
        out,
        "  (func $__wasm_set_eq_{s} (param $p i32) (param $q i32) (result i32)"
    )
    .expect("write");
    writeln!(out, "    (local $i i32) (local $n i32) (local $ea i32)").expect("write");
    // size check: |p| != |q| → not equal (cheap header compare, first).
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    i32.load").expect("write");
    writeln!(out, "    local.get $q").expect("write");
    writeln!(out, "    i32.load").expect("write");
    writeln!(out, "    i32.ne").expect("write");
    writeln!(out, "    if").expect("write");
    writeln!(out, "      i32.const 0").expect("write");
    writeln!(out, "      return").expect("write");
    writeln!(out, "    end").expect("write");
    // walk p; every key must be a member of q, else not equal.
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    i32.load").expect("write");
    writeln!(out, "    local.set $n").expect("write");
    writeln!(out, "    i32.const 0").expect("write");
    writeln!(out, "    local.set $i").expect("write");
    writeln!(out, "    (block $done").expect("write");
    writeln!(out, "      (loop $next").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        local.get $n").expect("write");
    writeln!(out, "        i32.ge_s").expect("write");
    writeln!(out, "        br_if $done").expect("write");
    // $ea = p + LIST_ELEMS_OFFSET + i*DICT_ENTRY_SIZE (entry i's address).
    writeln!(out, "        local.get $p").expect("write");
    writeln!(out, "        i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "        i32.mul").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.set $ea").expect("write");
    // if key@ea is NOT in q → return 0. Load the key with the kind's shape.
    writeln!(out, "        local.get $q").expect("write");
    writeln!(out, "        local.get $ea").expect("write");
    match kind {
        KeyKind::Int => writeln!(out, "        i64.load").expect("write"),
        KeyKind::Str => writeln!(out, "        i32.load").expect("write"),
    }
    writeln!(out, "        call $__wasm_dict_has_{s}").expect("write");
    writeln!(out, "        i32.eqz").expect("write");
    writeln!(out, "        if").expect("write");
    writeln!(out, "          i32.const 0").expect("write");
    writeln!(out, "          return").expect("write");
    writeln!(out, "        end").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const 1").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.set $i").expect("write");
    writeln!(out, "        br $next").expect("write");
    writeln!(out, "      )").expect("write");
    writeln!(out, "    )").expect("write");
    // equal size + every key present → equal.
    writeln!(out, "    i32.const 1").expect("write");
    writeln!(out, "  )").expect("write");

    // PMAT-1243: dict equality — `dict(p) == dict(q)` ? 1 : 0. STRUCTURAL over
    // keys AND values (Python `{1:2} == {2:1}` is False, `{1:2} == {1:2}` True):
    // sizes match AND every key of p is present in q with an EQUAL value. Dict
    // keys are unique, so equal size + (∀k∈p: k∈q ∧ p[k]==q[k]) ⟺ p == q — no
    // need to also walk q. Reuses the never-trapping `$__wasm_dict_has_{s}` to
    // probe membership and `$__wasm_dict_get_{s}` to fetch q's value (safe: only
    // called when has already returned 1, so it never traps). Values are always
    // the i64 slot, so `i64.ne` is the value compare. Order-INDEPENDENT: the
    // result never depends on the swap-into-hole storage order a `del`/`pop`
    // leaves behind, so it is CPython-exact even after a removal.
    writeln!(
        out,
        "  ;; __wasm_dict_eq_{s}(p, q) = (dict p == dict q) ? 1 : 0 (|p|==|q| AND ∀k: q[k]==p[k])"
    )
    .expect("write");
    writeln!(
        out,
        "  (func $__wasm_dict_eq_{s} (param $p i32) (param $q i32) (result i32)"
    )
    .expect("write");
    writeln!(
        out,
        "    (local $i i32) (local $n i32) (local $ea i32) (local $k {kparam})"
    )
    .expect("write");
    // size check: |p| != |q| → not equal (cheap header compare, first).
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    i32.load").expect("write");
    writeln!(out, "    local.get $q").expect("write");
    writeln!(out, "    i32.load").expect("write");
    writeln!(out, "    i32.ne").expect("write");
    writeln!(out, "    if").expect("write");
    writeln!(out, "      i32.const 0").expect("write");
    writeln!(out, "      return").expect("write");
    writeln!(out, "    end").expect("write");
    // walk p; every key must be present in q with an equal value, else not equal.
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    i32.load").expect("write");
    writeln!(out, "    local.set $n").expect("write");
    writeln!(out, "    i32.const 0").expect("write");
    writeln!(out, "    local.set $i").expect("write");
    writeln!(out, "    (block $done").expect("write");
    writeln!(out, "      (loop $next").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        local.get $n").expect("write");
    writeln!(out, "        i32.ge_s").expect("write");
    writeln!(out, "        br_if $done").expect("write");
    // $ea = p + LIST_ELEMS_OFFSET + i*DICT_ENTRY_SIZE (entry i's address).
    writeln!(out, "        local.get $p").expect("write");
    writeln!(out, "        i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "        i32.mul").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.set $ea").expect("write");
    // $k = key@ea (loaded with the kind's shape); cached for has + get probes.
    writeln!(out, "        local.get $ea").expect("write");
    match kind {
        KeyKind::Int => writeln!(out, "        i64.load").expect("write"),
        KeyKind::Str => writeln!(out, "        i32.load").expect("write"),
    }
    writeln!(out, "        local.set $k").expect("write");
    // if key ∉ q → return 0 (never-trapping membership probe).
    writeln!(out, "        local.get $q").expect("write");
    writeln!(out, "        local.get $k").expect("write");
    writeln!(out, "        call $__wasm_dict_has_{s}").expect("write");
    writeln!(out, "        i32.eqz").expect("write");
    writeln!(out, "        if").expect("write");
    writeln!(out, "          i32.const 0").expect("write");
    writeln!(out, "          return").expect("write");
    writeln!(out, "        end").expect("write");
    // if p[k] != q[k] → return 0. p's value is entry $ea's DICT_VAL_OFFSET slot;
    // q's value comes from get (safe now — has just confirmed the key is present).
    writeln!(out, "        local.get $ea").expect("write");
    writeln!(out, "        i64.load offset={DICT_VAL_OFFSET}").expect("write");
    writeln!(out, "        local.get $q").expect("write");
    writeln!(out, "        local.get $k").expect("write");
    writeln!(out, "        call $__wasm_dict_get_{s}").expect("write");
    writeln!(out, "        i64.ne").expect("write");
    writeln!(out, "        if").expect("write");
    writeln!(out, "          i32.const 0").expect("write");
    writeln!(out, "          return").expect("write");
    writeln!(out, "        end").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const 1").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.set $i").expect("write");
    writeln!(out, "        br $next").expect("write");
    writeln!(out, "      )").expect("write");
    writeln!(out, "    )").expect("write");
    // equal size + every key present with an equal value → equal.
    writeln!(out, "    i32.const 1").expect("write");
    writeln!(out, "  )").expect("write");

    // PMAT-1244: set subset — `set(p) ⊆ set(q)` ? 1 : 0. MEMBERSHIP-ONLY: every
    // key of p must be a member of q (NO size gate, unlike `set_eq`). This is the
    // engine behind Python's set ordering: `p <= q` ⇔ p ⊆ q; `p >= q` ⇔ q ⊆ p
    // (operands swapped by the caller); the STRICT `<`/`>` add an `emit_binop`
    // inline `|p| < |q|` header compare on top (a subset with unequal size is a
    // PROPER subset, since for sets p ⊆ q ⟹ |p| ≤ |q|). Reuses the never-trapping
    // `$__wasm_dict_has_{s}` to probe q, so it introduces NO helper a set of this
    // kind does not already force. Order-INDEPENDENT: the boolean result does not
    // depend on the swap-into-hole storage order, so it is CPython-exact even for
    // sets that have had elements removed.
    writeln!(
        out,
        "  ;; __wasm_set_subset_{s}(p, q) = (set p ⊆ set q) ? 1 : 0 (∀ key∈p: key∈q)"
    )
    .expect("write");
    writeln!(
        out,
        "  (func $__wasm_set_subset_{s} (param $p i32) (param $q i32) (result i32)"
    )
    .expect("write");
    writeln!(out, "    (local $i i32) (local $n i32) (local $ea i32)").expect("write");
    // walk p; every key must be a member of q, else not a subset. No size gate.
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    i32.load").expect("write");
    writeln!(out, "    local.set $n").expect("write");
    writeln!(out, "    i32.const 0").expect("write");
    writeln!(out, "    local.set $i").expect("write");
    writeln!(out, "    (block $done").expect("write");
    writeln!(out, "      (loop $next").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        local.get $n").expect("write");
    writeln!(out, "        i32.ge_s").expect("write");
    writeln!(out, "        br_if $done").expect("write");
    // $ea = p + LIST_ELEMS_OFFSET + i*DICT_ENTRY_SIZE (entry i's address).
    writeln!(out, "        local.get $p").expect("write");
    writeln!(out, "        i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "        i32.mul").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.set $ea").expect("write");
    // if key@ea is NOT in q → return 0. Load the key with the kind's shape.
    writeln!(out, "        local.get $q").expect("write");
    writeln!(out, "        local.get $ea").expect("write");
    match kind {
        KeyKind::Int => writeln!(out, "        i64.load").expect("write"),
        KeyKind::Str => writeln!(out, "        i32.load").expect("write"),
    }
    writeln!(out, "        call $__wasm_dict_has_{s}").expect("write");
    writeln!(out, "        i32.eqz").expect("write");
    writeln!(out, "        if").expect("write");
    writeln!(out, "          i32.const 0").expect("write");
    writeln!(out, "          return").expect("write");
    writeln!(out, "        end").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const 1").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.set $i").expect("write");
    writeln!(out, "        br $next").expect("write");
    writeln!(out, "      )").expect("write");
    writeln!(out, "    )").expect("write");
    // every key of p present in q → p ⊆ q.
    writeln!(out, "    i32.const 1").expect("write");
    writeln!(out, "  )").expect("write");

    // PMAT-1246: set disjoint — `set(p).isdisjoint(set(q))` ? 1 : 0. The
    // no-common-element predicate — the DUAL of subset: subset returns 0 on ANY
    // ABSENT key (∀ key∈p: key∈q), disjoint returns 0 on ANY PRESENT key
    // (∀ key∈p: key∉q). Two disjoint sets share nothing; if any key of p is a
    // member of q the sets intersect. Walk p, probe q with the never-trapping
    // `$__wasm_dict_has_{s}` (already forced by a set of this kind → NO new helper
    // dependency). NO size gate — disjoint has no cardinality relation (a size-1
    // set can be disjoint from a size-100 one). SYMMETRIC (p∩q=∅ ⇔ q∩p=∅) so
    // walking p vs q is CPython-exact regardless of which side is the receiver;
    // order-INDEPENDENT so it survives a swap-into-hole discard. Two EMPTY sets are
    // disjoint (the loop never runs → falls through to 1), matching CPython.
    writeln!(
        out,
        "  ;; __wasm_set_disjoint_{s}(p, q) = set(p).isdisjoint(set(q)) ? 1 : 0 (∀ key∈p: key∉q)"
    )
    .expect("write");
    writeln!(
        out,
        "  (func $__wasm_set_disjoint_{s} (param $p i32) (param $q i32) (result i32)"
    )
    .expect("write");
    writeln!(out, "    (local $i i32) (local $n i32) (local $ea i32)").expect("write");
    // walk p; the FIRST key that is a member of q → the sets intersect → return 0.
    writeln!(out, "    local.get $p").expect("write");
    writeln!(out, "    i32.load").expect("write");
    writeln!(out, "    local.set $n").expect("write");
    writeln!(out, "    i32.const 0").expect("write");
    writeln!(out, "    local.set $i").expect("write");
    writeln!(out, "    (block $done").expect("write");
    writeln!(out, "      (loop $next").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        local.get $n").expect("write");
    writeln!(out, "        i32.ge_s").expect("write");
    writeln!(out, "        br_if $done").expect("write");
    // $ea = p + LIST_ELEMS_OFFSET + i*DICT_ENTRY_SIZE (entry i's address).
    writeln!(out, "        local.get $p").expect("write");
    writeln!(out, "        i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "        i32.mul").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.set $ea").expect("write");
    // if key@ea IS in q → the sets share a key → return 0 (not disjoint).
    writeln!(out, "        local.get $q").expect("write");
    writeln!(out, "        local.get $ea").expect("write");
    match kind {
        KeyKind::Int => writeln!(out, "        i64.load").expect("write"),
        KeyKind::Str => writeln!(out, "        i32.load").expect("write"),
    }
    writeln!(out, "        call $__wasm_dict_has_{s}").expect("write");
    writeln!(out, "        if").expect("write");
    writeln!(out, "          i32.const 0").expect("write");
    writeln!(out, "          return").expect("write");
    writeln!(out, "        end").expect("write");
    writeln!(out, "        local.get $i").expect("write");
    writeln!(out, "        i32.const 1").expect("write");
    writeln!(out, "        i32.add").expect("write");
    writeln!(out, "        local.set $i").expect("write");
    writeln!(out, "        br $next").expect("write");
    writeln!(out, "      )").expect("write");
    writeln!(out, "    )").expect("write");
    // no key of p is a member of q → p ∩ q = ∅ → disjoint.
    writeln!(out, "    i32.const 1").expect("write");
    writeln!(out, "  )").expect("write");

    // PMAT-1247: the four SET-ALGEBRA helpers (union / intersection / difference
    // / symmetric difference), each allocating a NEW set from p and q. They
    // forward-reference `$__wasm_dict_set_{s}` (defined just below) and reuse the
    // above `$__wasm_dict_has_{s}` probe — WAT resolves calls by name, so the
    // forward reference assembles cleanly.
    emit_set_algebra_helpers(&mut out, kind);

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

    // pop: read AND remove the value at a matching key, else trap. PMAT-1225:
    // Python `d.pop(k)`. Removal is swap-last-into-hole + count-- — O(1) and
    // IN PLACE: the region only shrinks, so the base pointer NEVER moves (no
    // 2x realloc, unlike `set`) and the caller keeps its dict local unchanged.
    // The bare `d.pop(k)` KeyError analogue is the `unreachable` on the
    // not-found tail; the 2-arg `d.pop(k, default)` form is gated by `has` at
    // the call site (see `emit_dict_pop`), so this helper only ever runs when
    // the key is present — an absent key on the bare form is the one that traps.
    writeln!(
        out,
        "  ;; __wasm_dict_pop_{s}(p, key) -> d[key]; REMOVES the entry (swap-last-into-hole,"
    )
    .expect("write");
    writeln!(
        out,
        "  ;; count--); traps (unreachable) if absent (KeyError). Base pointer never moves."
    )
    .expect("write");
    writeln!(
        out,
        "  (func $__wasm_dict_pop_{s} (param $p i32) (param $k {kparam}) (result i64)"
    )
    .expect("write");
    writeln!(out, "    (local $v i64) (local $last i32)").expect("write");
    emit_dict_scan_prologue(&mut out);
    emit_dict_key_compare(&mut out, kind);
    writeln!(out, "        if").expect("write");
    // v = entry.value (captured BEFORE the hole is overwritten).
    writeln!(out, "          local.get $ea").expect("write");
    writeln!(out, "          i64.load offset={DICT_VAL_OFFSET}").expect("write");
    writeln!(out, "          local.set $v").expect("write");
    // last = p + LIST_ELEMS_OFFSET + (n-1)*DICT_ENTRY_SIZE (the final entry).
    writeln!(out, "          local.get $p").expect("write");
    writeln!(out, "          i32.const {LIST_ELEMS_OFFSET}").expect("write");
    writeln!(out, "          i32.add").expect("write");
    writeln!(out, "          local.get $n").expect("write");
    writeln!(out, "          i32.const 1").expect("write");
    writeln!(out, "          i32.sub").expect("write");
    writeln!(out, "          i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "          i32.mul").expect("write");
    writeln!(out, "          i32.add").expect("write");
    writeln!(out, "          local.set $last").expect("write");
    // memory.copy(dst=$ea, src=$last, ENTRY): move the last entry into the
    // hole. A no-op when $ea == $last (popping the last-indexed entry) — the
    // decremented count then drops it from the scan range either way.
    writeln!(out, "          local.get $ea").expect("write");
    writeln!(out, "          local.get $last").expect("write");
    writeln!(out, "          i32.const {DICT_ENTRY_SIZE}").expect("write");
    writeln!(out, "          memory.copy").expect("write");
    // count = n - 1 (stored at base+0).
    writeln!(out, "          local.get $p").expect("write");
    writeln!(out, "          local.get $n").expect("write");
    writeln!(out, "          i32.const 1").expect("write");
    writeln!(out, "          i32.sub").expect("write");
    writeln!(out, "          i32.store").expect("write");
    writeln!(out, "          local.get $v").expect("write");
    writeln!(out, "          return").expect("write");
    writeln!(out, "        end").expect("write");
    emit_dict_scan_epilogue(&mut out);
    writeln!(out, "    unreachable").expect("write");
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

/// PMAT-1248: the list-INT-SUM helper — Python `sum(xs)` over a `list[int]`.
/// `base` is an i32 pointer to a length-prefixed region (i32 element count @
/// base+0, packed i64 elements @ base+8, the PMAT-968 list ABI). It folds
/// `acc += xs[i]` LEFT-TO-RIGHT for `i` in `0..count` — matching CPython's
/// left-to-right reduction and the rust `.iter().sum::<i64>()` lane — and
/// returns the i64 total. The empty list sums to 0 (Python `sum([]) == 0`).
/// Non-allocating: it reads the list payload and touches no heap, so it is
/// gated ONLY on `needs_list_sum` (NOT `needs_heap`), like the byte-scan
/// str predicates. Integer overflow wraps in `i64.add` — the same modular
/// i64 posture the WASM `+` lowering already carries (Python ints are
/// unbounded; this is the documented scalar-subset wart, not a new one).
const LIST_SUM_INT_HELPER: &str = "\
  ;; __wasm_list_sum_i64(base) = sum(xs) for a list[int] (Python sum(xs))
  ;; base → length-prefixed region: i32 count @ base+0, i64 elements @ base+8.
  (func $__wasm_list_sum_i64 (param $base i32) (result i64)
    (local $i i32)
    (local $n i32)
    (local $acc i64)
    ;; n = element count (i32 header @ base+0); acc = 0; i = 0
    local.get $base
    i32.load
    local.set $n
    i64.const 0
    local.set $acc
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        ;; while i < n  (unsigned — count is a non-negative header)
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; acc += i64.load(base + 8 + i*8)
        local.get $acc
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.add
        local.set $acc
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    local.get $acc)
";

/// PMAT-1249: the list-FLOAT-SUM helper — Python `sum(xs)` over a
/// `list[float]`. Structurally identical to [`LIST_SUM_INT_HELPER`] but with an
/// f64 accumulator: `base` is the same PMAT-968 length-prefixed region (i32
/// element count @ base+0, packed f64 elements @ base+8 — a float list shares
/// the int list's header and 8-byte stride, only the element `*.load`/`*.store`
/// opcode differs), and it folds `acc += xs[i]` LEFT-TO-RIGHT for `i` in
/// `0..count`, returning the f64 total. The empty list sums to `0.0` (Python
/// `sum([]) == 0`, whose float promotion is `0.0`; the differential witness
/// diffs against CPython's `float(sum([]))`). Non-allocating: it reads the list
/// payload and touches no heap, so it is gated on its OWN `needs_list_sum_float`
/// (NOT `needs_heap`), exactly like the i64 sibling. Float addition is IEEE-754
/// round-to-nearest and left-associative here — matching CPython's plain
/// left-to-right `sum` (which does NOT use a compensated/pairwise reduction, so
/// no Kahan correction is owed) and the Rust `.iter().sum::<f64>()` lane.
const LIST_SUM_FLOAT_HELPER: &str = "\
  ;; __wasm_list_sum_f64(base) = sum(xs) for a list[float] (Python sum(xs))
  ;; base → length-prefixed region: i32 count @ base+0, f64 elements @ base+8.
  (func $__wasm_list_sum_f64 (param $base i32) (result f64)
    (local $i i32)
    (local $n i32)
    (local $acc f64)
    ;; n = element count (i32 header @ base+0); acc = 0.0; i = 0
    local.get $base
    i32.load
    local.set $n
    f64.const 0
    local.set $acc
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        ;; while i < n  (unsigned — count is a non-negative header)
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; acc += f64.load(base + 8 + i*8)
        local.get $acc
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        f64.load
        f64.add
        local.set $acc
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    local.get $acc)
";

/// PMAT-1250: the list-INT-MIN/MAX reduction helper — Python `min(xs)` /
/// `max(xs)` over a `list[int]`. Like the sum helpers it reads the PMAT-968
/// length-prefixed region (i32 count @ base+0, packed i64 elements @ base+8)
/// and touches no heap, so it rides its OWN gate (`needs_list_minmax`), NOT
/// `needs_heap`. The `$is_max` param selects the reduction at the call site
/// (`i32.const 1` for `max`, `i32.const 0` for `min`) so ONE helper serves both
/// directions. Semantics match CPython exactly: seed the accumulator with
/// `xs[0]`, then for `i` in `1..count` replace it ONLY on a STRICT improvement
/// (`x > acc` for max, `x < acc` for min) — so a tie keeps the FIRST extremal
/// element, as CPython's `max`/`min` do (for scalar ints ties are
/// indistinguishable, but the strict compare is the faithful lowering and
/// matches the Rust `.iter().max()/.min().unwrap()` lane). The EMPTY list traps
/// (`unreachable`) — Python `min([])`/`max([])` raises `ValueError`, and the
/// Rust lane's `.unwrap()` panics; a trap is the WASM analogue (never a silent
/// wrong value). Integer compares are signed (`i64.gt_s`/`i64.lt_s`), the same
/// signed-i64 posture the scalar subset already carries.
const LIST_MINMAX_INT_HELPER: &str = "\
  ;; __wasm_list_minmax_i64(base, is_max) = max(xs) if is_max else min(xs), list[int]
  ;; base → length-prefixed region: i32 count @ base+0, i64 elements @ base+8.
  (func $__wasm_list_minmax_i64 (param $base i32) (param $is_max i32) (result i64)
    (local $i i32)
    (local $n i32)
    (local $acc i64)
    (local $x i64)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; empty list → trap (Python min/max of an empty sequence raises ValueError)
    local.get $n
    i32.eqz
    if
      unreachable
    end
    ;; acc = xs[0] (i64.load @ base+8); i = 1
    local.get $base
    i32.const 8
    i32.add
    i64.load
    local.set $acc
    i32.const 1
    local.set $i
    (block $done
      (loop $next
        ;; while i < n  (unsigned — count is a non-negative header)
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; x = i64.load(base + 8 + i*8)
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        i64.load
        local.set $x
        ;; keep = is_max ? (x > acc) : (x < acc)  — STRICT, so ties keep the first
        local.get $is_max
        if (result i32)
          local.get $x
          local.get $acc
          i64.gt_s
        else
          local.get $x
          local.get $acc
          i64.lt_s
        end
        if
          local.get $x
          local.set $acc
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    local.get $acc)
";

/// PMAT-1250: the list-FLOAT-MIN/MAX twin — Python `min(xs)` / `max(xs)` over a
/// `list[float]`. Structurally identical to [`LIST_MINMAX_INT_HELPER`] but with
/// an f64 accumulator and IEEE-754 `f64.gt`/`f64.lt` compares (a float list
/// shares the int list's header + 8-byte stride; only the element `*.load` and
/// the compare opcode differ). Gated on its OWN `needs_list_minmax_float` (also
/// non-allocating, so likewise NOT on `needs_heap`). Empty list traps, matching
/// the int sibling and Python's `ValueError`. Ties keep the first extremal
/// (strict `f64.gt`/`f64.lt`), matching CPython and the Rust
/// `.iter().copied().reduce(f64::max/min).unwrap()` lane.
const LIST_MINMAX_FLOAT_HELPER: &str = "\
  ;; __wasm_list_minmax_f64(base, is_max) = max(xs) if is_max else min(xs), list[float]
  ;; base → length-prefixed region: i32 count @ base+0, f64 elements @ base+8.
  (func $__wasm_list_minmax_f64 (param $base i32) (param $is_max i32) (result f64)
    (local $i i32)
    (local $n i32)
    (local $acc f64)
    (local $x f64)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; empty list → trap (Python min/max of an empty sequence raises ValueError)
    local.get $n
    i32.eqz
    if
      unreachable
    end
    ;; acc = xs[0] (f64.load @ base+8); i = 1
    local.get $base
    i32.const 8
    i32.add
    f64.load
    local.set $acc
    i32.const 1
    local.set $i
    (block $done
      (loop $next
        ;; while i < n  (unsigned — count is a non-negative header)
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; x = f64.load(base + 8 + i*8)
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        f64.load
        local.set $x
        ;; keep = is_max ? (x > acc) : (x < acc)  — STRICT, so ties keep the first
        local.get $is_max
        if (result i32)
          local.get $x
          local.get $acc
          f64.gt
        else
          local.get $x
          local.get $acc
          f64.lt
        end
        if
          local.get $x
          local.set $acc
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    local.get $acc)
";

/// PMAT-1262: the list-INT-MEMBERSHIP helper — Python `x in xs` / `x not in xs`
/// over a `list[int]`. A NON-allocating read (a linear scan, like `sum`/`min`/
/// `max`), so it rides its OWN gate (`needs_list_contains`), NOT `needs_heap`.
/// It reads the PMAT-968 length-prefixed region (i32 count @ base+0, packed i64
/// elements @ base+8), compares each element to the `$needle` with `i64.eq`, and
/// returns `1` on the FIRST match, else `0` when the scan is exhausted (so the
/// EMPTY list yields `0` — `x in []` is `False`, exactly as CPython/`[].contains`).
/// The scan is left-to-right and short-circuits on the first hit — membership is
/// order-independent so the walk direction is immaterial, but a forward scan
/// matches the Rust `.iter().any(|&e| e == needle)` / `.contains(&needle)` lane.
/// `x not in xs` needs NO separate helper: the frontend wraps this
/// [`xpile_meta_hir::Expr::ListContains`] in a `UnOp::Not`, which the scalar
/// subset already lowers over the `i32` (0/1) result.
const LIST_CONTAINS_INT_HELPER: &str = "\
  ;; __wasm_list_contains_i64(base, needle) = 1 if needle in xs else 0, list[int]
  ;; base → length-prefixed region: i32 count @ base+0, i64 elements @ base+8.
  (func $__wasm_list_contains_i64 (param $base i32) (param $needle i64) (result i32)
    (local $i i32)
    (local $n i32)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        ;; while i < n  (unsigned — count is a non-negative header)
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; if i64.load(base + 8 + i*8) == needle → return 1
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        i64.load
        local.get $needle
        i64.eq
        if
          i32.const 1
          return
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    i32.const 0)
";

/// PMAT-1262: the list-FLOAT-MEMBERSHIP twin — Python `x in xs` / `x not in xs`
/// over a `list[float]`. Structurally identical to [`LIST_CONTAINS_INT_HELPER`]
/// but with an f64 `$needle` and an IEEE-754 `f64.eq` element compare (a float
/// list shares the int list's header + 8-byte stride; only the element `*.load`
/// and the compare opcode differ). Gated on its OWN `needs_list_contains_float`
/// (also non-allocating, so likewise NOT on `needs_heap`). The empty list yields
/// `0` (`x in []` is `False`), matching the int sibling.
///
/// IEEE-754 caveat (shared with the Rust `.contains` lane): `f64.eq` makes
/// `nan == nan` False, so `float('nan') in [float('nan')]` returns `0` here — the
/// same answer Rust's `Vec<f64>::contains` gives (its `PartialEq` is likewise
/// non-reflexive on NaN). CPython's `in` first checks OBJECT IDENTITY (`is`), so
/// it returns `True` when the SAME nan object is stored — a semantics neither the
/// WASM nor the Rust value-only lane models. The common no-NaN case is exact.
const LIST_CONTAINS_FLOAT_HELPER: &str = "\
  ;; __wasm_list_contains_f64(base, needle) = 1 if needle in xs else 0, list[float]
  ;; base → length-prefixed region: i32 count @ base+0, f64 elements @ base+8.
  (func $__wasm_list_contains_f64 (param $base i32) (param $needle f64) (result i32)
    (local $i i32)
    (local $n i32)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        ;; while i < n  (unsigned — count is a non-negative header)
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; if f64.load(base + 8 + i*8) == needle → return 1
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        f64.load
        local.get $needle
        f64.eq
        if
          i32.const 1
          return
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    i32.const 0)
";

/// PMAT-1282: the list-INT-INSERT helper — Python `xs.insert(i, v)` over a
/// `list[int]`. The FIRST list-mutation that both GROWS the live-element count
/// AND SHIFTS the tail (unlike `append`, which only grows at the end, and `pop`,
/// which only shrinks). It mutates the record IN PLACE — the base-pointer never
/// moves — so every alias holding it observes the insertion, the same alias-safe
/// posture `append`/`pop` rely on. It reads/writes the PMAT-968 length-prefixed
/// region (i32 count @ base+0, i32 capacity @ base+4, i64 elements @ base+8) and,
/// like `append`, refuses (traps `unreachable`) when the fixed capacity is full.
///
/// The insert position is clamped EXACTLY as CPython `list.insert` (`listobject.c`
/// `ins1`): a negative index adds the length (`i += n`) and, if still negative,
/// pins to `0` (the front); an index past the end pins to `n` (append position) —
/// never a `Vec::insert`-style panic on an out-of-range index. The clamp math runs
/// in SIGNED i64 (so a very large-magnitude negative index normalises correctly)
/// before narrowing to the i32 slot. The tail `[slot, n)` is shifted right by one
/// slot walking HIGH→LOW (`for j = n; j > slot; j--: elems[j] = elems[j-1]`) so no
/// element is overwritten before it is copied; the new value lands at `slot` and
/// the count header is bumped to `n + 1`.
const LIST_INSERT_INT_HELPER: &str = "\
  ;; __wasm_list_insert_i64(base, idx, val) — Python xs.insert(idx, val), list[int]
  ;; base → i32 count @ base+0, i32 capacity @ base+4, i64 elements @ base+8.
  (func $__wasm_list_insert_i64 (param $base i32) (param $idx i64) (param $val i64)
    (local $n i32)      ;; current element count
    (local $slot i32)   ;; normalized insert position in [0, n]
    (local $j i32)      ;; shift cursor
    (local $s i64)      ;; signed normalized index (CPython clamp math)
    ;; n = count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; capacity guard: if n >= capacity(base+4) → trap (bounded bump heap)
    local.get $n
    local.get $base
    i32.load offset=4
    i32.ge_u
    if
      unreachable
    end
    ;; s = idx ; CPython list.insert clamp: if s < 0 { s += n }
    local.get $idx
    local.set $s
    local.get $s
    i64.const 0
    i64.lt_s
    if
      local.get $s
      local.get $n
      i64.extend_i32_u
      i64.add
      local.set $s
    end
    ;; if s < 0 { s = 0 }  (still negative after += n → clamp to the front)
    local.get $s
    i64.const 0
    i64.lt_s
    if
      i64.const 0
      local.set $s
    end
    ;; if s > n { s = n }  (past the end → clamp to the append position)
    local.get $s
    local.get $n
    i64.extend_i32_u
    i64.gt_s
    if
      local.get $n
      i64.extend_i32_u
      local.set $s
    end
    ;; slot = (i32) s   (s now in [0, n], fits an i32)
    local.get $s
    i32.wrap_i64
    local.set $slot
    ;; shift tail right: for (j = n; j > slot; j--) elems[j] = elems[j-1]
    local.get $n
    local.set $j
    (block $done
      (loop $next
        ;; while j > slot  (unsigned; both in [0, n])
        local.get $j
        local.get $slot
        i32.le_u
        br_if $done
        ;; dst = base + 8 + j*8
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        ;; src value = i64.load(base + 8 + (j-1)*8)
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 1
        i32.sub
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        ;; j -= 1
        local.get $j
        i32.const 1
        i32.sub
        local.set $j
        br $next
      )
    )
    ;; elems[slot] = val:  addr = base + 8 + slot*8
    local.get $base
    i32.const 8
    i32.add
    local.get $slot
    i32.const 8
    i32.mul
    i32.add
    local.get $val
    i64.store
    ;; count = n + 1 (write back to base+0)
    local.get $base
    local.get $n
    i32.const 1
    i32.add
    i32.store)
";

/// PMAT-1282: the list-FLOAT-INSERT twin — Python `xs.insert(i, v)` over a
/// `list[float]`. Structurally identical to [`LIST_INSERT_INT_HELPER`] (same
/// header, same 8-byte stride, same CPython clamp + high→low shift); only the
/// `$val` param and the value store are f64. The tail shift moves whole 8-byte
/// words with `i64.load`/`i64.store` (a pure byte-move that never interprets the
/// payload, exactly like the `reversed` helper), so the shift itself is shared
/// verbatim with the int helper and only the final `elems[slot] = val` store is
/// f64. Gated together with the int helper under the single `needs_list_insert`
/// (the [`xpile_meta_hir::Stmt::ListInsert`] node carries no element-kind
/// discriminant, so both twins emit; the unused one is harmless dead WAT, exactly
/// like the `contains`/`count`/`index` twins).
const LIST_INSERT_FLOAT_HELPER: &str = "\
  ;; __wasm_list_insert_f64(base, idx, val) — Python xs.insert(idx, val), list[float]
  ;; base → i32 count @ base+0, i32 capacity @ base+4, f64 elements @ base+8.
  (func $__wasm_list_insert_f64 (param $base i32) (param $idx i64) (param $val f64)
    (local $n i32)      ;; current element count
    (local $slot i32)   ;; normalized insert position in [0, n]
    (local $j i32)      ;; shift cursor
    (local $s i64)      ;; signed normalized index (CPython clamp math)
    ;; n = count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; capacity guard: if n >= capacity(base+4) → trap (bounded bump heap)
    local.get $n
    local.get $base
    i32.load offset=4
    i32.ge_u
    if
      unreachable
    end
    ;; s = idx ; CPython list.insert clamp: if s < 0 { s += n }
    local.get $idx
    local.set $s
    local.get $s
    i64.const 0
    i64.lt_s
    if
      local.get $s
      local.get $n
      i64.extend_i32_u
      i64.add
      local.set $s
    end
    ;; if s < 0 { s = 0 }  (still negative after += n → clamp to the front)
    local.get $s
    i64.const 0
    i64.lt_s
    if
      i64.const 0
      local.set $s
    end
    ;; if s > n { s = n }  (past the end → clamp to the append position)
    local.get $s
    local.get $n
    i64.extend_i32_u
    i64.gt_s
    if
      local.get $n
      i64.extend_i32_u
      local.set $s
    end
    ;; slot = (i32) s   (s now in [0, n], fits an i32)
    local.get $s
    i32.wrap_i64
    local.set $slot
    ;; shift tail right: for (j = n; j > slot; j--) elems[j] = elems[j-1]
    ;; (an 8-byte word move; i64.load/store never interpret the f64 payload)
    local.get $n
    local.set $j
    (block $done
      (loop $next
        ;; while j > slot  (unsigned; both in [0, n])
        local.get $j
        local.get $slot
        i32.le_u
        br_if $done
        ;; dst = base + 8 + j*8
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        ;; src word = i64.load(base + 8 + (j-1)*8)
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 1
        i32.sub
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        ;; j -= 1
        local.get $j
        i32.const 1
        i32.sub
        local.set $j
        br $next
      )
    )
    ;; elems[slot] = val:  addr = base + 8 + slot*8
    local.get $base
    i32.const 8
    i32.add
    local.get $slot
    i32.const 8
    i32.mul
    i32.add
    local.get $val
    f64.store
    ;; count = n + 1 (write back to base+0)
    local.get $base
    local.get $n
    i32.const 1
    i32.add
    i32.store)
";

/// PMAT-1284: the list DELETE-AT-INDEX helper — Python `del xs[i]` over a
/// `list[int]`/`list[float]`, the in-place MIRROR of [`LIST_INSERT_INT_HELPER`]
/// (grow+shift-right ↔ shrink+shift-left).
///
/// This is the FIRST list-mutation that SHRINKS *and* SHIFTS: `pop()` shrinks
/// only at the END and `insert()` grows+shifts; `del xs[i]` removes the element
/// at an arbitrary position and slides the tail LEFT to close the hole. Because
/// it only shrinks, it needs NO capacity guard and — unlike `append`/`insert`,
/// which grow and so demand a literal-bound list's spare slack — it works on ANY
/// list local carrying a valid base-pointer (a param included, exactly like
/// `pop`): the record only gets smaller, the base-pointer never moves, so every
/// alias observes the deletion and nothing overruns.
///
/// The index follows CPython `del list[i]` / `list.pop(i)` EXACTLY (NOT the
/// forgiving `insert` clamp): a negative index adds the length (`i += n`) and an
/// index still out of `[0, n)` afterwards — including ANY index on an empty list
/// — raises `IndexError`, lowered here to a `unreachable` trap (never a
/// `Vec::remove`-style silent wrap). The clamp/bounds math runs in SIGNED i64 so
/// a large-magnitude negative index normalises correctly before narrowing to the
/// i32 slot. The tail `[slot+1, n)` is shifted left by one slot walking LOW→HIGH
/// (`for j = slot; j+1 < n; j++: elems[j] = elems[j+1]`) so no element is
/// overwritten before it is copied, then the count header drops to `n - 1`.
///
/// The shift moves whole 8-byte words with `i64.load`/`i64.store` — a pure
/// byte-move that never interprets the payload — so ONE helper serves BOTH the
/// `list[int]` (i64) and `list[float]` (f64) element kinds (like `reversed`,
/// unlike `insert`'s typed value store). A `list[bool]` (4-byte i32 stride) is
/// refused at the call site (it would need an i32-stride shift twin, deferred
/// exactly like `insert`/`append`).
const LIST_DELITEM_HELPER: &str = "\
  ;; __wasm_list_delitem(base, idx) — Python `del xs[i]`, list[int]/list[float]
  ;; base → i32 count @ base+0, i32 capacity @ base+4, 8-byte elements @ base+8.
  (func $__wasm_list_delitem (param $base i32) (param $idx i64)
    (local $n i32)      ;; current element count
    (local $slot i32)   ;; normalized delete position in [0, n)
    (local $j i32)      ;; shift cursor
    (local $s i64)      ;; signed normalized index (CPython del math)
    ;; n = count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; s = idx ; CPython del: if s < 0 { s += n }
    local.get $idx
    local.set $s
    local.get $s
    i64.const 0
    i64.lt_s
    if
      local.get $s
      local.get $n
      i64.extend_i32_u
      i64.add
      local.set $s
    end
    ;; bounds: if s < 0 → IndexError → trap (index too negative even after += n)
    local.get $s
    i64.const 0
    i64.lt_s
    if
      unreachable
    end
    ;; bounds: if s >= n → IndexError → trap (covers ANY index on an empty list)
    local.get $s
    local.get $n
    i64.extend_i32_u
    i64.ge_s
    if
      unreachable
    end
    ;; slot = (i32) s   (s now in [0, n), fits an i32)
    local.get $s
    i32.wrap_i64
    local.set $slot
    ;; shift tail left: for (j = slot; j+1 < n; j++) elems[j] = elems[j+1]
    ;; (an 8-byte word move; i64.load/store never interpret the payload)
    local.get $slot
    local.set $j
    (block $done
      (loop $next
        ;; while j+1 < n  →  break once (j+1) >= n
        local.get $j
        i32.const 1
        i32.add
        local.get $n
        i32.ge_u
        br_if $done
        ;; dst = base + 8 + j*8
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        ;; src word = i64.load(base + 8 + (j+1)*8)
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 1
        i32.add
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        ;; j += 1
        local.get $j
        i32.const 1
        i32.add
        local.set $j
        br $next
      )
    )
    ;; count = n - 1 (write back to base+0; n >= 1 here — the bounds trap above
    ;; already rejected every index when n == 0)
    local.get $base
    local.get $n
    i32.const 1
    i32.sub
    i32.store)
";

/// PMAT-1285: the list-INT-REMOVE helper — Python `xs.remove(v)` over a
/// `list[int]`. `list.remove` deletes the FIRST element EQUAL to `v` (a
/// value compare, NOT an index like `del xs[i]`), so this helper FUSES the
/// two shipped primitives: a linear scan for the first match (exactly like
/// [`LIST_INDEX_INT_HELPER`], `i64.eq` per element) followed by the same
/// left-shrinking tail shift as [`LIST_DELITEM_HELPER`]. A MISS (no element
/// equals `v`, including ANY value on an empty list) raises Python
/// `ValueError`, lowered here to a deterministic `unreachable` trap — the
/// same posture as `index`'s miss and `del`'s out-of-range trap.
///
/// Because it only SHRINKS (the base-pointer never moves, no overrun), it
/// imposes NO growable-list precondition and — like `del`/`pop` — accepts ANY
/// scalar list local, a PARAM included (every alias observes the removal). The
/// tail `[slot+1, n)` is shifted left one slot walking LOW→HIGH so no element
/// is overwritten before it is copied; the shift moves whole 8-byte words with
/// `i64.load`/`i64.store` (a pure byte-move that never interprets the payload).
/// Unlike `del` — a pure word move served by ONE helper — `remove` needs a
/// TYPED value compare, so it has an f64 twin ([`LIST_REMOVE_FLOAT_HELPER`]);
/// the SHIFT stays an i64 word move in both. A `list[bool]` (i32 stride) is
/// refused at the call site (an i32 twin is deferred, like `insert`).
const LIST_REMOVE_INT_HELPER: &str = "\
  ;; __wasm_list_remove_i64(base, needle) — Python `xs.remove(v)`, list[int]
  ;; base → i32 count @ base+0, i32 capacity @ base+4, 8-byte elements @ base+8.
  ;; Scans for the FIRST element == needle (i64.eq), shifts the tail left to
  ;; close the hole, then drops the count. No match → `unreachable` (ValueError).
  (func $__wasm_list_remove_i64 (param $base i32) (param $needle i64)
    (local $n i32)      ;; current element count
    (local $i i32)      ;; scan cursor / found slot
    (local $j i32)      ;; shift cursor
    ;; n = count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; find the first i where elems[i] == needle
    i32.const 0
    local.set $i
    (block $found
      (block $miss
        (loop $next
          ;; if i >= n → miss (value absent → ValueError)
          local.get $i
          local.get $n
          i32.ge_u
          br_if $miss
          ;; if i64.load(base + 8 + i*8) == needle → found (i is the slot)
          local.get $base
          i32.const 8
          i32.add
          local.get $i
          i32.const 8
          i32.mul
          i32.add
          i64.load
          local.get $needle
          i64.eq
          br_if $found
          ;; i += 1
          local.get $i
          i32.const 1
          i32.add
          local.set $i
          br $next
        )
      )
      ;; v not in xs → Python ValueError; trap deterministically.
      unreachable
    )
    ;; shift tail left: for (j = i; j+1 < n; j++) elems[j] = elems[j+1]
    ;; (an 8-byte word move; i64.load/store never interpret the payload)
    local.get $i
    local.set $j
    (block $done
      (loop $shift
        ;; while j+1 < n  →  break once (j+1) >= n
        local.get $j
        i32.const 1
        i32.add
        local.get $n
        i32.ge_u
        br_if $done
        ;; dst = base + 8 + j*8
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        ;; src word = i64.load(base + 8 + (j+1)*8)
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 1
        i32.add
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        ;; j += 1
        local.get $j
        i32.const 1
        i32.add
        local.set $j
        br $shift
      )
    )
    ;; count = n - 1 (n >= 1 here — the miss trap rejected the empty/absent cases)
    local.get $base
    local.get $n
    i32.const 1
    i32.sub
    i32.store)
";

/// PMAT-1285: the list-FLOAT-REMOVE twin — Python `xs.remove(v)` over a
/// `list[float]`. Structurally identical to [`LIST_REMOVE_INT_HELPER`] but the
/// find loop loads/compares with `f64.load`/`f64.eq` (the shift stays an i64
/// word move — a verbatim 8-byte copy, safer for NaN payloads than an
/// f64.load/store pair). Same IEEE-754 caveat as `index`/`count`: a NaN value
/// never matches (`nan != nan`), so `xs.remove(nan)` traps as a ValueError,
/// though CPython removes the element when the SAME nan OBJECT is stored (an
/// identity semantics the value-only lane does not model). Gated together with
/// the int helper under the single `needs_list_remove`.
const LIST_REMOVE_FLOAT_HELPER: &str = "\
  ;; __wasm_list_remove_f64(base, needle) — Python `xs.remove(v)`, list[float]
  ;; base → i32 count @ base+0, 8-byte f64 elements @ base+8. Scans with f64.eq;
  ;; the tail shift is an i64 word move. No match → `unreachable` (ValueError).
  (func $__wasm_list_remove_f64 (param $base i32) (param $needle f64)
    (local $n i32)
    (local $i i32)
    (local $j i32)
    local.get $base
    i32.load
    local.set $n
    i32.const 0
    local.set $i
    (block $found
      (block $miss
        (loop $next
          local.get $i
          local.get $n
          i32.ge_u
          br_if $miss
          ;; if f64.load(base + 8 + i*8) == needle → found
          local.get $base
          i32.const 8
          i32.add
          local.get $i
          i32.const 8
          i32.mul
          i32.add
          f64.load
          local.get $needle
          f64.eq
          br_if $found
          local.get $i
          i32.const 1
          i32.add
          local.set $i
          br $next
        )
      )
      unreachable
    )
    ;; shift tail left (8-byte word move via i64.load/store)
    local.get $i
    local.set $j
    (block $done
      (loop $shift
        local.get $j
        i32.const 1
        i32.add
        local.get $n
        i32.ge_u
        br_if $done
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 1
        i32.add
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        local.get $j
        i32.const 1
        i32.add
        local.set $j
        br $shift
      )
    )
    local.get $base
    local.get $n
    i32.const 1
    i32.sub
    i32.store)
";

/// PMAT-1289: the list-INT-INDEXED-POP helper — Python `xs.pop(i)` over a
/// `list[int]`. The VALUE-RETURNING sibling of [`LIST_DELITEM_HELPER`]: the
/// SAME CPython index math (a negative index adds the length; an index still
/// out of `[0, n)` afterwards — including ANY index on an empty list — raises
/// `IndexError`, lowered to a `unreachable` trap, never a silent wrap or a
/// last-element fallback), the SAME low→high left-shifting tail move, and the
/// SAME count drop — PLUS a typed load of `elems[slot]` BEFORE the shift
/// closes the hole, returned as the expression value (`xs.pop(i)` evaluates to
/// the removed element; `del xs[i]` is void).
///
/// The shift stays a pure 8-byte word move (`i64.load`/`i64.store`, never
/// interpreting the payload) in BOTH twins — only the value load/return is
/// typed, which is why (like `remove`/`insert`, unlike `del`'s single shared
/// helper) an f64 twin ([`LIST_POP_INDEX_FLOAT_HELPER`]) exists. Because a pop
/// only SHRINKS (the base-pointer never moves, no overrun), there is NO
/// growable-list precondition: ANY named scalar list — a PARAM included —
/// qualifies, exactly like `del`/`remove`/`pop()`. A `list[bool]` (4-byte i32
/// stride) is refused at the call site (an i32-stride twin is deferred, like
/// `insert`/`del`/`remove`).
const LIST_POP_INDEX_INT_HELPER: &str = "\
  ;; __wasm_list_pop_idx_i64(base, idx) — Python `xs.pop(i)`, list[int]
  ;; base → i32 count @ base+0, i32 capacity @ base+4, 8-byte elements @ base+8.
  ;; Normalises the index (neg += n; still out of [0,n) → unreachable =
  ;; IndexError), loads the removed element (the result), shifts the tail left,
  ;; drops the count, returns the element.
  (func $__wasm_list_pop_idx_i64 (param $base i32) (param $idx i64) (result i64)
    (local $n i32)      ;; current element count
    (local $slot i32)   ;; normalized pop position in [0, n)
    (local $j i32)      ;; shift cursor
    (local $s i64)      ;; signed normalized index (CPython pop math)
    (local $v i64)      ;; the removed element (the result)
    ;; n = count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; s = idx ; CPython pop: if s < 0 { s += n }
    local.get $idx
    local.set $s
    local.get $s
    i64.const 0
    i64.lt_s
    if
      local.get $s
      local.get $n
      i64.extend_i32_u
      i64.add
      local.set $s
    end
    ;; bounds: if s < 0 → IndexError → trap (index too negative even after += n)
    local.get $s
    i64.const 0
    i64.lt_s
    if
      unreachable
    end
    ;; bounds: if s >= n → IndexError → trap (covers ANY index on an empty list)
    local.get $s
    local.get $n
    i64.extend_i32_u
    i64.ge_s
    if
      unreachable
    end
    ;; slot = (i32) s   (s now in [0, n), fits an i32)
    local.get $s
    i32.wrap_i64
    local.set $slot
    ;; v = elems[slot] — the result, loaded BEFORE the shift closes the hole
    local.get $base
    i32.const 8
    i32.add
    local.get $slot
    i32.const 8
    i32.mul
    i32.add
    i64.load
    local.set $v
    ;; shift tail left: for (j = slot; j+1 < n; j++) elems[j] = elems[j+1]
    ;; (an 8-byte word move; i64.load/store never interpret the payload)
    local.get $slot
    local.set $j
    (block $done
      (loop $next
        ;; while j+1 < n  →  break once (j+1) >= n
        local.get $j
        i32.const 1
        i32.add
        local.get $n
        i32.ge_u
        br_if $done
        ;; dst = base + 8 + j*8
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        ;; src word = i64.load(base + 8 + (j+1)*8)
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 1
        i32.add
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        ;; j += 1
        local.get $j
        i32.const 1
        i32.add
        local.set $j
        br $next
      )
    )
    ;; count = n - 1 (n >= 1 here — the bounds trap already rejected n == 0)
    local.get $base
    local.get $n
    i32.const 1
    i32.sub
    i32.store
    ;; the removed element (the expression value)
    local.get $v)
";

/// PMAT-1289: the f64 twin of [`LIST_POP_INDEX_INT_HELPER`] — Python
/// `xs.pop(i)` over a `list[float]`. IDENTICAL index math, tail shift (still a
/// pure i64 word move — it never interprets the payload, so an f64 bit pattern
/// moves losslessly, NaN payloads included), and count drop; only the value
/// load (`f64.load`), the `$v` local, and the `(result f64)` are typed.
const LIST_POP_INDEX_FLOAT_HELPER: &str = "\
  ;; __wasm_list_pop_idx_f64(base, idx) — Python `xs.pop(i)`, list[float]
  ;; base → i32 count @ base+0, i32 capacity @ base+4, 8-byte elements @ base+8.
  ;; Same normalise/trap/shift/count-- as the i64 twin; only the value load and
  ;; the result type are f64 (the shift stays a pure i64 word move).
  (func $__wasm_list_pop_idx_f64 (param $base i32) (param $idx i64) (result f64)
    (local $n i32)      ;; current element count
    (local $slot i32)   ;; normalized pop position in [0, n)
    (local $j i32)      ;; shift cursor
    (local $s i64)      ;; signed normalized index (CPython pop math)
    (local $v f64)      ;; the removed element (the result)
    ;; n = count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; s = idx ; CPython pop: if s < 0 { s += n }
    local.get $idx
    local.set $s
    local.get $s
    i64.const 0
    i64.lt_s
    if
      local.get $s
      local.get $n
      i64.extend_i32_u
      i64.add
      local.set $s
    end
    ;; bounds: if s < 0 → IndexError → trap (index too negative even after += n)
    local.get $s
    i64.const 0
    i64.lt_s
    if
      unreachable
    end
    ;; bounds: if s >= n → IndexError → trap (covers ANY index on an empty list)
    local.get $s
    local.get $n
    i64.extend_i32_u
    i64.ge_s
    if
      unreachable
    end
    ;; slot = (i32) s   (s now in [0, n), fits an i32)
    local.get $s
    i32.wrap_i64
    local.set $slot
    ;; v = elems[slot] — the result, loaded (typed) BEFORE the shift
    local.get $base
    i32.const 8
    i32.add
    local.get $slot
    i32.const 8
    i32.mul
    i32.add
    f64.load
    local.set $v
    ;; shift tail left: for (j = slot; j+1 < n; j++) elems[j] = elems[j+1]
    ;; (an 8-byte word move; i64.load/store never interpret the payload)
    local.get $slot
    local.set $j
    (block $done
      (loop $next
        ;; while j+1 < n  →  break once (j+1) >= n
        local.get $j
        i32.const 1
        i32.add
        local.get $n
        i32.ge_u
        br_if $done
        ;; dst = base + 8 + j*8
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        ;; src word = i64.load(base + 8 + (j+1)*8)
        local.get $base
        i32.const 8
        i32.add
        local.get $j
        i32.const 1
        i32.add
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        ;; j += 1
        local.get $j
        i32.const 1
        i32.add
        local.set $j
        br $next
      )
    )
    ;; count = n - 1 (n >= 1 here — the bounds trap already rejected n == 0)
    local.get $base
    local.get $n
    i32.const 1
    i32.sub
    i32.store
    ;; the removed element (the expression value)
    local.get $v)
";

/// PMAT-1274: the list-INT-COUNT helper — Python `xs.count(x)` over a
/// `list[int]`. A NON-allocating read (a linear scan, like `contains`/`sum`),
/// so it rides its OWN gate (`needs_list_count`), NOT `needs_heap`. It reads the
/// PMAT-968 length-prefixed region (i32 count @ base+0, packed i64 elements @
/// base+8), compares each element to `$needle` with `i64.eq`, and accumulates
/// the number of matches into `$c`, returned as an i64 (so `[].count(x) == 0`
/// and no match yields `0` — exactly CPython's `list.count`). Unlike `contains`
/// this does NOT short-circuit: EVERY element is inspected (a full count).
const LIST_COUNT_INT_HELPER: &str = "\
  ;; __wasm_list_count_i64(base, needle) = number of elements == needle, list[int]
  ;; base → length-prefixed region: i32 count @ base+0, i64 elements @ base+8.
  (func $__wasm_list_count_i64 (param $base i32) (param $needle i64) (result i64)
    (local $i i32)
    (local $n i32)
    (local $c i64)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    i32.const 0
    local.set $i
    i64.const 0
    local.set $c
    (block $done
      (loop $next
        ;; while i < n  (unsigned — count is a non-negative header)
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; if i64.load(base + 8 + i*8) == needle → c += 1
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        i64.load
        local.get $needle
        i64.eq
        if
          local.get $c
          i64.const 1
          i64.add
          local.set $c
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    local.get $c)
";

/// PMAT-1274: the list-FLOAT-COUNT twin — Python `xs.count(x)` over a
/// `list[float]`. Structurally identical to [`LIST_COUNT_INT_HELPER`] but with
/// an f64 `$needle` and an IEEE-754 `f64.eq` element compare (only the element
/// `*.load` and the compare opcode differ). Gated on its OWN `needs_list_count`.
/// The IEEE-754 caveat is shared with `contains`: `f64.eq` makes `nan == nan`
/// False, so a NaN needle is never counted (Python `list.count` checks object
/// identity first, so a stored SAME nan object counts — a semantics the
/// value-only lane does not model; the common no-NaN case is exact).
const LIST_COUNT_FLOAT_HELPER: &str = "\
  ;; __wasm_list_count_f64(base, needle) = number of elements == needle, list[float]
  ;; base → length-prefixed region: i32 count @ base+0, f64 elements @ base+8.
  (func $__wasm_list_count_f64 (param $base i32) (param $needle f64) (result i64)
    (local $i i32)
    (local $n i32)
    (local $c i64)
    local.get $base
    i32.load
    local.set $n
    i32.const 0
    local.set $i
    i64.const 0
    local.set $c
    (block $done
      (loop $next
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; if f64.load(base + 8 + i*8) == needle → c += 1
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        f64.load
        local.get $needle
        f64.eq
        if
          local.get $c
          i64.const 1
          i64.add
          local.set $c
        end
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    local.get $c)
";

/// PMAT-1274: the list-INT-INDEX helper — Python `xs.index(x)` over a
/// `list[int]`. A NON-allocating read (a linear scan, like `contains`), so it
/// rides its OWN gate (`needs_list_index`), NOT `needs_heap`. It reads the
/// PMAT-968 length-prefixed region, compares each element to `$needle` with
/// `i64.eq`, and returns the i64 index of the FIRST match (a left-to-right scan
/// — Python `list.index` returns the LOWEST matching index). When the scan is
/// exhausted with no match the element is ABSENT, which Python signals by
/// raising `ValueError`; the WASM lane traps via `unreachable` (the same
/// posture as the out-of-bounds `IndexError` trap and the Rust `.expect(…)`
/// panic the scalar lane emits). `[].index(x)` therefore traps (empty ⇒ no
/// match), matching CPython raising `ValueError` on an empty list.
const LIST_INDEX_INT_HELPER: &str = "\
  ;; __wasm_list_index_i64(base, needle) = index of first elem == needle, list[int]
  ;; base → length-prefixed region: i32 count @ base+0, i64 elements @ base+8.
  ;; No match → `unreachable` (Python ValueError).
  (func $__wasm_list_index_i64 (param $base i32) (param $needle i64) (result i64)
    (local $i i32)
    (local $n i32)
    local.get $base
    i32.load
    local.set $n
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; if i64.load(base + 8 + i*8) == needle → return i (as i64)
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        i64.load
        local.get $needle
        i64.eq
        if
          local.get $i
          i64.extend_i32_u
          return
        end
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    ;; x not in xs → Python ValueError; trap deterministically.
    unreachable)
";

/// PMAT-1274: the list-FLOAT-INDEX twin — Python `xs.index(x)` over a
/// `list[float]`. Structurally identical to [`LIST_INDEX_INT_HELPER`] but with
/// an f64 `$needle` and an IEEE-754 `f64.eq` compare. Gated on its OWN
/// `needs_list_index`. Same IEEE-754 caveat as the count twin: a NaN needle
/// never matches, so `xs.index(nan)` traps (Python raises `ValueError` for the
/// value-compare, though it returns the position when the SAME nan object is
/// stored — an identity semantics the value-only lane does not model).
const LIST_INDEX_FLOAT_HELPER: &str = "\
  ;; __wasm_list_index_f64(base, needle) = index of first elem == needle, list[float]
  ;; base → length-prefixed region: i32 count @ base+0, f64 elements @ base+8.
  ;; No match → `unreachable` (Python ValueError).
  (func $__wasm_list_index_f64 (param $base i32) (param $needle f64) (result i64)
    (local $i i32)
    (local $n i32)
    local.get $base
    i32.load
    local.set $n
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; if f64.load(base + 8 + i*8) == needle → return i (as i64)
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        f64.load
        local.get $needle
        f64.eq
        if
          local.get $i
          i64.extend_i32_u
          return
        end
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    unreachable)
";

/// PMAT-1251: `any(xs)` / `all(xs)` over a `list[bool]` — the THIRD list-reduction
/// family (after `sum` and `min`/`max`), a non-allocating boolean fold. The list
/// is a PMAT-968 length-prefixed region whose elements are `i32` 0/1 (the
/// PMAT-1251 `list[bool]` element type: a 4-byte i32 stride, like `list[f32]`).
/// The `$is_all` param selects the reduction at the call site (`i32.const 1` for
/// `all`, `0` for `any`) so ONE helper serves both directions.
///
/// Semantics match CPython/the iterator adaptors exactly: `all` short-circuits
/// **False** (returns 0) on the FIRST falsey element; `any` short-circuits
/// **True** (returns 1) on the first truthy one; and when the loop is exhausted
/// with no short-circuit the result is `is_all` itself — so `all([]) == True`
/// (1) and `any([]) == False` (0) (the empty-list identities) fall straight out,
/// as does `all([1,1,1]) == 1` / `any([0,0,0]) == 0`. This mirrors the Rust
/// `.iter().all(|&b| b)` / `.any(|&b| b)` lane. Reads linear memory, allocates
/// nothing (an i32 bool, not a new object), so it is gated on its OWN
/// `needs_list_bool_reduce`, NOT `needs_heap`.
const LIST_BOOL_REDUCE_HELPER: &str = "\
  ;; __wasm_list_bool_reduce(base, is_all) = all(xs) if is_all else any(xs), list[bool]
  ;; base → length-prefixed region: i32 count @ base+0, i32 (0/1) elements @ base+8.
  (func $__wasm_list_bool_reduce (param $base i32) (param $is_all i32) (result i32)
    (local $i i32)
    (local $n i32)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    i32.const 0
    local.set $i
    (block $done
      (loop $next
        ;; while i < n  (unsigned — count is a non-negative header)
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        ;; x = i32.load(base + 8 + i*4)  — a bool 0/1 (4-byte i32 stride)
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 4
        i32.mul
        i32.add
        i32.load
        ;; b = (x != 0) — normalise the loaded element to a 0/1 truthiness.
        i32.const 0
        i32.ne
        ;; short-circuit iff (is_all XOR b):
        ;;   all (is_all=1): break on a FALSEY element (b=0 → 1^0 = 1);
        ;;   any (is_all=0): break on a TRUTHY element (b=1 → 0^1 = 1).
        local.get $is_all
        i32.xor
        if
          ;; the decided result: all → 0 (False), any → 1 (True) = !is_all
          local.get $is_all
          i32.eqz
          return
        end
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      )
    )
    ;; loop exhausted (incl. the empty list): all → 1, any → 0; both == is_all.
    local.get $is_all)
";

/// PMAT-1252: the list-INT-SORT reduction helper — Python `sorted(xs)` /
/// `sorted(xs, reverse=True)` over a `list[int]`. This is the FIRST list op that
/// RETURNS a new list, so unlike the sum/min/max/any/all folds it ALLOCATES: it
/// bump-allocates a fresh PMAT-968 length-prefixed record (`$__alloc(8 + n*8)`),
/// copies the source elements in, then INSERTION-SORTS the copy in place and
/// returns the new base-pointer — the source list is never mutated (Python's
/// `sorted` yields a new list). It calls `$__alloc`, so a module using it forces
/// `needs_heap` (via [`expr_has_heap_op`]) and rides its OWN `needs_list_sorted`.
///
/// Insertion sort is chosen for its compactness and STABILITY: the inner shift
/// fires only on a STRICT compare (`prev > key` ascending, `prev < key`
/// descending), so equal elements never cross and their input order is preserved
/// — matching CPython's stable `sorted` (and `sorted(reverse=True)`, which is a
/// stable descending sort, NOT ascending-then-reversed). The `$reverse` param
/// selects the direction at the call site (`i32.const 1` descending, `0`
/// ascending). The EMPTY list does NOT trap (unlike min/max): `sorted([]) == []`,
/// so `n == 0` allocates an empty record and returns it. Integer compares are
/// signed (`i64.gt_s`/`i64.lt_s`), the scalar subset's i64 posture, matching the
/// Rust `.sort()` / `.sort_by(desc)` lane.
const LIST_SORTED_INT_HELPER: &str = "\
  ;; __wasm_list_sorted_i64(base, reverse) -> a NEW sorted list[int]
  ;; base → length-prefixed region: i32 count @ base+0, i64 elements @ base+8.
  (func $__wasm_list_sorted_i64 (param $base i32) (param $reverse i32) (result i32)
    (local $n i32)
    (local $r i32)
    (local $ra i32)
    (local $i i32)
    (local $j i32)
    (local $key i64)
    (local $prev i64)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; r = __alloc(8 + n*8); write the i32 count header at r+0
    local.get $n
    i32.const 8
    i32.mul
    i32.const 8
    i32.add
    call $__alloc
    local.set $r
    local.get $r
    local.get $n
    i32.store
    ;; ra = r + 8 (address of element 0 in the new record; reused throughout)
    local.get $r
    i32.const 8
    i32.add
    local.set $ra
    ;; copy: for i in 0..n: r[i] = base[i]
    i32.const 0
    local.set $i
    (block $cpd
      (loop $cp
        local.get $i
        local.get $n
        i32.ge_u
        br_if $cpd
        ;; dst = ra + i*8
        local.get $ra
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        ;; val = i64.load(base + 8 + i*8)
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $cp
      )
    )
    ;; insertion sort r in place: for i in 1..n
    i32.const 1
    local.set $i
    (block $srtd
      (loop $srt
        local.get $i
        local.get $n
        i32.ge_u
        br_if $srtd
        ;; key = r[i]
        local.get $ra
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        i64.load
        local.set $key
        ;; j = i (the hole; shift larger/smaller predecessors up into it)
        local.get $i
        local.set $j
        (block $ind
          (loop $inn
            ;; if j == 0, the hole reached the front — stop
            local.get $j
            i32.eqz
            br_if $ind
            ;; prev = r[j-1]
            local.get $ra
            local.get $j
            i32.const 1
            i32.sub
            i32.const 8
            i32.mul
            i32.add
            i64.load
            local.set $prev
            ;; should_shift = reverse ? (prev < key) : (prev > key) — STRICT (stable)
            local.get $reverse
            if (result i32)
              local.get $prev
              local.get $key
              i64.lt_s
            else
              local.get $prev
              local.get $key
              i64.gt_s
            end
            ;; stop when NOT shifting (eqz of should_shift → break)
            i32.eqz
            br_if $ind
            ;; r[j] = prev (shift the predecessor up into the hole)
            local.get $ra
            local.get $j
            i32.const 8
            i32.mul
            i32.add
            local.get $prev
            i64.store
            ;; j -= 1
            local.get $j
            i32.const 1
            i32.sub
            local.set $j
            br $inn
          )
        )
        ;; r[j] = key (drop the key into the final hole)
        local.get $ra
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        local.get $key
        i64.store
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $srt
      )
    )
    local.get $r)
";

/// PMAT-1252: the list-FLOAT-SORT twin — Python `sorted(xs)` / `sorted(xs,
/// reverse=True)` over a `list[float]`. Structurally identical to
/// [`LIST_SORTED_INT_HELPER`] but with an f64 key/predecessor and IEEE-754
/// `f64.gt`/`f64.lt` compares (a float list shares the int list's header +
/// 8-byte stride; only the element `*.load`/`*.store` and the compare opcode
/// differ). Gated on its OWN `needs_list_sorted_float`. Empty list → a fresh
/// empty list (no trap), like the int sibling. NaN elements: an IEEE-754
/// compare with NaN is always false, so a NaN never triggers a shift and the
/// result is unspecified around it — matching CPython's documented undefined
/// NaN-sort behaviour (and the Rust `.sort_by(partial_cmp)` lane's Equal
/// fallback); it is NOT a trap.
const LIST_SORTED_FLOAT_HELPER: &str = "\
  ;; __wasm_list_sorted_f64(base, reverse) -> a NEW sorted list[float]
  ;; base → length-prefixed region: i32 count @ base+0, f64 elements @ base+8.
  (func $__wasm_list_sorted_f64 (param $base i32) (param $reverse i32) (result i32)
    (local $n i32)
    (local $r i32)
    (local $ra i32)
    (local $i i32)
    (local $j i32)
    (local $key f64)
    (local $prev f64)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; r = __alloc(8 + n*8); write the i32 count header at r+0
    local.get $n
    i32.const 8
    i32.mul
    i32.const 8
    i32.add
    call $__alloc
    local.set $r
    local.get $r
    local.get $n
    i32.store
    ;; ra = r + 8 (address of element 0 in the new record; reused throughout)
    local.get $r
    i32.const 8
    i32.add
    local.set $ra
    ;; copy: for i in 0..n: r[i] = base[i]
    i32.const 0
    local.set $i
    (block $cpd
      (loop $cp
        local.get $i
        local.get $n
        i32.ge_u
        br_if $cpd
        local.get $ra
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        local.get $base
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        f64.load
        f64.store
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $cp
      )
    )
    ;; insertion sort r in place: for i in 1..n
    i32.const 1
    local.set $i
    (block $srtd
      (loop $srt
        local.get $i
        local.get $n
        i32.ge_u
        br_if $srtd
        local.get $ra
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        f64.load
        local.set $key
        local.get $i
        local.set $j
        (block $ind
          (loop $inn
            local.get $j
            i32.eqz
            br_if $ind
            local.get $ra
            local.get $j
            i32.const 1
            i32.sub
            i32.const 8
            i32.mul
            i32.add
            f64.load
            local.set $prev
            ;; should_shift = reverse ? (prev < key) : (prev > key) — STRICT (stable)
            local.get $reverse
            if (result i32)
              local.get $prev
              local.get $key
              f64.lt
            else
              local.get $prev
              local.get $key
              f64.gt
            end
            i32.eqz
            br_if $ind
            local.get $ra
            local.get $j
            i32.const 8
            i32.mul
            i32.add
            local.get $prev
            f64.store
            local.get $j
            i32.const 1
            i32.sub
            local.set $j
            br $inn
          )
        )
        local.get $ra
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        local.get $key
        f64.store
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $srt
      )
    )
    local.get $r)
";

/// PMAT-1291: the SET→LIST materialisation helper — the source half of
/// `sorted(s)` over a `set[int]`. Python `sorted(s)` first materialises the set
/// to a list of its (unique) elements, then sorts; this helper does the first
/// step, copying an int set's keys into a FRESH PMAT-968 `list[int]` record so
/// [`LIST_SORTED_INT_HELPER`] can copy-and-sort it into the final result.
///
/// A set rides the bump-heap open-assoc ABI (PMAT-995): an `i32` live count @
/// `base+0`, an `i32` capacity @ `base+4`, then fixed [`DICT_ENTRY_SIZE`] (16)
/// byte entries from `base+8` ([`LIST_ELEMS_OFFSET`]) with the i64 key @
/// `entry+0` (the value half is a set dummy). This helper reads entry `i`'s key
/// (`sa + i*16`) and packs it into the new list at `ra + i*8` — a set is
/// dup-free by construction, so the materialised list already holds the unique
/// elements, exactly as CPython's `list(s)`. It calls `$__alloc`, so a module
/// using it forces `needs_heap` (which `sorted` already forces via
/// [`expr_has_heap_op`]); it rides its OWN `needs_set_to_list` gate.
///
/// ★ ONLY int sets (i64 keys → a `list[int]` result) are materialised — a `str`
/// set would produce a `list[str]`, which the WASM list subset does not model
/// ([`map_list_elem_type`] refuses it), so `sorted(str_set)` is refused upstream
/// at the list-typing level (and defensively in [`emit_list_sorted`]). The EMPTY
/// set does NOT trap: `n == 0` allocs an empty record → `sorted(set()) == []`.
///
/// ★ ORDER: the set is walked in STORAGE order, but the caller ALWAYS re-sorts
/// the result (this helper exists solely as `sorted`'s source), so the final
/// order is deterministic and CPython-exact regardless of the set's arbitrary
/// hash/storage order — which is exactly why `sorted(s)` is tractable while a
/// bare `list(s)` (arbitrary order, refused) is not.
const SET_TO_LIST_INT_HELPER: &str = "\
  ;; __wasm_set_to_list_i64(set) -> a NEW list[int] of the set's keys
  ;; set → open-assoc region: i32 count @ set+0, 16-byte entries @ set+8 (key @ entry+0)
  ;; result → length-prefixed list: i32 count @ r+0, i64 elements @ r+8
  (func $__wasm_set_to_list_i64 (param $s i32) (result i32)
    (local $n i32)
    (local $r i32)
    (local $ra i32)
    (local $sa i32)
    (local $i i32)
    ;; n = live-entry count (i32 header @ s+0)
    local.get $s
    i32.load
    local.set $n
    ;; r = __alloc(8 + n*8); write the i32 count header at r+0
    local.get $n
    i32.const 8
    i32.mul
    i32.const 8
    i32.add
    call $__alloc
    local.set $r
    local.get $r
    local.get $n
    i32.store
    ;; ra = r + 8 (address of list element 0)
    local.get $r
    i32.const 8
    i32.add
    local.set $ra
    ;; sa = s + 8 (address of set entry 0 — LIST_ELEMS_OFFSET)
    local.get $s
    i32.const 8
    i32.add
    local.set $sa
    ;; for i in 0..n: r[i] = key of set entry i
    ;;   dest = ra + i*8 ; key = i64.load(sa + i*16)
    i32.const 0
    local.set $i
    (block $cpd
      (loop $cp
        local.get $i
        local.get $n
        i32.ge_u
        br_if $cpd
        local.get $ra
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        local.get $sa
        local.get $i
        i32.const 16
        i32.mul
        i32.add
        i64.load
        i64.store
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $cp
      )
    )
    local.get $r)
";

/// PMAT-1253: the list-REVERSE helper — Python `reversed(xs)` /
/// `list(reversed(xs))` / `xs[::-1]` over a `list[int]` / `list[float]`. Like
/// [`LIST_SORTED_INT_HELPER`] this is a list-VALUED op that RETURNS a NEW list,
/// so it ALLOCATES: it bump-allocates a fresh PMAT-968 length-prefixed record
/// (`$__alloc(8 + n*8)`) and copies the source elements in BACK-TO-FRONT
/// (`r[i] = base[n-1-i]`), leaving the new base-pointer — the source list is
/// never mutated (Python's `reversed`/`[::-1]` yields a new sequence). It calls
/// `$__alloc`, so a module using it forces `needs_heap` (via [`expr_has_heap_op`])
/// and rides its OWN `needs_list_reversed`.
///
/// ONE helper serves BOTH `list[int]` and `list[float]`: reversal MOVES 8-byte
/// words verbatim and NEVER interprets them, so an f64's bit pattern copied as an
/// i64 word is lossless (and, unlike an `f64.load`/`f64.store` pair, an
/// `i64.load`/`i64.store` cannot canonicalise a NaN payload — a strictly safer
/// move for float data). This is why — unlike [`emit_list_sorted`], which needs
/// TYPED compares (`i64.gt_s` vs `f64.gt`) and thus two helpers — reversal needs
/// only one. A `list[bool]` (4-byte i32 stride) is refused at emit for parity
/// with `sorted` (a distinct-stride helper is deferred). The EMPTY list does NOT
/// trap: `list(reversed([])) == []`, so `n == 0` allocates an empty record and
/// returns it.
const LIST_REVERSED_HELPER: &str = "\
  ;; __wasm_list_reversed_i64(base) -> a NEW reversed list (8-byte stride)
  ;; base → length-prefixed region: i32 count @ base+0, 8-byte elements @ base+8.
  ;; Reversal MOVES 8-byte words verbatim (never interpreting them), so this ONE
  ;; helper serves BOTH list[int] and list[float]: r[i] = base[n-1-i].
  (func $__wasm_list_reversed_i64 (param $base i32) (result i32)
    (local $n i32)
    (local $r i32)
    (local $ra i32)
    (local $ba i32)
    (local $i i32)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; r = __alloc(8 + n*8); write the i32 count header at r+0
    local.get $n
    i32.const 8
    i32.mul
    i32.const 8
    i32.add
    call $__alloc
    local.set $r
    local.get $r
    local.get $n
    i32.store
    ;; ra = r + 8 (element 0 of the new record); ba = base + 8 (source element 0)
    local.get $r
    i32.const 8
    i32.add
    local.set $ra
    local.get $base
    i32.const 8
    i32.add
    local.set $ba
    ;; copy back-to-front: for i in 0..n: r[i] = base[n-1-i]
    i32.const 0
    local.set $i
    (block $cpd
      (loop $cp
        local.get $i
        local.get $n
        i32.ge_u
        br_if $cpd
        ;; dst = ra + i*8
        local.get $ra
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        ;; val = i64.load(ba + (n-1-i)*8)  — a verbatim 8-byte word move
        local.get $ba
        local.get $n
        i32.const 1
        i32.sub
        local.get $i
        i32.sub
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $cp
      )
    )
    local.get $r)
";

/// PMAT-1286: the IN-PLACE list-REVERSE helper — Python `xs.reverse()` over a
/// `list[int]` / `list[float]` (`Stmt::ListMutate` with `ListMutateOp::Reverse`).
/// Unlike [`LIST_REVERSED_HELPER`] (the allocating `reversed(xs)` / `xs[::-1]`
/// that RETURNS a fresh list), this reverses the record IN PLACE and returns
/// nothing: a two-pointer word swap `base[i] <-> base[n-1-i]` for `i` in
/// `0..n/2`. Because it mutates the SAME region, the base-pointer never moves, so
/// every alias holding it observes the reversal (the alias-safe posture the
/// append/insert/del/remove in-place mutators share).
///
/// ONE helper serves BOTH `list[int]` and `list[float]`: a swap MOVES two 8-byte
/// words verbatim and NEVER interprets them, so an f64's bit pattern moved as an
/// i64 word is lossless (and an `i64.load`/`i64.store` — unlike `f64.load`/`store`
/// — cannot canonicalise a NaN payload). This is the same insight that lets
/// [`LIST_REVERSED_HELPER`] and [`LIST_CONCAT_HELPER`] use one helper each; only
/// the TYPED-compare ops (`sorted`/`min`/`max`) need int/float twins. Because the
/// count is unchanged (a reversal neither grows nor shrinks), the call site
/// accepts ANY scalar list local — a param included — with NO capacity guard.
/// The `i32.ge_s` loop guard is SIGNED so the EMPTY list (`n == 0` → `j == -1`)
/// and the single-element list (`i == j == 0`) both loop zero times, never
/// touching memory: `[].reverse()` / `[x].reverse()` are no-ops, as in CPython.
const LIST_REVERSE_INPLACE_HELPER: &str = "\
  ;; __wasm_list_reverse(base) — Python xs.reverse(): reverse the list IN PLACE.
  ;; base → length-prefixed region: i32 count @ base+0, 8-byte elements @ base+8.
  ;; Two-pointer swap of 8-byte words: for i in 0..n/2 swap base[i] <-> base[n-1-i].
  ;; A word swap MOVES bytes verbatim (never interpreting them), so this ONE helper
  ;; serves BOTH list[int] and list[float]. Void — mutates in place (base never
  ;; moves, so every alias observes it). Empty / single-element lists loop zero
  ;; times (SIGNED i>=j guard: j == -1 when n == 0).
  (func $__wasm_list_reverse (param $base i32)
    (local $n i32)
    (local $ea i32)   ;; base + 8 (element 0)
    (local $i i32)    ;; low index
    (local $j i32)    ;; high index
    (local $lo i32)   ;; addr of low element
    (local $hi i32)   ;; addr of high element
    (local $tmp i64)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; ea = base + 8 (element 0)
    local.get $base
    i32.const 8
    i32.add
    local.set $ea
    ;; i = 0 ; j = n - 1
    i32.const 0
    local.set $i
    local.get $n
    i32.const 1
    i32.sub
    local.set $j
    (block $done
      (loop $sw
        ;; while i < j  (SIGNED: j == -1 when n == 0, so 0 >= -1 exits immediately)
        local.get $i
        local.get $j
        i32.ge_s
        br_if $done
        ;; lo = ea + i*8 ; hi = ea + j*8
        local.get $ea
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        local.set $lo
        local.get $ea
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        local.set $hi
        ;; tmp = *lo ; *lo = *hi ; *hi = tmp  (verbatim 8-byte word swap)
        local.get $lo
        i64.load
        local.set $tmp
        local.get $lo
        local.get $hi
        i64.load
        i64.store
        local.get $hi
        local.get $tmp
        i64.store
        ;; i++ ; j--
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        local.get $j
        i32.const 1
        i32.sub
        local.set $j
        br $sw
      )
    )
  )
";

/// PMAT-1288: the IN-PLACE list-SORT helper (int) — Python `xs.sort()` /
/// `xs.sort(reverse=True)` over a `list[int]` (`Stmt::ListMutate` with
/// `ListMutateOp::Sort`/`SortDesc`). This is [`LIST_SORTED_INT_HELPER`] MINUS
/// the alloc+copy phase: the SAME stable insertion sort (inner shift fires only
/// on a STRICT `i64.gt_s`/`i64.lt_s` compare, so equal elements never cross —
/// matching CPython's stable `list.sort`, and `sort(reverse=True)` is a stable
/// DESCENDING sort, not ascending-then-reversed), run directly over the
/// receiver's payload at `base+8` instead of a fresh record. No `$__alloc`
/// call, so it does NOT force `needs_heap`. Void — mutates in place; the
/// base-pointer never moves, so every alias observes the new order (the
/// alias-safe posture `reverse`/`del`/`remove` share), and because the count is
/// unchanged there is NO growable-list precondition: ANY scalar list local — a
/// PARAM included — qualifies. The outer `i32.ge_u` guard exits immediately for
/// the empty (`1 >= 0`) and single-element (`1 >= 1`) list: both are no-ops, as
/// in CPython. The `$reverse` param selects the direction at the call site
/// (`i32.const 1` descending, `0` ascending), exactly like the `sorted` pair.
const LIST_SORT_INPLACE_INT_HELPER: &str = "\
  ;; __wasm_list_sort_i64(base, reverse) — Python xs.sort([reverse=True]): sort IN PLACE.
  ;; base → length-prefixed region: i32 count @ base+0, i64 elements @ base+8.
  ;; Stable insertion sort over the receiver's own payload (no alloc, no copy).
  ;; Void — mutates in place (base never moves, so every alias observes it).
  (func $__wasm_list_sort_i64 (param $base i32) (param $reverse i32)
    (local $n i32)
    (local $ea i32)   ;; base + 8 (element 0)
    (local $i i32)
    (local $j i32)
    (local $key i64)
    (local $prev i64)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; ea = base + 8 (element 0)
    local.get $base
    i32.const 8
    i32.add
    local.set $ea
    ;; insertion sort in place: for i in 1..n
    i32.const 1
    local.set $i
    (block $srtd
      (loop $srt
        local.get $i
        local.get $n
        i32.ge_u
        br_if $srtd
        ;; key = elems[i]
        local.get $ea
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        i64.load
        local.set $key
        ;; j = i (the hole; shift out-of-order predecessors up into it)
        local.get $i
        local.set $j
        (block $ind
          (loop $inn
            ;; if j == 0, the hole reached the front — stop
            local.get $j
            i32.eqz
            br_if $ind
            ;; prev = elems[j-1]
            local.get $ea
            local.get $j
            i32.const 1
            i32.sub
            i32.const 8
            i32.mul
            i32.add
            i64.load
            local.set $prev
            ;; should_shift = reverse ? (prev < key) : (prev > key) — STRICT (stable)
            local.get $reverse
            if (result i32)
              local.get $prev
              local.get $key
              i64.lt_s
            else
              local.get $prev
              local.get $key
              i64.gt_s
            end
            ;; stop when NOT shifting (eqz of should_shift → break)
            i32.eqz
            br_if $ind
            ;; elems[j] = prev (shift the predecessor up into the hole)
            local.get $ea
            local.get $j
            i32.const 8
            i32.mul
            i32.add
            local.get $prev
            i64.store
            ;; j -= 1
            local.get $j
            i32.const 1
            i32.sub
            local.set $j
            br $inn
          )
        )
        ;; elems[j] = key (drop the key into the final hole)
        local.get $ea
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        local.get $key
        i64.store
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $srt
      )
    )
  )
";

/// PMAT-1288: the IN-PLACE list-SORT twin (float) — `xs.sort()` /
/// `xs.sort(reverse=True)` over a `list[float]`. Structurally identical to
/// [`LIST_SORT_INPLACE_INT_HELPER`] but with an f64 key/predecessor and
/// IEEE-754 `f64.gt`/`f64.lt` compares (the SAME compare opcodes as
/// [`LIST_SORTED_FLOAT_HELPER`], so `xs.sort()` and `xs = sorted(xs)` order a
/// float payload identically — NaN never fires the strict compare, matching
/// Python's undefined NaN-sort posture). A float list shares the int list's
/// header + 8-byte stride; only the element `*.load`/`*.store` and the compare
/// opcode differ. Both twins ride the single `needs_list_sort_inplace` gate
/// (`Stmt::ListMutate` carries `of_float`, but the emit site resolves the kind
/// from [`Scope::list_elem_of`] — one gate emitting both twins can never
/// mismatch it; the unused twin is harmless dead WAT, like `contains`/`insert`).
const LIST_SORT_INPLACE_FLOAT_HELPER: &str = "\
  ;; __wasm_list_sort_f64(base, reverse) — Python xs.sort([reverse=True]) over list[float].
  ;; base → length-prefixed region: i32 count @ base+0, f64 elements @ base+8.
  ;; Stable insertion sort over the receiver's own payload (no alloc, no copy).
  ;; Void — mutates in place (base never moves, so every alias observes it).
  (func $__wasm_list_sort_f64 (param $base i32) (param $reverse i32)
    (local $n i32)
    (local $ea i32)   ;; base + 8 (element 0)
    (local $i i32)
    (local $j i32)
    (local $key f64)
    (local $prev f64)
    ;; n = element count (i32 header @ base+0)
    local.get $base
    i32.load
    local.set $n
    ;; ea = base + 8 (element 0)
    local.get $base
    i32.const 8
    i32.add
    local.set $ea
    ;; insertion sort in place: for i in 1..n
    i32.const 1
    local.set $i
    (block $srtd
      (loop $srt
        local.get $i
        local.get $n
        i32.ge_u
        br_if $srtd
        ;; key = elems[i]
        local.get $ea
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        f64.load
        local.set $key
        ;; j = i (the hole; shift out-of-order predecessors up into it)
        local.get $i
        local.set $j
        (block $ind
          (loop $inn
            ;; if j == 0, the hole reached the front — stop
            local.get $j
            i32.eqz
            br_if $ind
            ;; prev = elems[j-1]
            local.get $ea
            local.get $j
            i32.const 1
            i32.sub
            i32.const 8
            i32.mul
            i32.add
            f64.load
            local.set $prev
            ;; should_shift = reverse ? (prev < key) : (prev > key) — STRICT (stable)
            local.get $reverse
            if (result i32)
              local.get $prev
              local.get $key
              f64.lt
            else
              local.get $prev
              local.get $key
              f64.gt
            end
            ;; stop when NOT shifting (eqz of should_shift → break)
            i32.eqz
            br_if $ind
            ;; elems[j] = prev (shift the predecessor up into the hole)
            local.get $ea
            local.get $j
            i32.const 8
            i32.mul
            i32.add
            local.get $prev
            f64.store
            ;; j -= 1
            local.get $j
            i32.const 1
            i32.sub
            local.set $j
            br $inn
          )
        )
        ;; elems[j] = key (drop the key into the final hole)
        local.get $ea
        local.get $j
        i32.const 8
        i32.mul
        i32.add
        local.get $key
        f64.store
        ;; i += 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $srt
      )
    )
  )
";

/// PMAT-1255: the list-CONCAT helper (`a + b` over two `list[scalar]`) — the
/// THIRD list-VALUED op that ALLOCATES (after `sorted` and `reversed`). Given
/// two length-prefixed base-pointers it bump-allocates a fresh record holding
/// `na + nb` elements, copies `a`'s payload then `b`'s, and returns the new
/// base. Concatenation MOVES 8-byte words verbatim (never interpreting them),
/// so — exactly like [`LIST_REVERSED_HELPER`] — this ONE helper serves BOTH
/// `list[int]` and `list[float]` (no int/float twin, unlike the two typed sort
/// helpers). The EMPTY list is the identity: `[] + b == b`, `a + [] == a` fall
/// out of `na == 0` / `nb == 0` copying zero words (no trap). Neither operand is
/// mutated — Python's `a + b` yields a fresh list.
const LIST_CONCAT_HELPER: &str = "\
  ;; __wasm_list_concat_i64(a, b) -> a NEW list = a ++ b (8-byte stride)
  ;; a, b → length-prefixed regions: i32 count @ base+0, 8-byte elements @ base+8.
  ;; Concatenation MOVES 8-byte words verbatim (never interpreting them), so this
  ;; ONE helper serves BOTH list[int] and list[float]: r = a[0..na] ++ b[0..nb].
  (func $__wasm_list_concat_i64 (param $a i32) (param $b i32) (result i32)
    (local $na i32)
    (local $nb i32)
    (local $r i32)
    (local $ra i32)
    (local $i i32)
    ;; na = count(a), nb = count(b) (i32 headers @ base+0)
    local.get $a
    i32.load
    local.set $na
    local.get $b
    i32.load
    local.set $nb
    ;; r = __alloc(8 + (na+nb)*8); write the i32 count header (na+nb) at r+0
    local.get $na
    local.get $nb
    i32.add
    i32.const 8
    i32.mul
    i32.const 8
    i32.add
    call $__alloc
    local.set $r
    local.get $r
    local.get $na
    local.get $nb
    i32.add
    i32.store
    ;; ra = r + 8 (element 0 of the new record)
    local.get $r
    i32.const 8
    i32.add
    local.set $ra
    ;; copy a: for i in 0..na: r[i] = a[i]  (a's element 0 is at a+8)
    i32.const 0
    local.set $i
    (block $acd
      (loop $ac
        local.get $i
        local.get $na
        i32.ge_u
        br_if $acd
        ;; dst = ra + i*8
        local.get $ra
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        ;; val = i64.load(a + 8 + i*8)
        local.get $a
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $ac
      )
    )
    ;; copy b: for i in 0..nb: r[na+i] = b[i]  (b's element 0 is at b+8)
    i32.const 0
    local.set $i
    (block $bcd
      (loop $bc
        local.get $i
        local.get $nb
        i32.ge_u
        br_if $bcd
        ;; dst = ra + (na+i)*8
        local.get $ra
        local.get $na
        local.get $i
        i32.add
        i32.const 8
        i32.mul
        i32.add
        ;; val = i64.load(b + 8 + i*8)
        local.get $b
        i32.const 8
        i32.add
        local.get $i
        i32.const 8
        i32.mul
        i32.add
        i64.load
        i64.store
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $bc
      )
    )
    local.get $r)
";

/// PMAT-1256: the list-SLICE helper (`xs[lo:hi]` over a `list[scalar]`) — the
/// FOURTH list-VALUED op that ALLOCATES (after `sorted`, `reversed`, and
/// `concat`). Given a length-prefixed base-pointer and two i64 ELEMENT bounds it
/// bump-allocates a fresh record holding the sub-list `base[lo:hi]` and returns
/// the new base. `lo`/`hi` carry FULL Python slice semantics: a negative bound is
/// normalised (`+= n`), BOTH bounds CLAMP to `[0, n]` (an out-of-range slice bound
/// never traps — unlike `xs[i]` indexing), and `hi` is raised to `lo` when it
/// would fall below it (an empty slice, never a negative length). The lowering
/// passes a missing `lo` as `0` and a missing `hi` as `i64::MAX` (clamped to `n`),
/// so `xs[:]` / `xs[a:]` / `xs[:b]` all fall out. Slicing MOVES the selected
/// 8-byte words verbatim (never interpreting them), so — exactly like
/// [`LIST_REVERSED_HELPER`] / [`LIST_CONCAT_HELPER`] — this ONE helper serves BOTH
/// `list[int]` and `list[float]` (no int/float twin, unlike the two typed sort
/// helpers): an f64's bit pattern copied as an i64-word range is lossless and a
/// `memory.copy` cannot canonicalise a NaN payload. The source is never mutated
/// (Python's `xs[lo:hi]` yields a fresh list); the empty slice allocates an empty
/// record and returns it (no trap).
const LIST_SLICE_HELPER: &str = "\
  ;; __wasm_list_slice_i64(base, lo, hi) -> a NEW list = base[lo:hi] (8-byte stride)
  ;; base → length-prefixed region: i32 count @ base+0, 8-byte elements @ base+8.
  ;; lo/hi are i64 ELEMENT indices with full Python slice semantics (negative
  ;; normalised += n, both clamp to [0, n], hi raised to lo → never a negative
  ;; length, out-of-range never traps). Slicing MOVES 8-byte words verbatim (never
  ;; interpreting them), so this ONE helper serves BOTH list[int] and list[float].
  (func $__wasm_list_slice_i64 (param $base i32) (param $lo i64) (param $hi i64) (result i32)
    (local $n i32)
    (local $nl i64)
    (local $cnt i32)
    (local $dst i32)
    ;; n = element count (i32 header @ base+0); nl = i64 widening for the bounds.
    local.get $base
    i32.load
    local.set $n
    local.get $n
    i64.extend_i32_u
    local.set $nl
    ;; --- normalise lo: if lo<0 lo+=nl; then clamp to [0, nl] ---
    local.get $lo
    i64.const 0
    i64.lt_s
    if
      local.get $lo
      local.get $nl
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
    local.get $nl
    i64.gt_s
    if
      local.get $nl
      local.set $lo
    end
    ;; --- normalise hi: if hi<0 hi+=nl; then clamp to [0, nl] ---
    local.get $hi
    i64.const 0
    i64.lt_s
    if
      local.get $hi
      local.get $nl
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
    local.get $nl
    i64.gt_s
    if
      local.get $nl
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
    ;; cnt = (hi - lo) as i32 — the element count of the sub-list.
    local.get $hi
    local.get $lo
    i64.sub
    i32.wrap_i64
    local.set $cnt
    ;; dst = __alloc(8 + cnt*8); write the i32 count header at dst+0.
    local.get $cnt
    i32.const 8
    i32.mul
    i32.const 8
    i32.add
    call $__alloc
    local.set $dst
    local.get $dst
    local.get $cnt
    i32.store
    ;; memory.copy(dst+8, base + 8 + lo*8, cnt*8) — a verbatim word-range move.
    local.get $dst
    i32.const 8
    i32.add
    local.get $base
    i32.const 8
    i32.add
    local.get $lo
    i32.wrap_i64
    i32.const 8
    i32.mul
    i32.add
    local.get $cnt
    i32.const 8
    i32.mul
    memory.copy
    local.get $dst)
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
                // PMAT-956: an allocating module additionally cites the Layer-5
                // heap contract C-WASM-HEAP (structural channel), so heap-using
                // WAT is not uncited at Layer 5.
                let mut citations = vec![ContractId::new(CONTRACT_ID)];
                if module_needs_heap(module) {
                    citations.push(ContractId::new(HEAP_CONTRACT_ID));
                }
                Ok(Artifact {
                    primary: wat,
                    sidecars: Vec::new(),
                    citations,
                    quorum_status: QuorumStatus::Single {
                        emitter: "xpile-wasm-codegen".to_string(),
                    },
                }
                .with_citations(config.emit_contracts))
            }
            WasmBackendInner::DiffExecWitness(inner) => inner
                .lower(module, config)
                .map(|a| a.with_citations(config.emit_contracts)),
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
/// (list literals bound to a name notwithstanding) refuse with precise
/// messages.
///
/// PMAT-1260: `for i, x in enumerate(xs[, start])` — a [`Stmt::ForEachPair`]
/// with [`PairIterKind::Enumerate`] over a NAMED `list[scalar]` — is desugared
/// here too, into the same Let+While+`Index` subset: the enumerate index IS the
/// loop counter (offset by `start`), the element is the `Index` read. The
/// element type is resolved from a per-function name→type env (params + every
/// `let`), since — unlike [`Stmt::ForEach`] — `ForEachPair` carries no
/// `elem_ty` field. `zip`, `Pairs` (`d.items()`), and a non-name enumerate
/// source still refuse.
fn desugar_module_foreach(module: &Module) -> Result<Module, BackendError> {
    let mut m = module.clone();
    for item in &mut m.items {
        match item {
            Item::Function(f) => {
                let env = fn_name_type_env(f);
                let mut next = 0usize;
                f.body.stmts = desugar_foreach_stmts(&f.body.stmts, &mut next, &env)?;
            }
            Item::Struct { methods, .. } => {
                for f in methods {
                    let env = fn_name_type_env(f);
                    let mut next = 0usize;
                    f.body.stmts = desugar_foreach_stmts(&f.body.stmts, &mut next, &env)?;
                }
            }
            _ => {}
        }
    }
    Ok(m)
}

/// PMAT-1260: a function's `name → declared Type` env (params + every `let` in
/// the body, a shadowing `let` overriding the param). Drives the enumerate
/// element-type resolution in [`desugar_foreach_stmts`] — `ForEachPair` has no
/// `elem_ty` field, so the list element type is recovered from the iterable's
/// declared list type here. Reuses [`collect_let_types`] (the same walker the
/// f-string int normaliser uses); it does not descend into loop bodies, so an
/// enumerate over a name first bound *inside* another loop is not resolved and
/// refuses honestly downstream — the common param / top-level-`let` source
/// resolves.
fn fn_name_type_env(f: &Function) -> HashMap<String, Type> {
    let mut env: HashMap<String, Type> = HashMap::new();
    for p in &f.params {
        env.insert(p.name.clone(), p.ty.clone());
    }
    collect_let_types(&f.body.stmts, &mut env);
    env
}

/// The recursive statement rewrite behind [`desugar_module_foreach`].
/// `next` numbers the synthetic locals within one function; `env` resolves the
/// enumerate element type (PMAT-1260).
fn desugar_foreach_stmts(
    stmts: &[Stmt],
    next: &mut usize,
    env: &HashMap<String, Type>,
) -> Result<Vec<Stmt>, BackendError> {
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
                let body = desugar_foreach_stmts(body, next, env)?;
                let k = *next;
                *next += 1;
                let idx = format!("{FOREACH_IDX_PREFIX}{k}");
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
            // PMAT-1260: `for i, x in enumerate(xs[, start])` over a NAMED
            // `list[scalar]`. The enumerate index IS the loop counter (offset
            // by `start`); the element is the same `Index` read the single-var
            // ForEach uses. Desugared into Let+While so every downstream scan +
            // emit pass sees only statements it already lowers. `zip`, `Pairs`
            // (`d.items()`), and a non-name source refuse honestly.
            Stmt::ForEachPair {
                first,
                second,
                iter,
                kind,
                body,
            } => {
                match kind {
                    PairIterKind::Enumerate { start } => {
                        // `enumerate` binds the SECOND target to the element, so the
                        // element type is the iterable's list element type. Resolve it
                        // from the per-function env (ForEachPair carries no elem_ty).
                        let Expr::Ident(src) = iter else {
                            return Err(unsupported(&format!(
                                "for-loop over enumerate({}) — the WASM subset iterates \
                         a named `list[scalar]`; bind the iterable to a name \
                         first",
                                expr_kind(iter)
                            )));
                        };
                        let elem_ty = match env.get(src) {
                            Some(Type::List(elem)) => (**elem).clone(),
                            _ => {
                                return Err(unsupported(&format!(
                                    "for-loop over enumerate(`{src}`) — `{src}` is not a \
                             declared `list[scalar]` in scope; the WASM \
                             enumerate subset needs its element type"
                                )));
                            }
                        };
                        let body = desugar_foreach_stmts(body, next, env)?;
                        let k = *next;
                        *next += 1;
                        let idx = format!("{FOREACH_IDX_PREFIX}{k}");
                        // The enumerate index: the raw counter, offset by `start` when
                        // non-zero (`checked` semantics are unnecessary — the WASM lane
                        // models Python `int` as i64 throughout).
                        let index_val = if *start == 0 {
                            Expr::Ident(idx.clone())
                        } else {
                            Expr::BinOp {
                                op: BinOp::Add,
                                lhs: Box::new(Expr::Ident(idx.clone())),
                                rhs: Box::new(Expr::LitInt(*start)),
                            }
                        };
                        let mut wbody = Vec::with_capacity(body.len() + 3);
                        // Both bindings read the counter BEFORE the increment, so the
                        // index is the current position (CPython-exact) and `continue`
                        // (→ `br` back to the `while` cond) still sees the advance.
                        wbody.push(Stmt::Let {
                            name: first.clone(),
                            ty: Type::I64,
                            value: index_val,
                            mutable: false,
                        });
                        wbody.push(Stmt::Let {
                            name: second.clone(),
                            ty: elem_ty,
                            value: Expr::Index {
                                collection: Box::new(Expr::Ident(src.clone())),
                                index: Box::new(Expr::Ident(idx.clone())),
                            },
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
                                rhs: Box::new(Expr::Len(Box::new(Expr::Ident(src.clone())))),
                            },
                            body: wbody,
                        });
                    }
                    // PMAT-1261: `for a, b in zip(xs, ys)` over TWO named
                    // `list[scalar]` sources. The paired loop desugars into a
                    // single Let+While driven by a shared counter, with
                    // SHORTEST-ITERABLE termination — the `while` condition is
                    // `idx < len(xs) and idx < len(ys)`, so iteration stops at the
                    // shorter operand exactly as CPython's `zip` does. Each
                    // binding is the same `Index` read the single-var ForEach uses,
                    // typed from the per-function env (ForEachPair carries no
                    // elem_ty). Both operands must be NAMED lists; a non-name
                    // source (list literal / nested expr) refuses honestly, and a
                    // 3+-way `zip` never reaches here (the frontend nests it).
                    PairIterKind::Zip(other) => {
                        let Expr::Ident(src_a) = iter else {
                            return Err(unsupported(&format!(
                                "for-loop over zip({}, ...) — the WASM subset \
                             zip-iterates two NAMED `list[scalar]` sources; \
                             bind the first iterable to a name first",
                                expr_kind(iter)
                            )));
                        };
                        let Expr::Ident(src_b) = other.as_ref() else {
                            return Err(unsupported(&format!(
                                "for-loop over zip({src_a}, {}) — the WASM subset \
                             zip-iterates two NAMED `list[scalar]` sources; \
                             bind the second iterable to a name first",
                                expr_kind(other)
                            )));
                        };
                        let elem_a = match env.get(src_a) {
                            Some(Type::List(elem)) => (**elem).clone(),
                            _ => {
                                return Err(unsupported(&format!(
                                    "for-loop over zip(`{src_a}`, ...) — `{src_a}` \
                                 is not a declared `list[scalar]` in scope; the \
                                 WASM zip subset needs its element type"
                                )));
                            }
                        };
                        let elem_b = match env.get(src_b) {
                            Some(Type::List(elem)) => (**elem).clone(),
                            _ => {
                                return Err(unsupported(&format!(
                                    "for-loop over zip(..., `{src_b}`) — `{src_b}` \
                                 is not a declared `list[scalar]` in scope; the \
                                 WASM zip subset needs its element type"
                                )));
                            }
                        };
                        let body = desugar_foreach_stmts(body, next, env)?;
                        let k = *next;
                        *next += 1;
                        let idx = format!("{FOREACH_IDX_PREFIX}{k}");
                        let mut wbody = Vec::with_capacity(body.len() + 3);
                        // Both bindings read the CURRENT counter, then it advances —
                        // so `continue` (→ `br` to the `while` cond) still sees the
                        // increment and the shortest-iterable guard re-checks.
                        wbody.push(Stmt::Let {
                            name: first.clone(),
                            ty: elem_a,
                            value: Expr::Index {
                                collection: Box::new(Expr::Ident(src_a.clone())),
                                index: Box::new(Expr::Ident(idx.clone())),
                            },
                            mutable: false,
                        });
                        wbody.push(Stmt::Let {
                            name: second.clone(),
                            ty: elem_b,
                            value: Expr::Index {
                                collection: Box::new(Expr::Ident(src_b.clone())),
                                index: Box::new(Expr::Ident(idx.clone())),
                            },
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
                        out.push(Stmt::Let {
                            name: idx.clone(),
                            ty: Type::I64,
                            value: Expr::LitInt(0),
                            mutable: true,
                        });
                        // Shortest-iterable: `idx < len(xs) and idx < len(ys)`.
                        out.push(Stmt::While {
                            cond: Expr::BinOp {
                                op: BinOp::And,
                                lhs: Box::new(Expr::BinOp {
                                    op: BinOp::Lt,
                                    lhs: Box::new(Expr::Ident(idx.clone())),
                                    rhs: Box::new(Expr::Len(Box::new(Expr::Ident(src_a.clone())))),
                                }),
                                rhs: Box::new(Expr::BinOp {
                                    op: BinOp::Lt,
                                    lhs: Box::new(Expr::Ident(idx.clone())),
                                    rhs: Box::new(Expr::Len(Box::new(Expr::Ident(src_b.clone())))),
                                }),
                            },
                            body: wbody,
                        });
                    }
                    PairIterKind::Pairs => {
                        return Err(unsupported(
                            "for-loop over a list of 2-tuples (e.g. `d.items()`) — \
                         the WASM subset paired-iterates only \
                         `enumerate(<named list>)` and `zip(<named list>, \
                         <named list>)`; tuple-element destructuring is not yet \
                         in the lane",
                        ));
                    }
                }
            }
            Stmt::While { cond, body } => out.push(Stmt::While {
                cond: cond.clone(),
                body: desugar_foreach_stmts(body, next, env)?,
            }),
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => out.push(Stmt::If {
                cond: cond.clone(),
                then_body: desugar_foreach_stmts(then_body, next, env)?,
                else_body: desugar_foreach_stmts(else_body, next, env)?,
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
    // PMAT-1187: `s.capitalize()` (`Expr::StrMethod`, op `Capitalize`) — an
    // allocating string-RETURNING op (a fresh heap string, first ASCII letter
    // upper-cased, the rest lower-cased). Its own `$__wasm_str_capitalize` helper
    // (i == 0 upper-flips, i > 0 lower-flips; a non-ASCII byte traps). Rides
    // `needs_heap` (set via `expr_has_heap_op`, like upper/lower/replace/zfill).
    // Byte-parallel (no charlen math — all survivors are 1-byte ASCII).
    let needs_capitalize = module_uses_str_method(module, StrMethodOp::Capitalize);
    // PMAT-1201: `s.swapcase()` (`Expr::StrMethod`, op `SwapCase`) — an allocating
    // string-RETURNING op (a fresh heap string with the case of every ASCII letter
    // flipped BOTH ways). Its own `$__wasm_str_swapcase` helper (the both-directions
    // twin of `$__wasm_str_upper_lower`; a non-ASCII byte traps). Rides `needs_heap`
    // (set via `expr_has_heap_op`, like upper/lower/capitalize/replace/zfill).
    // Byte-parallel (no charlen math — all survivors are 1-byte ASCII).
    let needs_swapcase = module_uses_str_method(module, StrMethodOp::SwapCase);
    // PMAT-1203: `s.title()` (`Expr::StrMethod`, op `Title`) — an allocating
    // string-RETURNING op (a fresh heap string title-cased word-by-word). Its own
    // `$__wasm_str_title` helper carries a `$prev` flag (the first ASCII letter of
    // each word upper-cased, the rest lower-cased, a non-letter a word boundary; a
    // non-ASCII byte traps). Rides `needs_heap` (set via `expr_has_heap_op`, like
    // upper/lower/capitalize/swapcase/replace/zfill). Byte-parallel storage (no
    // charlen math — all survivors are 1-byte ASCII) but STATEFUL across the scan.
    let needs_title = module_uses_str_method(module, StrMethodOp::Title);
    // PMAT-1205: `s.strip()` / `s.lstrip()` / `s.rstrip()` (`Expr::StrMethod`, ops
    // `Strip` / `LStrip` / `RStrip`) — allocating string-RETURNING ops (a fresh
    // heap string with the leading/trailing ASCII-whitespace run removed, the
    // retained byte range copied verbatim). All three share the single
    // `$__wasm_str_strip` helper (`left` / `right` i32 flags select which ends to
    // trim), so any one present must emit it. Rides `needs_heap` (set via
    // `expr_has_heap_op`, like upper/lower/capitalize/swapcase/title/replace/zfill).
    // Byte-parallel storage (no charlen math — the retained bytes are copied
    // verbatim; a non-ASCII BOUNDARY byte traps rather than being (mis)judged).
    let needs_strip = module_uses_str_method(module, StrMethodOp::Strip)
        || module_uses_str_method(module, StrMethodOp::LStrip)
        || module_uses_str_method(module, StrMethodOp::RStrip);
    // PMAT-1209: `s.rjust(w)` / `s.ljust(w)` / `s.center(w)` (`Expr::StrMethod`,
    // ops `RJust` / `LJust` / `Center`) — allocating string-RETURNING ops (a fresh
    // heap string padded with ASCII space to `w` code points). All three share the
    // single `$__wasm_str_pad` helper (a `mode` i32 flag picks rjust=0 / ljust=1 /
    // center=2), so any one present must emit it. Rides `needs_heap` (set via
    // `expr_has_heap_op`, like zfill/upper/lower/strip). Like zfill its width math
    // calls `$__wasm_str_charlen` (co-emitted for any str-touching module via
    // `module_touches_str`, which a `StrMethod` always sets), so it forces no extra
    // helper beyond the always-present char family — and unlike the case-fold ops it
    // never inspects a payload byte (pad = ASCII space, `s` copied verbatim), so it
    // is char-exact for any UTF-8 with NO trap arm.
    let needs_pad = module_uses_str_method(module, StrMethodOp::RJust)
        || module_uses_str_method(module, StrMethodOp::LJust)
        || module_uses_str_method(module, StrMethodOp::Center);
    // PMAT-1213: `s[::-1]` (`Expr::StrMethod`, op `Reverse`) — an allocating
    // string-RETURNING op (a fresh heap string with the CODE POINTS of `s` in reverse
    // order). Its own `$__wasm_str_reverse` helper copies each UTF-8 code point as an
    // intact unit to a descending output position. Rides `needs_heap` (set via
    // `expr_has_heap_op`, like upper/lower/capitalize/swapcase/title/strip/pad). Unlike
    // the case-fold ops it needs NO Unicode table (the UTF-8 lead byte gives each code
    // point's length), so it is char-exact for any valid UTF-8 with NO trap arm —
    // matching CPython `s[::-1]` and the rust/ruchy `.chars().rev()` lane.
    let needs_reverse = module_uses_str_method(module, StrMethodOp::Reverse);
    // PMAT-1219: `s.expandtabs()` / `s.expandtabs(tabsize)` (`Expr::StrMethod`, op
    // `ExpandTabs`) — an allocating string-RETURNING op (a fresh heap string with
    // each `\t` replaced by the ASCII spaces needed to reach the next multiple of
    // `tabsize`, the COLUMN counted in CODE POINTS and reset on `\n`/`\r`). Its own
    // `$__wasm_str_expandtabs` helper does a two-pass walk (pass 1 sizes the output,
    // pass 2 fills it) — it copies each non-tab code point verbatim and only
    // interprets the ASCII tab/newline bytes, so like reverse it needs NO Unicode
    // table and is char-exact for any valid UTF-8 with NO trap arm. Rides
    // `needs_heap` (set via `expr_has_heap_op`, like reverse/pad/strip).
    let needs_expandtabs = module_uses_str_method(module, StrMethodOp::ExpandTabs);
    // PMAT-1189: `s.isdigit()` (`Expr::StrMethod`, op `IsDigit`) — a bool (i32)
    // predicate: `1` iff `s` is non-empty and every code point is an ASCII digit.
    // NON-allocating (a single byte scan, no heap), so — unlike the case-fold
    // ops — it does NOT ride `needs_heap`; it carries its own `$__wasm_str_isdigit`
    // helper and only needs the `(memory …)` its payload load reads (pulled in
    // below, alongside `needs_startswith`/`needs_endswith`, whichever way the
    // receiver reaches memory).
    //
    // PMAT-1211: `s.isnumeric()` (`IsNumeric`) SHARES this exact helper. On the
    // ASCII-decidable domain the numeric characters are exactly `'0'`–`'9'`
    // (`0x30`–`0x39`, all Unicode category Nd), so `isnumeric` and `isdigit`
    // compute the identical function over an all-ASCII string; and on the
    // UNDECIDABLE non-ASCII domain (where `isnumeric`'s Nd/Nl/No superset — `"½"`,
    // Roman numerals — would decide differently) BOTH must trap (this scalar lane
    // carries no Unicode table). The isdigit scan already traps on a non-ASCII byte
    // reached with an all-digit prefix and short-circuits `0` on a leading ASCII
    // non-digit (`"a½".isnumeric()` → `0`, matching Python), so it is byte-exact
    // for `isnumeric` on precisely the inputs it is byte-exact for `isdigit`. So
    // either op present emits the one `$__wasm_str_isdigit` helper (like `isupper`
    // /`islower` share `$__wasm_str_isupper_islower`) — no duplicate scan.
    let needs_isdigit = module_uses_str_method(module, StrMethodOp::IsDigit)
        || module_uses_str_method(module, StrMethodOp::IsNumeric);
    // PMAT-1191: `s.isalpha()` (`Expr::StrMethod`, op `IsAlpha`) — the predicate
    // twin of `isdigit`: a bool (i32) `1` iff `s` is non-empty and every code
    // point is an ASCII letter. Same non-allocating byte scan — it does NOT ride
    // `needs_heap`; it carries its own `$__wasm_str_isalpha` helper and only needs
    // the `(memory …)` its payload load reads (pulled in below alongside
    // `needs_isdigit`).
    let needs_isalpha = module_uses_str_method(module, StrMethodOp::IsAlpha);
    // PMAT-1193: `s.isspace()` (`Expr::StrMethod`, op `IsSpace`) — the third
    // predicate in the `is*` family: a bool (i32) `1` iff `s` is non-empty and
    // every code point is ASCII whitespace (`0x09`–`0x0D` or `0x1C`–`0x20`). Same
    // non-allocating byte scan as isdigit/isalpha — it does NOT ride `needs_heap`;
    // it carries its own `$__wasm_str_isspace` helper and only needs the
    // `(memory …)` its payload load reads (pulled in below alongside
    // `needs_isdigit`/`needs_isalpha`).
    let needs_isspace = module_uses_str_method(module, StrMethodOp::IsSpace);
    // PMAT-1195: `s.isalnum()` (`Expr::StrMethod`, op `IsAlnum`) — the fourth
    // predicate in the `is*` family: a bool (i32) `1` iff `s` is non-empty and
    // every code point is ASCII alphanumeric (`0x30`–`0x39` / `0x41`–`0x5A` /
    // `0x61`–`0x7A`, the UNION of the isdigit and isalpha ranges). Same
    // non-allocating byte scan as isdigit/isalpha/isspace — it does NOT ride
    // `needs_heap`; it carries its own `$__wasm_str_isalnum` helper and only
    // needs the `(memory …)` its payload load reads (pulled in below alongside
    // `needs_isdigit`/`needs_isalpha`/`needs_isspace`).
    let needs_isalnum = module_uses_str_method(module, StrMethodOp::IsAlnum);
    // PMAT-1197: `s.isupper()` / `s.islower()` (`Expr::StrMethod`, ops `IsUpper` /
    // `IsLower`) — the fifth/sixth predicates in the `is*` family, and the first
    // pair whose truth needs STATE across the scan (at least one cased char AND no
    // opposite-case char) rather than an every-char fold. Both share the single
    // non-allocating `$__wasm_str_isupper_islower` helper (a `want_upper` i32 flag
    // picks the wanted/disqualifier ranges), so either op present must emit it.
    // Like the four sibling predicates it does NOT ride `needs_heap` (a bool from
    // a byte scan, no allocator); it only needs the `(memory …)` its payload load
    // reads (folded into the condition below alongside `needs_isdigit`).
    let needs_isupper_lower = module_uses_str_method(module, StrMethodOp::IsUpper)
        || module_uses_str_method(module, StrMethodOp::IsLower);
    // PMAT-1199: `s.isascii()` (`Expr::StrMethod`, op `IsAscii`) — the SEVENTH
    // predicate in the `is*` family, and the ONLY one that is fully decidable at
    // the byte level (a byte `>= 0x80` is the DEFINITIVE False; it NEVER traps and
    // needs NO empty guard). Same non-allocating byte scan as the siblings — it
    // does NOT ride `needs_heap`; it carries its own `$__wasm_str_isascii` helper
    // and only needs the `(memory …)` its payload load reads (folded into the
    // condition below alongside `needs_isdigit`).
    let needs_isascii = module_uses_str_method(module, StrMethodOp::IsAscii);
    // PMAT-1248: `sum(xs)` over a `list[int]` (`Expr::Sum { of_float: false }`)
    // reduces the list payload via `$__wasm_list_sum_i64` — a non-allocating
    // read (like the byte-scan str predicates), so it rides its OWN gate, NOT
    // `needs_heap`. It needs the `(memory …)` its list-payload loads read; a
    // summed list already forces `(memory)` via `module_uses_list_param`/
    // `needs_heap` (a param base-pointer or a `ListLit` local), but assert it
    // here too, mirroring the `needs_str_cmp`/`needs_isascii` belt-and-suspenders.
    let needs_list_sum = module_uses_list_sum(module);
    // PMAT-1249: the list[float] sum twin — its own gate/helper, same `(memory)`
    // need as the i64 form (it reads the f64 list payload).
    let needs_list_sum_float = module_uses_list_sum_float(module);
    // PMAT-1250: `min(xs)` / `max(xs)` over a `list[int]` / `list[float]`
    // (`Expr::ListMinMax`) folds the list payload via a `$__wasm_list_minmax_*`
    // helper — non-allocating (a payload read, like sum), so each rides its OWN
    // gate, NOT `needs_heap`, and needs the `(memory …)` its loads read.
    let needs_list_minmax = module_uses_list_minmax(module);
    let needs_list_minmax_float = module_uses_list_minmax_float(module);
    // PMAT-1251: `any(xs)`/`all(xs)` over a `list[bool]` (`Expr::BoolReduce`,
    // direct non-generator form) folds the list payload via
    // `$__wasm_list_bool_reduce` — non-allocating (a payload read, like sum/
    // minmax), so it rides its OWN gate, NOT `needs_heap`, and needs the
    // `(memory …)` its i32-element loads read.
    let needs_list_bool_reduce = module_uses_list_bool_reduce(module);
    // PMAT-1252: `sorted(xs)` over a `list[int]` / `list[float]` (`Expr::Sorted`)
    // returns a NEW sorted list via a `$__wasm_list_sorted_*` helper — the FIRST
    // list-VALUED op that ALLOCATES, so it ALSO forces `needs_heap` (via
    // `expr_has_heap_op`, which pulls in `$__alloc` + the bump-heap `(memory)`).
    // Each kind rides its OWN gate so a module that sorts only ints carries no
    // dead f64 helper and vice-versa.
    let needs_list_sorted = module_uses_list_sorted(module);
    let needs_list_sorted_float = module_uses_list_sorted_float(module);
    // PMAT-1291: `sorted(s)` over a `set[int]` materialises the set to a fresh
    // `list[int]` via `$__wasm_set_to_list_i64` (the source of the sort), which
    // ALLOCATES via `$__alloc` — so it also forces `needs_heap` (via
    // `expr_has_heap_op`). ONE helper (int sets only; a str set → `list[str]` is
    // unmodelled) rides its OWN gate.
    let needs_set_to_list = module_uses_set_to_list(module);
    // PMAT-1253: `reversed(xs)` / `list(reversed(xs))` / `xs[::-1]` over a
    // `list[int]` / `list[float]` (`Expr::Reversed`) returns a NEW reversed list
    // via `$__wasm_list_reversed_i64` — the SECOND list-VALUED op that ALLOCATES,
    // so (like `sorted`) it ALSO forces `needs_heap` (via `expr_has_heap_op`).
    // ONE helper serves both int and float (reversal is a verbatim 8-byte-word
    // move), so a SINGLE gate drives it (no int/float twin, unlike sorted).
    let needs_list_reversed = module_uses_list_reversed(module);
    // PMAT-1255: `a + b` over two `list[int]`/`list[float]` (`Expr::ListConcat`)
    // returns a NEW concatenated list via `$__wasm_list_concat_i64` — the THIRD
    // list-VALUED op that ALLOCATES, so (like `sorted`/`reversed`) it ALSO forces
    // `needs_heap` (via `expr_has_heap_op`). ONE helper serves both int and float
    // (concat moves 8-byte words verbatim), so a SINGLE gate drives it.
    let needs_list_concat = module_uses_list_concat(module);
    // PMAT-1256: `xs[lo:hi]` over a `list[int]`/`list[float]`
    // (`Expr::Slice { of_str: false }`) returns a NEW sub-list via
    // `$__wasm_list_slice_i64` — the FOURTH list-VALUED op that ALLOCATES, so
    // (like `sorted`/`reversed`/`concat`) it ALSO forces `needs_heap` (via
    // `expr_has_heap_op`). ONE helper serves both int and float (slicing moves
    // 8-byte words verbatim), so a SINGLE gate drives it.
    let needs_list_slice = module_uses_list_slice(module);
    // PMAT-1262: `x in xs` / `x not in xs` over a `list[int]`/`list[float]`
    // (`Expr::ListContains`) tests membership via a `$__wasm_list_contains_*`
    // linear scan — NON-allocating (a payload read, like sum/minmax), so it rides
    // its OWN gate, NOT `needs_heap`, and needs the `(memory …)` its loads read.
    // The node carries no element-kind discriminant, so BOTH typed helpers are
    // emitted under this single gate (the unused twin is a harmless dead fn).
    let needs_list_contains = module_uses_list_contains(module);
    // PMAT-1274: `xs.count(v)` / `xs.index(v)` over a `list[int]`/`list[float]`
    // (`Expr::ListQuery`) scan the payload via `$__wasm_list_{count,index}_*` —
    // NON-allocating (a read, like contains), so each rides its OWN gate, NOT
    // `needs_heap`, and needs the `(memory …)` its loads read. The node carries
    // the `op` (Count/Index) so count and index gate SEPARATELY; each gate emits
    // both element-kind twins (no element-kind discriminant on the node).
    let needs_list_count = module_uses_list_count(module);
    let needs_list_index = module_uses_list_index(module);
    // PMAT-1282: `xs.insert(i, v)` over a `list[int]`/`list[float]`
    // (`Stmt::ListInsert`) shifts the tail and writes via a `$__wasm_list_insert_*`
    // helper — it reads/writes the length-prefixed region, so (like the read
    // helpers) it rides its OWN gate and needs the `(memory …)`. The node carries
    // no element-kind discriminant, so BOTH typed helpers are emitted under this
    // single gate (the unused twin is a harmless dead fn, like `contains`).
    let needs_list_insert = module_uses_list_insert(module);
    // PMAT-1284: `del xs[i]` over a `list[int]`/`list[float]` (`Stmt::DelItem`,
    // `is_dict == false`) shrinks-and-shifts via the single `$__wasm_list_delitem`
    // helper — it reads/writes the length-prefixed region, so it rides its OWN gate
    // and needs the `(memory …)`. The shift is a pure 8-byte word move, so ONE
    // helper serves both element kinds (no dead twin, unlike `insert`).
    let needs_list_delitem = module_uses_list_delitem(module);
    // PMAT-1285: `xs.remove(v)` over a `list[int]`/`list[float]`
    // (`Stmt::ListRemoveValue`) scans for the first value-match then
    // shrinks-and-shifts via a `$__wasm_list_remove_{i64,f64}` helper — it
    // reads/writes the length-prefixed region, so it rides its OWN gate and needs
    // the `(memory …)`. Unlike `del` (a pure 8-byte word move → one helper), the
    // value compare is TYPED (`i64.eq`/`f64.eq`), so BOTH twins are emitted under
    // this single gate (the unused twin is a harmless dead fn, like `contains`).
    let needs_list_remove = module_uses_list_remove(module);
    // PMAT-1286: `xs.reverse()` over a `list[int]`/`list[float]` (`Stmt::ListMutate`
    // with `ListMutateOp::Reverse`) reverses the payload IN PLACE via the single
    // `$__wasm_list_reverse` helper — it reads/writes the length-prefixed region,
    // so it rides its OWN gate and needs the `(memory …)`. The swap is a pure
    // 8-byte word move, so ONE helper serves both element kinds (no dead twin).
    let needs_list_reverse = module_uses_list_reverse(module);
    // PMAT-1288: `xs.sort()` / `xs.sort(reverse=True)` over a
    // `list[int]`/`list[float]` (`Stmt::ListMutate` with `Sort`/`SortDesc`)
    // insertion-sorts the payload IN PLACE via `$__wasm_list_sort_{i64,f64}` —
    // it reads/writes the length-prefixed region, so it rides its OWN gate and
    // needs the `(memory …)`. Unlike `reverse` (a pure word move → one helper),
    // the compare is TYPED (`i64.gt_s`/`f64.gt`), so BOTH twins are emitted
    // under this single gate (the unused twin is harmless dead WAT, like
    // `contains`/`insert`; no `$__alloc` inside, so no `needs_heap` coupling).
    let needs_list_sort_inplace = module_uses_list_sort_inplace(module);
    // PMAT-1289: `xs.pop(i)` — the INDEXED pop (`Expr::ListPop` with an index) —
    // over a `list[int]`/`list[float]` loads-then-shifts via the typed
    // `$__wasm_list_pop_idx_{i64,f64}` helper pair — it reads/writes the
    // length-prefixed region, so it rides its OWN gate and needs the
    // `(memory …)`. The tail shift is a pure word move but the value load/return
    // is TYPED (like `remove`, unlike `del`'s single helper), so BOTH twins are
    // emitted under this single gate (the node carries no element-kind
    // discriminant; the unused twin is harmless dead WAT, like `contains`). The
    // no-index `xs.pop()` stays INLINE (no helper) and does NOT arm this gate.
    let needs_list_pop_idx = module_uses_list_pop_index(module);
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
        || needs_isdigit
        || needs_isalpha
        || needs_isspace
        || needs_isalnum
        || needs_isupper_lower
        || needs_isascii
        || needs_list_sum
        || needs_list_sum_float
        || needs_list_minmax
        || needs_list_minmax_float
        || needs_list_bool_reduce
        || needs_list_sorted
        || needs_list_sorted_float
        || needs_set_to_list
        || needs_list_reversed
        || needs_list_concat
        || needs_list_slice
        || needs_list_contains
        || needs_list_count
        || needs_list_index
        || needs_list_insert
        || needs_list_delitem
        || needs_list_remove
        || needs_list_reverse
        || needs_list_sort_inplace
        || needs_list_pop_idx
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
            // PMAT-956: the bump-heap allocator is governed by C-WASM-HEAP (the
            // Layer-5 extension of C-COMPILE-RUST-TO-WASM); cite it in-text
            // whenever the module allocates, so heap-using WAT is not uncited.
            writeln!(out, "  ;; xpile-contract: {HEAP_CONTRACT_ID}").expect("write");
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
    // PMAT-1187: emit the string CAPITALIZE helper once, when any function uses
    // `s.capitalize()` (`Expr::StrMethod`, op `Capitalize`). The
    // `$__wasm_str_capitalize` helper upper-flips the first ASCII letter and
    // lower-flips the rest. Allocating (calls `$__alloc` + `i32.store8`), so it
    // rides `needs_heap` — a capitalize sets the heap gate via `expr_has_heap_op`.
    // Byte-parallel (no char helper — a non-ASCII byte traps rather than folding).
    // Gated on an actual use so an unrelated heap-string module carries no dead
    // helper.
    if needs_heap && needs_capitalize {
        out.push_str(STR_CAPITALIZE_HELPER);
    }
    // PMAT-1201: emit the string SWAPCASE helper once, when any function uses
    // `s.swapcase()` (`Expr::StrMethod`, op `SwapCase`). The `$__wasm_str_swapcase`
    // helper flips the case of every ASCII letter BOTH ways in one pass (the
    // both-directions twin of `$__wasm_str_upper_lower`). Allocating (calls
    // `$__alloc` + `i32.store8`), so it rides `needs_heap` — a swapcase sets the
    // heap gate via `expr_has_heap_op`. Byte-parallel (no char helper — a non-ASCII
    // byte traps rather than folding). Gated on an actual use so an unrelated
    // heap-string module carries no dead helper.
    if needs_heap && needs_swapcase {
        out.push_str(STR_SWAPCASE_HELPER);
    }
    // PMAT-1203: emit the string TITLE helper once, when any function uses
    // `s.title()` (`Expr::StrMethod`, op `Title`). The `$__wasm_str_title` helper
    // title-cases word-by-word (first ASCII letter of each word upper, the rest
    // lower, any non-letter a word boundary — stateful via a `$prev` flag).
    // Allocating (calls `$__alloc` + `i32.store8`), so it rides `needs_heap` — a
    // title sets the heap gate via `expr_has_heap_op`. A non-ASCII byte traps
    // rather than folding. Gated on an actual use so an unrelated heap-string
    // module carries no dead helper.
    if needs_heap && needs_title {
        out.push_str(STR_TITLE_HELPER);
    }
    // PMAT-1205: emit the string STRIP helper once, when any function uses
    // `s.strip()` / `s.lstrip()` / `s.rstrip()` (ops `Strip` / `LStrip` /
    // `RStrip`). The single `$__wasm_str_strip` helper serves all three (`left` /
    // `right` i32 flags pick which ends to trim). Allocating (calls `$__alloc` +
    // `memory.copy`), so it rides `needs_heap` — a strip sets the heap gate via
    // `expr_has_heap_op`. A non-ASCII BOUNDARY byte traps (the honest ASCII-only
    // boundary). Gated on an actual use so an unrelated heap-string module carries
    // no dead helper.
    if needs_heap && needs_strip {
        out.push_str(STR_STRIP_HELPER);
    }
    // PMAT-1209: emit the string PAD helper once, when any function uses
    // `s.rjust(w)` / `s.ljust(w)` / `s.center(w)` (ops `RJust` / `LJust` /
    // `Center`). The single `$__wasm_str_pad` helper serves all three (a `mode` i32
    // flag picks rjust=0 / ljust=1 / center=2). Allocating (calls `$__alloc` +
    // `memory.fill` + `memory.copy`), so it rides `needs_heap` — a pad sets the heap
    // gate via `expr_has_heap_op`. Its width math uses `$__wasm_str_charlen` (emitted
    // above via `module_touches_str`). Char-exact for any UTF-8 (pad is ASCII space,
    // `s` copied verbatim), so no trap arm. Gated on an actual use so an unrelated
    // heap-string module carries no dead helper.
    if needs_heap && needs_pad {
        out.push_str(STR_PAD_HELPER);
    }
    // PMAT-1213: emit the string REVERSE helper once, when any function uses `s[::-1]`
    // (`Expr::StrMethod`, op `Reverse`). The `$__wasm_str_reverse` helper copies each
    // UTF-8 code point of `s` as an intact unit into reverse order. Allocating (calls
    // `$__alloc` + `memory.copy`), so it rides `needs_heap` — a reverse sets the heap
    // gate via `expr_has_heap_op`. Unlike the case-fold ops it needs NO Unicode table,
    // so it is char-exact for any valid UTF-8 with NO trap arm. Gated on an actual use
    // so an unrelated heap-string module carries no dead helper.
    if needs_heap && needs_reverse {
        out.push_str(STR_REVERSE_HELPER);
    }
    // PMAT-1219: emit the string EXPANDTABS helper once, when any function uses
    // `s.expandtabs()` / `s.expandtabs(tabsize)` (`Expr::StrMethod`, op `ExpandTabs`).
    // The `$__wasm_str_expandtabs` helper expands each `\t` to spaces to the next
    // multiple of `tabsize` (column counted in CODE POINTS, reset on `\n`/`\r`).
    // Allocating (calls `$__alloc` + `memory.fill` + `memory.copy`), so it rides
    // `needs_heap` — an expandtabs sets the heap gate via `expr_has_heap_op`. Like
    // reverse it needs NO Unicode table (only ASCII tab/newline bytes are
    // interpreted; the payload is copied verbatim), so it is char-exact for any valid
    // UTF-8 with NO trap arm. Gated on an actual use so an unrelated heap-string
    // module carries no dead helper.
    if needs_heap && needs_expandtabs {
        out.push_str(STR_EXPANDTABS_HELPER);
    }
    // PMAT-1189: emit the string ISDIGIT helper once, when any function uses
    // `s.isdigit()` (`Expr::StrMethod`, op `IsDigit`). NON-allocating (a single
    // byte scan returning a bool), so — unlike the case-fold helpers — it is
    // gated ONLY on `needs_isdigit`, NOT `needs_heap`: a read-only str module
    // (`def f(s): return s.isdigit()`) carries no allocator but still needs this
    // helper. Gated on an actual use so an unrelated module carries no dead code.
    if needs_isdigit {
        out.push_str(STR_ISDIGIT_HELPER);
    }
    // PMAT-1191: emit the string ISALPHA helper once, when any function uses
    // `s.isalpha()` (`Expr::StrMethod`, op `IsAlpha`). Like `isdigit` it is
    // non-allocating (a single byte scan returning a bool), so it is gated ONLY
    // on `needs_isalpha`, NOT `needs_heap`: a read-only str module
    // (`def f(s): return s.isalpha()`) carries no allocator but still needs it.
    if needs_isalpha {
        out.push_str(STR_ISALPHA_HELPER);
    }
    // PMAT-1193: emit the string ISSPACE helper once, when any function uses
    // `s.isspace()` (`Expr::StrMethod`, op `IsSpace`). Like isdigit/isalpha it is
    // non-allocating (a single byte scan returning a bool), so it is gated ONLY
    // on `needs_isspace`, NOT `needs_heap`: a read-only str module
    // (`def f(s): return s.isspace()`) carries no allocator but still needs it.
    if needs_isspace {
        out.push_str(STR_ISSPACE_HELPER);
    }
    // PMAT-1195: emit the string ISALNUM helper once, when any function uses
    // `s.isalnum()` (`Expr::StrMethod`, op `IsAlnum`). Like isdigit/isalpha/
    // isspace it is non-allocating (a single byte scan returning a bool), so it
    // is gated ONLY on `needs_isalnum`, NOT `needs_heap`: a read-only str module
    // (`def f(s): return s.isalnum()`) carries no allocator but still needs it.
    if needs_isalnum {
        out.push_str(STR_ISALNUM_HELPER);
    }
    // PMAT-1197: emit the shared string ISUPPER/ISLOWER helper once, when any
    // function uses `s.isupper()` or `s.islower()` (ops `IsUpper` / `IsLower`).
    // Like isdigit/isalpha/isspace/isalnum it is non-allocating (a single byte
    // scan returning a bool), so it is gated ONLY on `needs_isupper_lower`, NOT
    // `needs_heap`: a read-only str module (`def f(s): return s.isupper()`)
    // carries no allocator but still needs it. One helper serves both directions
    // (a `want_upper` i32 flag), like the `$__wasm_str_upper_lower` case-fold pair.
    if needs_isupper_lower {
        out.push_str(STR_ISUPPER_ISLOWER_HELPER);
    }
    // PMAT-1199: emit the string ISASCII helper once, when any function uses
    // `s.isascii()` (`Expr::StrMethod`, op `IsAscii`). Like the six sibling
    // predicates it is non-allocating (a single byte scan returning a bool), so it
    // is gated ONLY on `needs_isascii`, NOT `needs_heap`: a read-only str module
    // (`def f(s): return s.isascii()`) carries no allocator but still needs it.
    // Unlike the isdigit family it is fully decidable (no `unreachable` trap arm).
    if needs_isascii {
        out.push_str(STR_ISASCII_HELPER);
    }
    // PMAT-1248: emit the list-INT-SUM reduction helper once, when any function
    // uses `sum(xs)` over a `list[int]` (`Expr::Sum { of_float: false }`). Like
    // the byte-scan `is*` predicates it is NON-allocating (it folds the list
    // payload into an i64 total, touching no heap), so it is gated ONLY on
    // `needs_list_sum`, NOT `needs_heap`: a `def total(xs: list[int]) -> int:
    // return sum(xs)` module carries no allocator but still needs it.
    if needs_list_sum {
        out.push_str(LIST_SUM_INT_HELPER);
    }
    // PMAT-1249: the list[float] sum reduction helper — the f64-accumulator twin
    // of the i64 helper above, gated on its own `needs_list_sum_float` (also
    // non-allocating, so likewise NOT on `needs_heap`).
    if needs_list_sum_float {
        out.push_str(LIST_SUM_FLOAT_HELPER);
    }
    // PMAT-1250: emit the list-INT-MIN/MAX reduction helper once when any function
    // uses `min(xs)`/`max(xs)` over a `list[int]` (`Expr::ListMinMax`). Like the
    // sum helpers it is non-allocating (a payload fold), so gated ONLY on
    // `needs_list_minmax`, NOT `needs_heap`.
    if needs_list_minmax {
        out.push_str(LIST_MINMAX_INT_HELPER);
    }
    // PMAT-1250: the list[float] min/max twin — the f64-accumulator sibling,
    // gated on its own `needs_list_minmax_float` (also non-allocating).
    if needs_list_minmax_float {
        out.push_str(LIST_MINMAX_FLOAT_HELPER);
    }
    // PMAT-1251: emit the list-BOOL any/all reduction helper once, when any
    // function uses `any(xs)`/`all(xs)` over a `list[bool]` (`Expr::BoolReduce`,
    // direct non-generator form). Non-allocating (a payload fold, like sum/
    // minmax), so gated ONLY on `needs_list_bool_reduce`, NOT `needs_heap`.
    if needs_list_bool_reduce {
        out.push_str(LIST_BOOL_REDUCE_HELPER);
    }
    // PMAT-1252: emit the list-SORT reduction helpers once, when any function
    // uses `sorted(xs)` over a `list[int]` / `list[float]` (`Expr::Sorted`). Each
    // ALLOCATES a fresh sorted record via `$__alloc` (so the module also carries
    // the bump heap + `(memory)` via `needs_heap`); each rides its OWN gate so a
    // module that sorts only ints carries no dead f64 helper and vice-versa. The
    // helpers call `$__alloc`, emitted above under `needs_heap` — WAT function
    // references are order-independent, so a forward reference is fine.
    if needs_list_sorted {
        out.push_str(LIST_SORTED_INT_HELPER);
    }
    if needs_list_sorted_float {
        out.push_str(LIST_SORTED_FLOAT_HELPER);
    }
    // PMAT-1291: emit the SET→LIST materialisation helper once, when any function
    // sorts a set (`sorted(s)` → `Sorted { list: SetToList { set } }`). It copies
    // an int set's keys into a fresh `list[int]` record via `$__alloc` (so the
    // module also carries the bump heap + `(memory)` via `needs_heap`, which
    // `sorted` already forces); `$__wasm_list_sorted_i64` (gated above) then sorts
    // a copy of that record. Int sets only (a str set → `list[str]`, unmodelled).
    if needs_set_to_list {
        out.push_str(SET_TO_LIST_INT_HELPER);
    }
    // PMAT-1253: emit the list-REVERSE helper once, when any function uses
    // `reversed(xs)` / `list(reversed(xs))` / `xs[::-1]` over a `list[int]` /
    // `list[float]` (`Expr::Reversed`). It ALLOCATES a fresh reversed record via
    // `$__alloc` (so the module also carries the bump heap + `(memory)` via
    // `needs_heap`). ONE helper serves both int and float — reversal moves 8-byte
    // words verbatim, never interpreting them — so there is no dead-twin problem
    // (contrast the two typed sort helpers above).
    if needs_list_reversed {
        out.push_str(LIST_REVERSED_HELPER);
    }
    // PMAT-1255: emit the list-CONCAT helper once, when any function uses
    // `a + b` over two `list[int]`/`list[float]` (`Expr::ListConcat`). It
    // ALLOCATES a fresh record holding `na + nb` elements via `$__alloc` (so the
    // module also carries the bump heap + `(memory)` via `needs_heap`). ONE
    // helper serves both int and float — concat moves 8-byte words verbatim,
    // never interpreting them — so there is no dead-twin problem (contrast the
    // two typed sort helpers above; this mirrors the single reverse helper).
    if needs_list_concat {
        out.push_str(LIST_CONCAT_HELPER);
    }
    // PMAT-1256: emit the list-SLICE helper once, when any function uses
    // `xs[lo:hi]` over a `list[int]` / `list[float]` (`Expr::Slice { of_str:
    // false, step: None }`). It ALLOCATES a fresh sub-list record via `$__alloc`
    // (so the module also carries the bump heap + `(memory)` via `needs_heap`).
    // ONE helper serves both int and float — slicing moves 8-byte words verbatim,
    // never interpreting them — so there is no dead-twin problem (like the reverse
    // and concat helpers; contrast the two typed sort helpers above).
    if needs_list_slice {
        out.push_str(LIST_SLICE_HELPER);
    }
    // PMAT-1262: emit the list-MEMBERSHIP helpers once, when any function tests
    // `x in xs` / `x not in xs` over a `list[int]`/`list[float]`
    // (`Expr::ListContains`). Each is NON-allocating (a linear scan over the list
    // payload, like sum/minmax), so gated ONLY on `needs_list_contains`, NOT
    // `needs_heap`. Because the node carries no element-kind discriminant (unlike
    // the `of_float`-tagged Sum/ListMinMax), BOTH typed helpers are emitted; the
    // unused twin (e.g. the f64 helper in an all-int module) is a harmless dead
    // function (a valid, uncalled WAT export — contrast the precisely-gated
    // sum/minmax twins, which CAN see their kind in the HIR node).
    if needs_list_contains {
        out.push_str(LIST_CONTAINS_INT_HELPER);
        out.push_str(LIST_CONTAINS_FLOAT_HELPER);
    }
    // PMAT-1274: emit the list-COUNT helpers once, when any function uses
    // `xs.count(v)` over a `list[int]`/`list[float]` (`Expr::ListQuery` with
    // `Count`). NON-allocating (a linear scan, like contains), so gated on
    // `needs_list_count`, NOT `needs_heap`. Both typed helpers are emitted (no
    // element-kind discriminant on the node); the unused twin is harmless dead
    // WAT. The `index` twin is gated separately below.
    if needs_list_count {
        out.push_str(LIST_COUNT_INT_HELPER);
        out.push_str(LIST_COUNT_FLOAT_HELPER);
    }
    // PMAT-1274: emit the list-INDEX helpers once, when any function uses
    // `xs.index(v)` (`Expr::ListQuery` with `Index`). Same non-allocating
    // posture; `index` traps (`unreachable`) on a miss (Python `ValueError`).
    if needs_list_index {
        out.push_str(LIST_INDEX_INT_HELPER);
        out.push_str(LIST_INDEX_FLOAT_HELPER);
    }
    // PMAT-1282: emit the list-INSERT helpers once, when any function uses
    // `xs.insert(i, v)` over a `list[int]`/`list[float]` (`Stmt::ListInsert`). Each
    // shifts the tail + writes in place (it GROWS the count, so — like `append` —
    // only literal-bound lists with spare capacity are accepted at the call site;
    // a full record traps). Both typed helpers are emitted (no element-kind
    // discriminant on the node); the unused twin is harmless dead WAT.
    if needs_list_insert {
        out.push_str(LIST_INSERT_INT_HELPER);
        out.push_str(LIST_INSERT_FLOAT_HELPER);
    }
    // PMAT-1284: emit the single list DELETE-AT-INDEX helper once, when any
    // function uses `del xs[i]` over a `list[int]`/`list[float]` (`Stmt::DelItem`,
    // `is_dict == false`). It shrinks+shifts in place (the base-pointer never
    // moves, so unlike `insert`/`append` it accepts ANY list local — a param
    // included — with no capacity guard). The shift is a pure 8-byte word move, so
    // ONE helper serves both element kinds (no dead twin).
    if needs_list_delitem {
        out.push_str(LIST_DELITEM_HELPER);
    }
    // PMAT-1285: emit the list REMOVE-BY-VALUE helpers once, when any function uses
    // `xs.remove(v)` over a `list[int]`/`list[float]` (`Stmt::ListRemoveValue`). It
    // scans for the first value-match (typed `i64.eq`/`f64.eq`), shifts the tail
    // left in place, and drops the count — or traps (`unreachable` = ValueError) on
    // a miss. It shrinks in place (the base-pointer never moves), so — unlike
    // `insert`/`append` — the call site accepts ANY scalar list local (a param
    // included), no capacity guard. Unlike `del`, the value compare is TYPED, so
    // BOTH twins are emitted (the unused twin is harmless dead WAT, like `contains`).
    if needs_list_remove {
        out.push_str(LIST_REMOVE_INT_HELPER);
        out.push_str(LIST_REMOVE_FLOAT_HELPER);
    }
    // PMAT-1286: emit the single IN-PLACE list-REVERSE helper once, when any
    // function uses `xs.reverse()` over a `list[int]`/`list[float]`
    // (`Stmt::ListMutate` with `ListMutateOp::Reverse`). It swaps 8-byte words
    // two-pointer in place (the base-pointer never moves, so — like
    // `del`/`remove` — it accepts ANY scalar list local, a param included, with
    // no capacity guard). The swap is a pure 8-byte word move, so ONE helper
    // serves both element kinds (no dead twin, unlike the typed `sorted` pair).
    if needs_list_reverse {
        out.push_str(LIST_REVERSE_INPLACE_HELPER);
    }
    // PMAT-1288: emit the IN-PLACE list-SORT helper pair once, when any function
    // uses `xs.sort()` / `xs.sort(reverse=True)` over a `list[int]`/`list[float]`
    // (`Stmt::ListMutate` with `Sort`/`SortDesc`). The stable insertion sort runs
    // directly over the receiver's payload (the base-pointer never moves, so —
    // like `reverse`/`del`/`remove` — it accepts ANY scalar list local, a param
    // included, with no capacity guard). The compare is TYPED, so BOTH twins are
    // emitted (the unused twin is harmless dead WAT, like `contains`/`insert`).
    if needs_list_sort_inplace {
        out.push_str(LIST_SORT_INPLACE_INT_HELPER);
        out.push_str(LIST_SORT_INPLACE_FLOAT_HELPER);
    }
    // PMAT-1289: emit the INDEXED-POP helper pair once, when any function uses
    // `xs.pop(i)` over a `list[int]`/`list[float]` (`Expr::ListPop` with an
    // index). It loads the removed element (typed), shifts the tail left in
    // place, and drops the count — `del xs[i]`'s value-returning sibling. It
    // only SHRINKS (the base-pointer never moves), so the call site accepts ANY
    // scalar list local (a param included), no capacity guard. The value
    // load/return is TYPED, so BOTH twins are emitted (the unused twin is
    // harmless dead WAT, like `contains`/`remove`).
    if needs_list_pop_idx {
        out.push_str(LIST_POP_INDEX_INT_HELPER);
        out.push_str(LIST_POP_INDEX_FLOAT_HELPER);
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
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => expr_touches_str(elem),
        Stmt::ListInsert { index, elem, .. } => expr_touches_str(index) || expr_touches_str(elem),
        Stmt::SideEffectCall { call } => expr_touches_str(call),
        // PMAT-1234: `del d[k]` over a str-keyed dict — the KEY (`del d[chr(n)]`)
        // can be the sole str-touching site in a function, gating `(memory)` +
        // the char-helper family; scan it (the write-side sibling of DictSet).
        Stmt::DelItem { key, .. } => expr_touches_str(key),
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
        // PMAT-1223: `d.get(k, default)` — recurse into all three operands.
        Expr::DictGetOr { dict, key, default } => {
            expr_touches_str(dict) || expr_touches_str(key) || expr_touches_str(default)
        }
        // PMAT-1225: `d.pop(k[, default])` — recurse into all operands.
        Expr::DictPop { dict, key, default } => {
            expr_touches_str(dict)
                || expr_touches_str(key)
                || default.as_deref().is_some_and(expr_touches_str)
        }
        Expr::ListPop { list, index } => {
            expr_touches_str(list)
                || index
                    .as_deref()
                    .map(pop_index_scan_expr)
                    .is_some_and(expr_touches_str)
        }
        // PMAT-1227: `d.setdefault(k, default)` — recurse into all three operands.
        Expr::DictSetDefault { dict, key, default } => {
            expr_touches_str(dict) || expr_touches_str(key) || expr_touches_str(default)
        }
        Expr::SetContains { set, elem } => expr_touches_str(set) || expr_touches_str(elem),
        Expr::ListContains { list, elem } => expr_touches_str(list) || expr_touches_str(elem),
        Expr::ListQuery { list, arg, .. } => expr_touches_str(list) || expr_touches_str(arg),
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
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => expr_has_str_slice(elem),
        Stmt::ListInsert { index, elem, .. } => {
            expr_has_str_slice(index) || expr_has_str_slice(elem)
        }
        Stmt::SideEffectCall { call } => expr_has_str_slice(call),
        // PMAT-1234: `del d[s[1:4]]` — the KEY can host the slice helper.
        Stmt::DelItem { key, .. } => expr_has_str_slice(key),
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
        // PMAT-1223: `d.get(k, default)` — recurse into all three operands.
        Expr::DictGetOr { dict, key, default } => {
            expr_has_str_slice(dict) || expr_has_str_slice(key) || expr_has_str_slice(default)
        }
        // PMAT-1225: `d.pop(k[, default])` — recurse into all operands.
        Expr::DictPop { dict, key, default } => {
            expr_has_str_slice(dict)
                || expr_has_str_slice(key)
                || default.as_deref().is_some_and(expr_has_str_slice)
        }
        Expr::ListPop { list, index } => {
            expr_has_str_slice(list)
                || index
                    .as_deref()
                    .map(pop_index_scan_expr)
                    .is_some_and(expr_has_str_slice)
        }
        // PMAT-1227: `d.setdefault(k, default)` — recurse into all three operands.
        Expr::DictSetDefault { dict, key, default } => {
            expr_has_str_slice(dict) || expr_has_str_slice(key) || expr_has_str_slice(default)
        }
        Expr::SetContains { set, elem } => expr_has_str_slice(set) || expr_has_str_slice(elem),
        Expr::ListContains { list, elem } => expr_has_str_slice(list) || expr_has_str_slice(elem),
        Expr::ListQuery { list, arg, .. } => expr_has_str_slice(list) || expr_has_str_slice(arg),
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
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => expr_has_int_to_str(elem),
        Stmt::ListInsert { index, elem, .. } => {
            expr_has_int_to_str(index) || expr_has_int_to_str(elem)
        }
        Stmt::SideEffectCall { call } => expr_has_int_to_str(call),
        // PMAT-1234: `del d[str(n)]` over a str-keyed dict — the KEY routes a
        // `ToStr` through `emit_dict_key`, emitting `call $__wasm_int_to_str`;
        // scan it so the helper is never called-but-undeclared (the exact
        // gate-hole class PMAT-1151 fixed for the DictSet write side).
        Stmt::DelItem { key, .. } => expr_has_int_to_str(key),
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
        // PMAT-1223: `d.get(k, default)` — a str-keyed `d.get(str(n), 0)` (or an
        // int-to-str-hosting default) must gate `$__wasm_int_to_str`.
        Expr::DictGetOr { dict, key, default } => {
            expr_has_int_to_str(dict) || expr_has_int_to_str(key) || expr_has_int_to_str(default)
        }
        // PMAT-1225: `d.pop(k[, default])` — recurse into all operands (a
        // `str(n)` key/default must still gate `$__wasm_int_to_str`).
        Expr::DictPop { dict, key, default } => {
            expr_has_int_to_str(dict)
                || expr_has_int_to_str(key)
                || default.as_deref().is_some_and(expr_has_int_to_str)
        }
        Expr::ListPop { list, index } => {
            expr_has_int_to_str(list)
                || index
                    .as_deref()
                    .map(pop_index_scan_expr)
                    .is_some_and(expr_has_int_to_str)
        }
        // PMAT-1227: `d.setdefault(k, default)` — a str-keyed `d.setdefault(str(n),
        // 0)` (or an int-to-str-hosting default) must gate `$__wasm_int_to_str`.
        Expr::DictSetDefault { dict, key, default } => {
            expr_has_int_to_str(dict) || expr_has_int_to_str(key) || expr_has_int_to_str(default)
        }
        Expr::SetContains { set, elem } => expr_has_int_to_str(set) || expr_has_int_to_str(elem),
        Expr::ListContains { list, elem } => expr_has_int_to_str(list) || expr_has_int_to_str(elem),
        Expr::ListQuery { list, arg, .. } => expr_has_int_to_str(list) || expr_has_int_to_str(arg),
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
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => expr_uses_str_method(elem, op),
        Stmt::ListInsert { index, elem, .. } => {
            expr_uses_str_method(index, op) || expr_uses_str_method(elem, op)
        }
        Stmt::SideEffectCall { call } => expr_uses_str_method(call, op),
        // PMAT-1234: `del d[s.upper()]` — the KEY can host a str-method call.
        Stmt::DelItem { key, .. } => expr_uses_str_method(key, op),
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
        // PMAT-1223: `d.get(k, default)` — recurse into all three operands.
        Expr::DictGetOr { dict, key, default } => {
            expr_uses_str_method(dict, op)
                || expr_uses_str_method(key, op)
                || expr_uses_str_method(default, op)
        }
        // PMAT-1225: `d.pop(k[, default])` — recurse into all operands.
        Expr::DictPop { dict, key, default } => {
            expr_uses_str_method(dict, op)
                || expr_uses_str_method(key, op)
                || default
                    .as_deref()
                    .is_some_and(|d| expr_uses_str_method(d, op))
        }
        Expr::ListPop { list, index } => {
            expr_uses_str_method(list, op)
                || index
                    .as_deref()
                    .map(pop_index_scan_expr)
                    .is_some_and(|i| expr_uses_str_method(i, op))
        }
        // PMAT-1227: `d.setdefault(k, default)` — recurse into all three operands.
        Expr::DictSetDefault { dict, key, default } => {
            expr_uses_str_method(dict, op)
                || expr_uses_str_method(key, op)
                || expr_uses_str_method(default, op)
        }
        Expr::SetContains { set, elem } => {
            expr_uses_str_method(set, op) || expr_uses_str_method(elem, op)
        }
        Expr::ListContains { list, elem } => {
            expr_uses_str_method(list, op) || expr_uses_str_method(elem, op)
        }
        Expr::ListQuery { list, arg, .. } => {
            expr_uses_str_method(list, op) || expr_uses_str_method(arg, op)
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
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => expr_has_str_contains(elem),
        Stmt::ListInsert { index, elem, .. } => {
            expr_has_str_contains(index) || expr_has_str_contains(elem)
        }
        Stmt::SideEffectCall { call } => expr_has_str_contains(call),
        // PMAT-1234: `del d[1 if "a" in s else 0]` — the KEY can host `x in s`.
        Stmt::DelItem { key, .. } => expr_has_str_contains(key),
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
        // PMAT-1223: `d.get(k, default)` — recurse into all three operands.
        Expr::DictGetOr { dict, key, default } => {
            expr_has_str_contains(dict)
                || expr_has_str_contains(key)
                || expr_has_str_contains(default)
        }
        // PMAT-1225: `d.pop(k[, default])` — recurse into all operands.
        Expr::DictPop { dict, key, default } => {
            expr_has_str_contains(dict)
                || expr_has_str_contains(key)
                || default.as_deref().is_some_and(expr_has_str_contains)
        }
        Expr::ListPop { list, index } => {
            expr_has_str_contains(list)
                || index
                    .as_deref()
                    .map(pop_index_scan_expr)
                    .is_some_and(expr_has_str_contains)
        }
        // PMAT-1227: `d.setdefault(k, default)` — recurse into all three operands.
        Expr::DictSetDefault { dict, key, default } => {
            expr_has_str_contains(dict)
                || expr_has_str_contains(key)
                || expr_has_str_contains(default)
        }
        Expr::SetContains { set, elem } => {
            expr_has_str_contains(set) || expr_has_str_contains(elem)
        }
        Expr::ListContains { list, elem } => {
            expr_has_str_contains(list) || expr_has_str_contains(elem)
        }
        Expr::ListQuery { list, arg, .. } => {
            expr_has_str_contains(list) || expr_has_str_contains(arg)
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

/// PMAT-1248: does any function reduce a list with `sum(xs)` over a
/// `list[int]` (`Expr::Sum { of_float: false, .. }`)? Gates the
/// `$__wasm_list_sum_i64` reduction helper (and the `(memory …)` its list-
/// payload loads read) so a module with no int-list sum carries no dead
/// helper. Exhaustive over the same stmt/expr forms as the other gate walkers
/// (`expr_has_str_repeat` &c.) — a missed sub-expression would leave the
/// helper undeclared at the `call $__wasm_list_sum_i64` site (a hard wat2wasm
/// failure, the recurring gate-hole class). Runs AFTER `desugar_module_foreach`
/// (like every gate scan), so it needs no `Stmt::ForEach` arm. The `of_float`
/// (list[float]) form is gated separately by [`module_uses_list_sum_float`]
/// (PMAT-1249) — the two share the `want_float`-parametrised walk below so each
/// kind emits exactly its own helper.
fn module_uses_list_sum(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_sum(&f.body, false))
}

/// PMAT-1249: the twin gate for `sum(xs)` over a `list[float]`
/// (`Expr::Sum { of_float: true, .. }`), driving the `$__wasm_list_sum_f64`
/// helper. Shares the exhaustive walk below with the int gate — only the
/// `want_float` flag differs — so the int and float sums each emit exactly their
/// own helper and no dead one (a float-only module carries no i64 helper and
/// vice-versa).
fn module_uses_list_sum_float(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_sum(&f.body, true))
}

fn block_has_list_sum(block: &Block, want_float: bool) -> bool {
    block.stmts.iter().any(|s| stmt_has_list_sum(s, want_float))
        || expr_has_list_sum(&block.trailing_return, want_float)
}

fn stmt_has_list_sum(s: &Stmt, want_float: bool) -> bool {
    let e = |x| expr_has_list_sum(x, want_float);
    let st = |x| stmt_has_list_sum(x, want_float);
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => e(cond) || then_body.iter().any(st) || else_body.iter().any(st),
        Stmt::While { cond, body } => e(cond) || body.iter().any(st),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_list_sum(expr: &Expr, want_float: bool) -> bool {
    let e = |x| expr_has_list_sum(x, want_float);
    match expr {
        // this node IS the list sum of the kind we gate — no need to recurse
        // (its only operand in the supported shape is a bare list NAME).
        Expr::Sum { of_float, .. } if *of_float == want_float => true,
        // a sum of the OTHER kind is a distinct helper; still recurse into its
        // operands (a supported same-kind sum could be nested inside, degenerate
        // but exhaustive).
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::DictPop { dict, key, default } => {
            e(dict) || e(key) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::ListPop { list, index } => {
            e(list) || index.as_deref().map(pop_index_scan_expr).is_some_and(e)
        }
        Expr::SetContains { set, elem } => e(set) || e(elem),
        // PMAT-1262: a list membership `x in xs` can nest a helper-gated op in its
        // needle (`sum(ys) in xs`, `len(s) in xs`), so recurse into both operands.
        Expr::ListContains { list, elem } => e(list) || e(elem),
        // PMAT-1274: `xs.count(v)`/`xs.index(v)` can nest a helper-gated op in its
        // needle (`xs.count(sum(ys))`), so recurse into both operands.
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        // PMAT-1248: a `sum(xs)` can be nested inside a `seq * count` repeat's
        // COUNT (`"ab" * sum(xs)`, the int-repeat form — the sibling
        // `expr_has_str_repeat` covers this arm too), or inside a container
        // literal / struct field (`[sum(xs)]`, `Point(x=sum(xs))`). Recurse
        // into each so the helper is never left undeclared at a
        // `call $__wasm_list_sum_*` site (the recurring gate-hole class:
        // over-detecting is harmless — a valid unused WAT function — but
        // under-detecting is a hard wat2wasm failure).
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
}

/// PMAT-1250: does any function `min(xs)`/`max(xs)` over a `list[int]`
/// (`Expr::ListMinMax { of_float: false, .. }`)? Gates the
/// `$__wasm_list_minmax_i64` reduction helper (and the `(memory …)` its list-
/// payload loads read). Shares the `want_float`-parametrised walk below with the
/// float gate — exactly like the sum pair — so each kind emits only its own
/// helper and no dead one. Exhaustive over the same stmt/expr forms as
/// [`expr_has_list_sum`]; a missed sub-expression would leave the helper
/// undeclared at the `call $__wasm_list_minmax_i64` site (a hard wat2wasm
/// failure — the recurring gate-hole class, where over-detecting is a harmless
/// unused function but under-detecting is fatal).
fn module_uses_list_minmax(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_minmax(&f.body, false))
}

/// PMAT-1250: the twin gate for `min(xs)`/`max(xs)` over a `list[float]`
/// (`Expr::ListMinMax { of_float: true, .. }`), driving `$__wasm_list_minmax_f64`.
fn module_uses_list_minmax_float(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_minmax(&f.body, true))
}

fn block_has_list_minmax(block: &Block, want_float: bool) -> bool {
    block
        .stmts
        .iter()
        .any(|s| stmt_has_list_minmax(s, want_float))
        || expr_has_list_minmax(&block.trailing_return, want_float)
}

fn stmt_has_list_minmax(s: &Stmt, want_float: bool) -> bool {
    let e = |x| expr_has_list_minmax(x, want_float);
    let st = |x| stmt_has_list_minmax(x, want_float);
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => e(cond) || then_body.iter().any(st) || else_body.iter().any(st),
        Stmt::While { cond, body } => e(cond) || body.iter().any(st),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_list_minmax(expr: &Expr, want_float: bool) -> bool {
    let e = |x| expr_has_list_minmax(x, want_float);
    match expr {
        // this node IS the list min/max of the kind we gate — the gate must fire
        // (no need to recurse: the SUPPORTED shape carries a bare-Ident list with
        // no key/default, so nothing of interest nests).
        Expr::ListMinMax { of_float, .. } if *of_float == want_float => true,
        // a min/max of the OTHER kind is a distinct helper; still recurse into its
        // operands (a supported same-kind min/max could be nested in a `default=`).
        Expr::ListMinMax { list, default, .. } => e(list) || default.as_deref().is_some_and(e),
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::DictPop { dict, key, default } => {
            e(dict) || e(key) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::ListPop { list, index } => {
            e(list) || index.as_deref().map(pop_index_scan_expr).is_some_and(e)
        }
        Expr::SetContains { set, elem } => e(set) || e(elem),
        // PMAT-1262: a list membership `x in xs` can nest a helper-gated op in its
        // needle (`sum(ys) in xs`, `len(s) in xs`), so recurse into both operands.
        Expr::ListContains { list, elem } => e(list) || e(elem),
        // PMAT-1274: `xs.count(v)`/`xs.index(v)` can nest a helper-gated op in its
        // needle (`xs.count(sum(ys))`, `xs.index(len(s))`), so recurse into both.
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
}

/// PMAT-1262: does any function test list membership with `x in xs` / `x not in
/// xs` (`Expr::ListContains`)? Gates BOTH the `$__wasm_list_contains_i64` and
/// `$__wasm_list_contains_f64` helpers (and the `(memory …)` their list-payload
/// loads read). Exhaustive over the same stmt/expr forms as
/// [`expr_has_list_minmax`]; a missed sub-expression would leave a helper
/// undeclared at the `call $__wasm_list_contains_*` site (a hard wat2wasm failure
/// — the recurring gate-hole class, where over-detecting is a harmless unused
/// function but under-detecting is fatal).
///
/// Unlike [`Expr::Sum`] / [`Expr::ListMinMax`], `ListContains` carries NO
/// `of_float` discriminant — the element kind (i64 vs f64) is only resolvable at
/// emit time via [`Scope::list_elem_of`], NOT at this module-level walker. So a
/// SINGLE gate fires on ANY membership test and BOTH typed helpers are emitted;
/// the unused twin (e.g. the f64 helper in an all-int module) is a harmless dead
/// function (contrast the precisely-gated sum/minmax int/float twins). The
/// detecting arm returns `true` directly (the gate must fire on ANY
/// `ListContains`, regardless of what nests inside — the helpers serve every use).
fn module_uses_list_contains(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_contains(&f.body))
}

fn block_has_list_contains(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_list_contains) || expr_has_list_contains(&block.trailing_return)
}

fn stmt_has_list_contains(s: &Stmt) -> bool {
    let e = expr_has_list_contains;
    let st = stmt_has_list_contains;
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => e(cond) || then_body.iter().any(st) || else_body.iter().any(st),
        Stmt::While { cond, body } => e(cond) || body.iter().any(st),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_list_contains(expr: &Expr) -> bool {
    let e = expr_has_list_contains;
    match expr {
        // this node IS a list membership test — the gate must fire (the typed
        // helpers serve every use, so no need to inspect what nests inside).
        Expr::ListContains { .. } => true,
        // PMAT-1274: a `ListContains` can nest inside a `xs.count(v)`/`xs.index(v)`
        // needle (`xs.count(3 if (y in ys) else 0)`), so recurse into both.
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        Expr::ListMinMax { list, default, .. } => e(list) || default.as_deref().is_some_and(e),
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::DictPop { dict, key, default } => {
            e(dict) || e(key) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::ListPop { list, index } => {
            e(list) || index.as_deref().map(pop_index_scan_expr).is_some_and(e)
        }
        Expr::SetContains { set, elem } => e(set) || e(elem),
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
}

/// PMAT-1282: does any function use `xs.insert(i, v)` (`Stmt::ListInsert`)? Gates
/// BOTH the `$__wasm_list_insert_i64` and `$__wasm_list_insert_f64` helpers (and
/// the `(memory …)` their shift loads/stores touch). Like `contains`/`count`, the
/// node carries NO element-kind discriminant (resolved only at emit time via
/// [`Scope::list_elem_of`]), so BOTH typed helpers are emitted under this single
/// gate; the unused twin is a harmless dead function. `ListInsert` is a STATEMENT,
/// so the walk recurses into `If`/`While` bodies only — every `for` loop has
/// already been rewritten to `While` by [`desugar_module_foreach`] before any gate
/// scan runs. It returns `true` on the FIRST `ListInsert` regardless of what nests
/// in its index/elem (the helpers serve every use); the nested-op gate-holes are
/// closed separately by the `Stmt::ListInsert` arms added to every `*_has_*` walker.
fn module_uses_list_insert(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_insert(&f.body))
}

fn block_has_list_insert(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_list_insert)
}

fn stmt_has_list_insert(s: &Stmt) -> bool {
    match s {
        Stmt::ListInsert { .. } => true,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().any(stmt_has_list_insert) || else_body.iter().any(stmt_has_list_insert)
        }
        Stmt::While { body, .. } => body.iter().any(stmt_has_list_insert),
        _ => false,
    }
}

/// PMAT-1284: does any function use `del xs[i]` over a LIST (`Stmt::DelItem` with
/// `is_dict == false`)? Gates the single `$__wasm_list_delitem` helper (and the
/// `(memory …)` its shift loads/stores touch). The node carries NO element-kind
/// discriminant (int vs float resolved at emit time via [`Scope::list_elem_of`]),
/// but the shift is a pure 8-byte word move, so ONE helper serves both — unlike
/// `insert`, no twin is emitted. A dict `del d[k]` (`is_dict == true`) rides the
/// dict helpers, NOT this gate, so it is filtered out here. `DelItem` is a
/// STATEMENT, so the walk recurses into `If`/`While` bodies only (every `for`
/// loop is already desugared to `While` before any gate scan). Nested list ops in
/// the index are gated separately by the `Stmt::DelItem { key, .. }` arms already
/// present in every `*_has_*` walker.
fn module_uses_list_delitem(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_delitem(&f.body))
}

fn block_has_list_delitem(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_list_delitem)
}

fn stmt_has_list_delitem(s: &Stmt) -> bool {
    match s {
        Stmt::DelItem { is_dict, .. } => !*is_dict,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().any(stmt_has_list_delitem)
                || else_body.iter().any(stmt_has_list_delitem)
        }
        Stmt::While { body, .. } => body.iter().any(stmt_has_list_delitem),
        _ => false,
    }
}

/// PMAT-1285: does any function use `xs.remove(v)` (`Stmt::ListRemoveValue`)? Gates
/// BOTH the `$__wasm_list_remove_i64` and `$__wasm_list_remove_f64` helpers (and the
/// `(memory …)` their scan/shift loads/stores touch). The node carries NO
/// element-kind discriminant (int vs float resolved at emit time via
/// [`Scope::list_elem_of`]), so BOTH typed helpers are emitted under this single
/// gate; the unused twin is a harmless dead function (like `contains`/`index`).
/// Unlike `del` (a pure 8-byte word move → one helper), `remove` needs a TYPED
/// value compare (`i64.eq`/`f64.eq`), so a twin is required. `ListRemoveValue` is a
/// STATEMENT, so the walk recurses into `If`/`While` bodies only (every `for` loop
/// is already desugared to `While` before any gate scan). A nested helper-gated op
/// in the removed VALUE is gated separately by the `Stmt::ListRemoveValue { value,
/// .. }` arms present in every `*_has_*` walker.
fn module_uses_list_remove(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_remove(&f.body))
}

fn block_has_list_remove(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_list_remove)
}

fn stmt_has_list_remove(s: &Stmt) -> bool {
    match s {
        Stmt::ListRemoveValue { .. } => true,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().any(stmt_has_list_remove) || else_body.iter().any(stmt_has_list_remove)
        }
        Stmt::While { body, .. } => body.iter().any(stmt_has_list_remove),
        _ => false,
    }
}

/// PMAT-1286: does any function use `xs.reverse()` over a LIST (`Stmt::ListMutate`
/// with `ListMutateOp::Reverse`)? Gates the single `$__wasm_list_reverse` helper
/// (and the `(memory …)` its swap loads/stores touch). The swap is a pure 8-byte
/// word move, so ONE helper serves both element kinds (no dead twin, like `del`).
/// A `ListMutateOp::Clear`/`Sort`/`SortDesc` is filtered out here — `clear` on a
/// dict/set is a bare header write (no helper) and list `sort`/`clear` are still
/// refused, so none of them need this gate. `ListMutate` is a STATEMENT, so the
/// walk recurses into `If`/`While` bodies only (every `for` loop is already
/// desugared to `While` before any gate scan); it nests no expression, so no
/// `*_has_*` expr walker needs a new arm.
fn module_uses_list_reverse(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_reverse(&f.body))
}

fn block_has_list_reverse(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_list_reverse)
}

fn stmt_has_list_reverse(s: &Stmt) -> bool {
    match s {
        Stmt::ListMutate {
            op: ListMutateOp::Reverse,
            ..
        } => true,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().any(stmt_has_list_reverse)
                || else_body.iter().any(stmt_has_list_reverse)
        }
        Stmt::While { body, .. } => body.iter().any(stmt_has_list_reverse),
        _ => false,
    }
}

/// PMAT-1288: does any function use `xs.sort()` / `xs.sort(reverse=True)` over a
/// LIST (`Stmt::ListMutate` with `ListMutateOp::Sort`/`SortDesc`)? Gates BOTH the
/// `$__wasm_list_sort_i64` and `$__wasm_list_sort_f64` helpers (and the
/// `(memory …)` their payload loads/stores touch). The compare is TYPED, so both
/// twins ride this single gate — the stmt does carry an `of_float` flag, but the
/// emit site resolves the element kind from [`Scope::list_elem_of`], and one gate
/// emitting both twins can never mismatch that resolution; the unused twin is
/// harmless dead WAT (like `contains`/`insert`). A `ListMutateOp::Reverse`/`Clear`
/// is filtered out here — `reverse` rides its own gate and `clear` is a bare
/// header write (no helper). `ListMutate` is a STATEMENT, so the walk recurses
/// into `If`/`While` bodies only (every `for` loop is already desugared to
/// `While` before any gate scan); it nests no expression, so no `*_has_*` expr
/// walker needs a new arm.
fn module_uses_list_sort_inplace(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_sort_inplace(&f.body))
}

fn block_has_list_sort_inplace(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_list_sort_inplace)
}

fn stmt_has_list_sort_inplace(s: &Stmt) -> bool {
    match s {
        Stmt::ListMutate {
            op: ListMutateOp::Sort | ListMutateOp::SortDesc,
            ..
        } => true,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().any(stmt_has_list_sort_inplace)
                || else_body.iter().any(stmt_has_list_sort_inplace)
        }
        Stmt::While { body, .. } => body.iter().any(stmt_has_list_sort_inplace),
        _ => false,
    }
}

/// PMAT-1274: does any function use `xs.count(v)` (`Expr::ListQuery` with
/// `ListQueryOp::Count`)? Gates BOTH the `$__wasm_list_count_i64` and
/// `$__wasm_list_count_f64` helpers (and the `(memory …)` their list-payload
/// loads read). Like `contains`, the node carries NO element-kind discriminant
/// (resolved only at emit time via [`Scope::list_elem_of`]), so BOTH typed
/// helpers are emitted under this single gate; the unused twin is a harmless
/// dead function. But the node DOES carry the `op` (Count vs Index) — available
/// at this walker — so count and index are gated SEPARATELY (the sum/minmax
/// precise-gating discipline, parametrised by `want_index` on a shared walk).
fn module_uses_list_count(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_query(&f.body, false))
}

/// PMAT-1274: the twin gate for `xs.index(v)` (`Expr::ListQuery` with
/// `ListQueryOp::Index`), driving `$__wasm_list_index_i64` / `_f64`.
fn module_uses_list_index(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_query(&f.body, true))
}

fn block_has_list_query(block: &Block, want_index: bool) -> bool {
    block
        .stmts
        .iter()
        .any(|s| stmt_has_list_query(s, want_index))
        || expr_has_list_query(&block.trailing_return, want_index)
}

fn stmt_has_list_query(s: &Stmt, want_index: bool) -> bool {
    let e = |x| expr_has_list_query(x, want_index);
    let st = |x| stmt_has_list_query(x, want_index);
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => e(cond) || then_body.iter().any(st) || else_body.iter().any(st),
        Stmt::While { cond, body } => e(cond) || body.iter().any(st),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_list_query(expr: &Expr, want_index: bool) -> bool {
    let e = |x| expr_has_list_query(x, want_index);
    match expr {
        // this node IS a list query of the OP we gate — fire (the typed helpers
        // serve every use, so no need to inspect what nests inside).
        Expr::ListQuery { op, .. } if matches!(op, ListQueryOp::Index) == want_index => true,
        // a query of the OTHER op is a distinct helper; still recurse into its
        // operands (a supported same-op query could nest inside `xs.index(ys.count(3))`).
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        Expr::ListMinMax { list, default, .. } => e(list) || default.as_deref().is_some_and(e),
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::DictPop { dict, key, default } => {
            e(dict) || e(key) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::ListPop { list, index } => {
            e(list) || index.as_deref().map(pop_index_scan_expr).is_some_and(e)
        }
        Expr::SetContains { set, elem } => e(set) || e(elem),
        // recurse into a nested membership test (a `ListQuery` could nest in its
        // needle — `(xs.count(3)) in ys`); operand order is immaterial for `||`,
        // written `elem`-first here so it is textually distinct from the sibling
        // gate walkers (this walker's own `ListQuery` arms are above).
        Expr::ListContains { list, elem } => e(elem) || e(list),
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
}

/// PMAT-1289: undo the frontend's PMAT-609 runtime-pop-index normalize wrap.
///
/// The frontend hands `Expr::ListPop.index` in exactly three shapes: a bare
/// NON-NEGATIVE literal (`xs.pop(2)`), the negative-literal rewrite
/// `len(xs) - k` (`xs.pop(-k)`, PMAT-570 — see `emit_list_pop`'s own unwrap),
/// and — for any RUNTIME index — this normalize Block:
///
/// ```text
/// { let __pidx: i64 = RAW; if __pidx < 0 { len(xs) + __pidx } else { __pidx } }
/// ```
///
/// That wrap exists for the RUST lane (`Vec::remove` takes a `usize`; a bare
/// `(i) as usize` would wrap a negative to `usize::MAX`). The WASM lane's
/// `$__wasm_list_pop_idx_*` helper applies the CPython normalize ITSELF
/// (negative `+= n`, then the `[0, n)` bounds trap), so the WASM emit wants
/// the RAW index back — emitting the Block would DOUBLE-normalize (a value in
/// `[-n, 0)` after the Block's `+ len` would be re-added `n`, silently popping
/// where CPython raises `IndexError`). This returns `Some(RAW)` when `index`
/// is exactly that Block shape, `None` otherwise (a non-matching Block falls
/// through to `emit_list_pop`'s honest refusal — never a miscompile).
fn unwrap_pop_index_normalize(index: &Expr) -> Option<&Expr> {
    let Expr::Block(block) = index else {
        return None;
    };
    let [Stmt::Let {
        name,
        value: raw,
        ty: Type::I64,
        ..
    }] = block.stmts.as_slice()
    else {
        return None;
    };
    if name != "__pidx" {
        return None;
    }
    let Expr::IfExpr {
        cond,
        then_expr,
        else_expr,
    } = &block.trailing_return
    else {
        return None;
    };
    let cond_is_neg_check = matches!(
        &**cond,
        Expr::BinOp { op: BinOp::Lt, lhs, rhs }
            if matches!(&**lhs, Expr::Ident(n) if n == "__pidx")
                && matches!(&**rhs, Expr::LitInt(0))
    );
    let then_is_len_add = matches!(
        &**then_expr,
        Expr::BinOp { op: BinOp::Add, lhs, rhs }
            if matches!(&**lhs, Expr::Len(_))
                && matches!(&**rhs, Expr::Ident(n) if n == "__pidx")
    );
    let else_is_ident = matches!(&**else_expr, Expr::Ident(n) if n == "__pidx");
    if cond_is_neg_check && then_is_len_add && else_is_ident {
        Some(raw)
    } else {
        None
    }
}

/// PMAT-1289: match the PMAT-570 negative-literal index rewrite
/// `len(<receiver>) - k` (with `k ≥ 0` and the SAME receiver being indexed —
/// `xs.pop(len(ys) - 2)` must NOT match) and return `k`.
///
/// The frontend pre-rewrites a NEGATIVE LITERAL index to `len(xs) - k` for the
/// Rust lane (`Vec` indexing takes a `usize`) in THREE places: a pop index
/// (`xs.pop(-k)`), a del index (`del xs[-k]`), and a read-side subscript
/// (`xs[-k]`). The WASM lane's runtimes apply the CPython normalise THEMSELVES
/// (negative `+= n`, then a bounds trap), so each of those emit sites must
/// recover the raw `-k` — passing the pre-rewritten value through would
/// DOUBLE-normalise, silently indexing where CPython raises `IndexError`
/// whenever `n < k ≤ 2n` (the caught corners: `[5].pop(-2)`,
/// `del xs[-4]` on 3 elements, `xs[-2]` on 1 element — the last two found by
/// the PMAT-1289 differential fuzz REFUTING shipped PMAT-1284/PMAT-1001
/// behaviour). A user-written `xs[len(xs) - k]` is HIR-identical and also
/// unwraps; on an underflow it traps exactly where the Rust lane's
/// `(len - k) as usize` panics — the safe, cross-backend-consistent posture
/// for that ambiguous corner.
fn neg_literal_index_k(index: &Expr, receiver: &str) -> Option<i64> {
    let Expr::BinOp {
        op: BinOp::Sub,
        lhs,
        rhs,
    } = index
    else {
        return None;
    };
    let Expr::Len(l) = &**lhs else {
        return None;
    };
    let Expr::Ident(n) = &**l else {
        return None;
    };
    if n != receiver {
        return None;
    }
    let Expr::LitInt(k) = &**rhs else {
        return None;
    };
    (*k >= 0).then_some(*k)
}

/// PMAT-1289: the expression a GATE WALKER should scan inside a pop INDEX —
/// the RAW index when the frontend's normalize Block wraps it (the same unwrap
/// `emit_list_pop` performs, so the walkers see exactly what the emit will
/// emit), the index itself otherwise. Without this, a gated op nested in a
/// RUNTIME index (`xs.pop(ys.index(30))` — the `ys.index` call sits INSIDE the
/// Block's `let __pidx = …`) would be invisible to every `expr_has_*` walker
/// (none has an `Expr::Block` arm — the Block itself is never emitted on this
/// lane), leaving its helper undeclared at the emitted call site — the
/// recurring gate-hole class as a hard wat2wasm failure. The Block's OWN glue
/// (`len`, `__pidx`, literals, the compare/add) gates nothing, so scanning RAW
/// alone is exhaustive.
fn pop_index_scan_expr(index: &Expr) -> &Expr {
    unwrap_pop_index_normalize(index).unwrap_or(index)
}

/// PMAT-1289: does any function use the INDEXED pop `xs.pop(i)`
/// (`Expr::ListPop` with `index: Some`)? Gates BOTH the
/// `$__wasm_list_pop_idx_i64` and `$__wasm_list_pop_idx_f64` helpers (and the
/// `(memory …)` their shifts read/write). The node carries NO element-kind
/// discriminant (resolved only at emit time via [`Scope::list_elem_of`], like
/// `ListContains`), so BOTH typed twins are emitted under this single gate;
/// the unused twin is a harmless dead function. The NO-INDEX `xs.pop()` is
/// INLINE WAT (no helper, PMAT-1278) and must NOT arm this gate — the
/// detecting arm keys on `index: Some` (the sort walker's Reverse/Clear
/// filtering discipline applied to the `index` axis). Exhaustive over the same
/// stmt/expr forms as [`expr_has_list_query`]; a missed sub-expression would
/// leave the helper undeclared at the `call $__wasm_list_pop_idx_*` site (a
/// hard wat2wasm failure — the recurring gate-hole class, where over-detecting
/// is a harmless unused function but under-detecting is fatal).
fn module_uses_list_pop_index(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_pop_index(&f.body))
}

fn block_has_list_pop_index(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_list_pop_index)
        || expr_has_list_pop_index(&block.trailing_return)
}

fn stmt_has_list_pop_index(s: &Stmt) -> bool {
    let e = |x| expr_has_list_pop_index(x);
    let st = |x| stmt_has_list_pop_index(x);
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => e(cond) || then_body.iter().any(st) || else_body.iter().any(st),
        Stmt::While { cond, body } => e(cond) || body.iter().any(st),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_list_pop_index(expr: &Expr) -> bool {
    let e = |x| expr_has_list_pop_index(x);
    match expr {
        // this node IS an indexed pop — fire (the typed helper pair serves
        // every use, a same-gate pop nested in the index included, so no need
        // to inspect what nests inside).
        Expr::ListPop { index: Some(_), .. } => true,
        // a NO-index pop is INLINE (no helper of its own) — recurse into the
        // receiver defensively (the emit accepts only a bare name there).
        Expr::ListPop { list, index: None } => e(list),
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        Expr::ListMinMax { list, default, .. } => e(list) || default.as_deref().is_some_and(e),
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        // operand order `key`-first (commutative for `||`), textually distinct
        // from the sibling gate walkers so a bulk arm-injection edit anchored on
        // the standard DictPop+DictSetDefault text cannot inject a DUPLICATE
        // ListPop arm here (this walker's own detecting arms are above).
        Expr::DictPop { dict, key, default } => {
            e(key) || e(dict) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(key) || e(dict) || e(default),
        Expr::SetContains { set, elem } => e(set) || e(elem),
        // operand order `elem`-first, textually distinct from the sibling gate
        // walkers (this walker's own detecting arms are the ListPop pair above).
        Expr::ListContains { list, elem } => e(elem) || e(list),
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
}

/// PMAT-1251: does any function reduce a `list[bool]` with `any(xs)`/`all(xs)`
/// (`Expr::BoolReduce`)? Gates the `$__wasm_list_bool_reduce` helper (and the
/// `(memory …)` its list-payload i32 loads read). Exhaustive over the same
/// stmt/expr forms as [`expr_has_list_minmax`]; a missed sub-expression would
/// leave the helper undeclared at the `call $__wasm_list_bool_reduce` site (a
/// hard wat2wasm failure — the recurring gate-hole class, where over-detecting is
/// a harmless unused function but under-detecting is fatal). The detecting arm
/// returns `true` directly (the gate must fire on ANY `BoolReduce`, regardless of
/// what nests inside — one helper serves every use).
fn module_uses_list_bool_reduce(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_bool_reduce(&f.body))
}

fn block_has_bool_reduce(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_bool_reduce) || expr_has_bool_reduce(&block.trailing_return)
}

fn stmt_has_bool_reduce(s: &Stmt) -> bool {
    let e = |x| expr_has_bool_reduce(x);
    let st = |x| stmt_has_bool_reduce(x);
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => e(cond) || then_body.iter().any(st) || else_body.iter().any(st),
        Stmt::While { cond, body } => e(cond) || body.iter().any(st),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_bool_reduce(expr: &Expr) -> bool {
    let e = |x| expr_has_bool_reduce(x);
    match expr {
        // this node IS the bool reduce — the gate must fire (one helper serves
        // every use, so no need to inspect what nests inside).
        Expr::BoolReduce { .. } => true,
        Expr::ListMinMax { list, default, .. } => e(list) || default.as_deref().is_some_and(e),
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::DictPop { dict, key, default } => {
            e(dict) || e(key) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::ListPop { list, index } => {
            e(list) || index.as_deref().map(pop_index_scan_expr).is_some_and(e)
        }
        Expr::SetContains { set, elem } => e(set) || e(elem),
        // PMAT-1262: a list membership `x in xs` can nest a helper-gated op in its
        // needle (`sum(ys) in xs`, `len(s) in xs`), so recurse into both operands.
        Expr::ListContains { list, elem } => e(list) || e(elem),
        // PMAT-1274: `xs.count(v)`/`xs.index(v)` can nest a helper-gated op in its
        // needle (`xs.count(sum(ys))`, `xs.index(len(s))`), so recurse into both.
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
}

/// PMAT-1252: does any function `sorted(xs)` a `list[int]` (`Expr::Sorted {
/// of_float: false, .. }`)? Gates the `$__wasm_list_sorted_i64` helper. Keyed on
/// `of_float` so the int and float sorts drive INDEPENDENT gates — a module that
/// only sorts int lists carries no dead f64 helper and vice-versa. Exhaustive
/// over the same stmt/expr forms as [`expr_has_list_minmax`]; a missed
/// sub-expression would leave the helper undeclared at the `call
/// $__wasm_list_sorted_i64` site (a hard wat2wasm failure — the recurring
/// gate-hole class, where over-detecting is a harmless unused function but
/// under-detecting is fatal). The gate keys on the SAME `of_float` that
/// [`emit_list_sorted`] uses to pick the helper, so gate and emit never disagree.
fn module_uses_list_sorted(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_sorted(&f.body, false))
}

/// PMAT-1252: the twin gate for `sorted(xs)` over a `list[float]`
/// (`Expr::Sorted { of_float: true, .. }`), driving `$__wasm_list_sorted_f64`.
fn module_uses_list_sorted_float(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_sorted(&f.body, true))
}

fn block_has_list_sorted(block: &Block, want_float: bool) -> bool {
    block
        .stmts
        .iter()
        .any(|s| stmt_has_list_sorted(s, want_float))
        || expr_has_list_sorted(&block.trailing_return, want_float)
}

fn stmt_has_list_sorted(s: &Stmt, want_float: bool) -> bool {
    let e = |x| expr_has_list_sorted(x, want_float);
    let st = |x| stmt_has_list_sorted(x, want_float);
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => e(cond) || then_body.iter().any(st) || else_body.iter().any(st),
        Stmt::While { cond, body } => e(cond) || body.iter().any(st),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_list_sorted(expr: &Expr, want_float: bool) -> bool {
    let e = |x| expr_has_list_sorted(x, want_float);
    match expr {
        // this node IS the list sort of the kind we gate — the gate must fire
        // (the SUPPORTED shape carries a bare-Ident list with no key, so nothing
        // of interest nests; the detecting arm returns `true` directly).
        Expr::Sorted { of_float, .. } if *of_float == want_float => true,
        // a sort of the OTHER kind is a distinct helper; still recurse into its
        // `list` operand (defensive — a future keyed form could nest a same-kind
        // sort there).
        Expr::Sorted { list, .. } => e(list),
        Expr::ListMinMax { list, default, .. } => e(list) || default.as_deref().is_some_and(e),
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        // PMAT-1259: a list-valued concat operand can now nest a same-kind
        // `sorted(...)` (`sorted(xs) + ys`), so recurse into BOTH operands —
        // else the sort helper is left undeclared at its `call` site (a hard
        // wat2wasm failure, the recurring gate-hole class).
        Expr::ListConcat { lhs, rhs } => e(lhs) || e(rhs),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::DictPop { dict, key, default } => {
            e(dict) || e(key) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::ListPop { list, index } => {
            e(list) || index.as_deref().map(pop_index_scan_expr).is_some_and(e)
        }
        Expr::SetContains { set, elem } => e(set) || e(elem),
        // PMAT-1262: a list membership `x in xs` can nest a helper-gated op in its
        // needle (`sum(ys) in xs`, `len(s) in xs`), so recurse into both operands.
        Expr::ListContains { list, elem } => e(list) || e(elem),
        // PMAT-1274: `xs.count(v)`/`xs.index(v)` can nest a helper-gated op in its
        // needle (`xs.count(sum(ys))`, `xs.index(len(s))`), so recurse into both.
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
}

/// PMAT-1291: does any function materialise a set to a list (`Expr::SetToList`)?
/// Gates the `$__wasm_set_to_list_i64` helper. In the supported shape a
/// `SetToList` only appears as the source of `sorted(s)`
/// (`Sorted { list: SetToList { set } }`), so this walker's load-bearing arm is
/// the recursion into `Sorted`'s `list`; the detecting arm returns `true`
/// directly (the set operand is a bare Ident, nothing further nests). Exhaustive
/// over the same stmt/expr forms as [`expr_has_list_sorted`] so a `SetToList`
/// buried under any wrapper still arms the gate — else the helper is left
/// undeclared at its `call $__wasm_set_to_list_i64` site (a hard wat2wasm
/// failure, the recurring gate-hole class).
fn module_uses_set_to_list(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_set_to_list(&f.body))
}

fn block_has_set_to_list(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_set_to_list) || expr_has_set_to_list(&block.trailing_return)
}

fn stmt_has_set_to_list(s: &Stmt) -> bool {
    let e = expr_has_set_to_list;
    let st = stmt_has_set_to_list;
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => e(cond) || then_body.iter().any(st) || else_body.iter().any(st),
        Stmt::While { cond, body } => e(cond) || body.iter().any(st),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_set_to_list(expr: &Expr) -> bool {
    let e = expr_has_set_to_list;
    match expr {
        // this node IS the set→list materialisation the gate fires on.
        Expr::SetToList { .. } => true,
        // the SUPPORTED shape: `sorted(s)` = `Sorted { list: SetToList { set } }`.
        Expr::Sorted { list, .. } => e(list),
        Expr::ListMinMax { list, default, .. } => e(list) || default.as_deref().is_some_and(e),
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        Expr::ListConcat { lhs, rhs } => e(lhs) || e(rhs),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::DictPop { dict, key, default } => {
            e(dict) || e(key) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::ListPop { list, index } => {
            e(list) || index.as_deref().map(pop_index_scan_expr).is_some_and(e)
        }
        Expr::SetContains { set, elem } => e(set) || e(elem),
        Expr::ListContains { list, elem } => e(list) || e(elem),
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
}

/// PMAT-1253: does any function reverse a `list[int]` / `list[float]`
/// (`Expr::Reversed` over a genuine list)? Gates the single
/// `$__wasm_list_reversed_i64` helper (one helper serves both kinds — reversal is
/// a verbatim 8-byte-word move, so no `want_float` split unlike the sort gate).
/// Exhaustive over the same stmt/expr forms as [`expr_has_list_sorted`]; a missed
/// sub-expression would leave the helper undeclared at the `call
/// $__wasm_list_reversed_i64` site (a hard wat2wasm failure — the recurring
/// gate-hole class, where over-detecting is a harmless unused function but
/// under-detecting is fatal). The `reversed(s)` STR form lowers to
/// `Reversed(StrChars(s))` (types as `list[str]`, refused at emit — NOT this
/// helper's job), so it does NOT gate; the detecting arm excludes it.
fn module_uses_list_reversed(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_reversed(&f.body))
}

fn block_has_list_reversed(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_list_reversed) || expr_has_list_reversed(&block.trailing_return)
}

fn stmt_has_list_reversed(s: &Stmt) -> bool {
    let e = expr_has_list_reversed;
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            e(cond)
                || then_body.iter().any(stmt_has_list_reversed)
                || else_body.iter().any(stmt_has_list_reversed)
        }
        Stmt::While { cond, body } => e(cond) || body.iter().any(stmt_has_list_reversed),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_list_reversed(expr: &Expr) -> bool {
    let e = expr_has_list_reversed;
    match expr {
        // this node IS a genuine list reversal (int/float) — the gate fires. The
        // `reversed(s)` str form wraps a `StrChars` (types as `list[str]`, refused
        // at emit, not this helper), so it does NOT gate; recurse into it instead.
        Expr::Reversed { list } if !matches!(list.as_ref(), Expr::StrChars { .. }) => true,
        Expr::Reversed { list } => e(list),
        Expr::Sorted { list, .. } => e(list),
        Expr::ListMinMax { list, default, .. } => e(list) || default.as_deref().is_some_and(e),
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        // PMAT-1259: a list-valued concat operand can now nest a
        // `list(reversed(...))` (`list(reversed(xs)) + ys`), so recurse into
        // BOTH operands — else the reverse helper is left undeclared at its
        // `call` site (a hard wat2wasm failure, the recurring gate-hole class).
        Expr::ListConcat { lhs, rhs } => e(lhs) || e(rhs),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::DictPop { dict, key, default } => {
            e(dict) || e(key) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::ListPop { list, index } => {
            e(list) || index.as_deref().map(pop_index_scan_expr).is_some_and(e)
        }
        Expr::SetContains { set, elem } => e(set) || e(elem),
        // PMAT-1262: a list membership `x in xs` can nest a helper-gated op in its
        // needle (`sum(ys) in xs`, `len(s) in xs`), so recurse into both operands.
        Expr::ListContains { list, elem } => e(list) || e(elem),
        // PMAT-1274: `xs.count(v)`/`xs.index(v)` can nest a helper-gated op in its
        // needle (`xs.count(sum(ys))`, `xs.index(len(s))`), so recurse into both.
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
}

/// PMAT-1255: does any function CONCATENATE two lists with `a + b`
/// (`Expr::ListConcat`)? Gates the single `$__wasm_list_concat_i64` helper (one
/// helper serves both kinds — concat is a verbatim 8-byte-word move, so no
/// `want_float` split, exactly like the reverse gate). Exhaustive over the same
/// stmt/expr forms as [`expr_has_list_reversed`]; a missed sub-expression would
/// leave the helper undeclared at the `call $__wasm_list_concat_i64` site (a hard
/// wat2wasm failure — the recurring gate-hole class, where over-detecting is a
/// harmless unused function but under-detecting is fatal).
///
/// PMAT-1259: a concat operand is now any list-VALUED expr (`sorted`/`reversed`/
/// slice/nested-concat/list-literal, not only a bare Ident), so the OTHER
/// allocating-list gates MUST recurse into `ListConcat`'s operands — the
/// `sorted`/`reversed`/`slice` walkers (and the heap gate) each carry a
/// `ListConcat` arm for exactly that reason. This gate itself fires on ANY
/// `ListConcat` node (`=> true`), so it never needs to look inside its operands.
fn module_uses_list_concat(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_concat(&f.body))
}

fn block_has_list_concat(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_list_concat) || expr_has_list_concat(&block.trailing_return)
}

fn stmt_has_list_concat(s: &Stmt) -> bool {
    let e = expr_has_list_concat;
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            e(cond)
                || then_body.iter().any(stmt_has_list_concat)
                || else_body.iter().any(stmt_has_list_concat)
        }
        Stmt::While { cond, body } => e(cond) || body.iter().any(stmt_has_list_concat),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_list_concat(expr: &Expr) -> bool {
    let e = expr_has_list_concat;
    match expr {
        // this node IS a list concatenation — the gate fires.
        Expr::ListConcat { .. } => true,
        Expr::Reversed { list } => e(list),
        Expr::Sorted { list, .. } => e(list),
        Expr::ListMinMax { list, default, .. } => e(list) || default.as_deref().is_some_and(e),
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::DictPop { dict, key, default } => {
            e(dict) || e(key) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::ListPop { list, index } => {
            e(list) || index.as_deref().map(pop_index_scan_expr).is_some_and(e)
        }
        Expr::SetContains { set, elem } => e(set) || e(elem),
        // PMAT-1262: a list membership `x in xs` can nest a helper-gated op in its
        // needle (`sum(ys) in xs`, `len(s) in xs`), so recurse into both operands.
        Expr::ListContains { list, elem } => e(list) || e(elem),
        // PMAT-1274: `xs.count(v)`/`xs.index(v)` can nest a helper-gated op in its
        // needle (`xs.count(sum(ys))`, `xs.index(len(s))`), so recurse into both.
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
}

/// PMAT-1256: does any function SLICE a list with `xs[lo:hi]`
/// (`Expr::Slice { of_str: false, step: None }`)? Gates the single
/// `$__wasm_list_slice_i64` helper (one helper serves both kinds — a list slice
/// is a verbatim 8-byte-word range move, so no `want_float` split, exactly like
/// the reverse/concat gates). A STRING slice (`of_str: true`) is the [`STR_SLICE_HELPER`]
/// gate's business, and a STEPPED list slice (`step: Some`) is refused at emit
/// (so it must NOT arm this helper), so the detecting arm keys on BOTH
/// `of_str: false` AND `step: None`. Exhaustive over the same stmt/expr forms as
/// [`expr_has_list_concat`]; a missed sub-expression would leave the helper
/// undeclared at the `call $__wasm_list_slice_i64` site (a hard wat2wasm failure —
/// the recurring gate-hole class, where over-detecting is a harmless unused
/// function but under-detecting is fatal). The supported slice carries a bare-Ident
/// `collection`, so no OTHER list-op walker needs a list-`Slice` arm — nothing
/// supported can nest inside the sliced list (a non-Ident collection refuses at
/// emit, aborting codegen before any helper mismatch); the bounds are int exprs,
/// already recursed by every walker's existing `Slice` arm.
fn module_uses_list_slice(module: &Module) -> bool {
    module_functions(module).any(|f| block_has_list_slice(&f.body))
}

fn block_has_list_slice(block: &Block) -> bool {
    block.stmts.iter().any(stmt_has_list_slice) || expr_has_list_slice(&block.trailing_return)
}

fn stmt_has_list_slice(s: &Stmt) -> bool {
    let e = expr_has_list_slice;
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return(value) => e(value),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            e(cond)
                || then_body.iter().any(stmt_has_list_slice)
                || else_body.iter().any(stmt_has_list_slice)
        }
        Stmt::While { cond, body } => e(cond) || body.iter().any(stmt_has_list_slice),
        Stmt::FieldAssign { value, .. } => e(value),
        Stmt::IndexAssign { indices, value, .. } => indices.iter().any(e) || e(value),
        Stmt::DictSet { key, value, .. } => e(key) || e(value),
        Stmt::DelItem { key, .. } => e(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => e(elem),
        Stmt::ListInsert { index, elem, .. } => e(index) || e(elem),
        Stmt::SideEffectCall { call } => e(call),
        _ => false,
    }
}

fn expr_has_list_slice(expr: &Expr) -> bool {
    let e = expr_has_list_slice;
    match expr {
        // this node IS a (non-stepped) LIST slice — the gate fires. A string
        // slice (of_str: true) and a stepped list slice (step: Some, refused at
        // emit) do NOT arm this helper.
        Expr::Slice {
            of_str: false,
            step: None,
            ..
        } => true,
        Expr::Slice {
            collection, lo, hi, ..
        } => e(collection) || lo.as_deref().is_some_and(e) || hi.as_deref().is_some_and(e),
        Expr::ListConcat { lhs, rhs } => e(lhs) || e(rhs),
        Expr::Reversed { list } => e(list),
        Expr::Sorted { list, .. } => e(list),
        Expr::ListMinMax { list, default, .. } => e(list) || default.as_deref().is_some_and(e),
        Expr::Sum { list, start, .. } => e(list) || start.as_deref().is_some_and(e),
        Expr::Concat { lhs, rhs }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::FloatBinOp { lhs, rhs, .. } => e(lhs) || e(rhs),
        Expr::UnOp { operand, .. } => e(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => e(cond) || e(then_expr) || e(else_expr),
        Expr::Call { args, .. } => args.iter().any(e),
        Expr::MethodCall { obj, args, .. } => e(obj) || args.iter().any(e),
        Expr::Index { collection, index } => e(collection) || e(index),
        Expr::Len(c) => e(c),
        Expr::Ord { value } | Expr::Chr { value } => e(value),
        Expr::StrCharAt { string, index } => e(string) || e(index),
        Expr::StrMethod { recv, args, .. } => e(recv) || args.iter().any(e),
        Expr::StrContains { haystack, needle } => e(haystack) || e(needle),
        Expr::FieldAccess { obj, .. } => e(obj),
        Expr::ToStr { value, .. } => e(value),
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => e(dict) || e(key),
        Expr::DictGetOr { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::DictPop { dict, key, default } => {
            e(dict) || e(key) || default.as_deref().is_some_and(e)
        }
        Expr::DictSetDefault { dict, key, default } => e(dict) || e(key) || e(default),
        Expr::ListPop { list, index } => {
            e(list) || index.as_deref().map(pop_index_scan_expr).is_some_and(e)
        }
        Expr::SetContains { set, elem } => e(set) || e(elem),
        // PMAT-1262: a list membership `x in xs` can nest a helper-gated op in its
        // needle (`sum(ys) in xs`, `len(s) in xs`), so recurse into both operands.
        Expr::ListContains { list, elem } => e(list) || e(elem),
        // PMAT-1274: `xs.count(v)`/`xs.index(v)` can nest a helper-gated op in its
        // needle (`xs.count(sum(ys))`, `xs.index(len(s))`), so recurse into both.
        Expr::ListQuery { list, arg, .. } => e(list) || e(arg),
        Expr::Repeat { seq, n, .. } => e(seq) || e(n),
        Expr::ListLit(xs) | Expr::TupleLit(xs) | Expr::SetLit(xs) => xs.iter().any(e),
        Expr::DictLit(kvs) => kvs.iter().any(|(k, v)| e(k) || e(v)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| e(v)),
        _ => false,
    }
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
        // PMAT-1234: `del d[s * n]` — the KEY can host a str-repeat.
        Stmt::DelItem { key, .. } => expr_has_str_repeat(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => expr_has_str_repeat(elem),
        Stmt::ListInsert { index, elem, .. } => {
            expr_has_str_repeat(index) || expr_has_str_repeat(elem)
        }
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
        // PMAT-1223: `d.get(k, default)` — recurse into all three operands.
        Expr::DictGetOr { dict, key, default } => {
            expr_has_str_repeat(dict) || expr_has_str_repeat(key) || expr_has_str_repeat(default)
        }
        // PMAT-1225: `d.pop(k[, default])` — recurse into all operands.
        Expr::DictPop { dict, key, default } => {
            expr_has_str_repeat(dict)
                || expr_has_str_repeat(key)
                || default.as_deref().is_some_and(expr_has_str_repeat)
        }
        Expr::ListPop { list, index } => {
            expr_has_str_repeat(list)
                || index
                    .as_deref()
                    .map(pop_index_scan_expr)
                    .is_some_and(expr_has_str_repeat)
        }
        // PMAT-1227: `d.setdefault(k, default)` — recurse into all three operands.
        Expr::DictSetDefault { dict, key, default } => {
            expr_has_str_repeat(dict) || expr_has_str_repeat(key) || expr_has_str_repeat(default)
        }
        Expr::SetContains { set, elem } => expr_has_str_repeat(set) || expr_has_str_repeat(elem),
        Expr::ListContains { list, elem } => expr_has_str_repeat(list) || expr_has_str_repeat(elem),
        Expr::ListQuery { list, arg, .. } => expr_has_str_repeat(list) || expr_has_str_repeat(arg),
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
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => expr_has_str_method_2arg(elem, target),
        Stmt::ListInsert { index, elem, .. } => {
            expr_has_str_method_2arg(index, target) || expr_has_str_method_2arg(elem, target)
        }
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
        // PMAT-1223: `d.get(k, default)` — recurse into all three operands.
        Expr::DictGetOr { dict, key, default } => {
            expr_has_str_method_2arg(dict, target)
                || expr_has_str_method_2arg(key, target)
                || expr_has_str_method_2arg(default, target)
        }
        // PMAT-1225: `d.pop(k[, default])` — recurse into all operands.
        Expr::DictPop { dict, key, default } => {
            expr_has_str_method_2arg(dict, target)
                || expr_has_str_method_2arg(key, target)
                || default
                    .as_deref()
                    .is_some_and(|d| expr_has_str_method_2arg(d, target))
        }
        Expr::ListPop { list, index } => {
            expr_has_str_method_2arg(list, target)
                || index
                    .as_deref()
                    .map(pop_index_scan_expr)
                    .is_some_and(|i| expr_has_str_method_2arg(i, target))
        }
        // PMAT-1227: `d.setdefault(k, default)` — recurse into all three operands.
        Expr::DictSetDefault { dict, key, default } => {
            expr_has_str_method_2arg(dict, target)
                || expr_has_str_method_2arg(key, target)
                || expr_has_str_method_2arg(default, target)
        }
        Expr::SetContains { set, elem } => {
            expr_has_str_method_2arg(set, target) || expr_has_str_method_2arg(elem, target)
        }
        Expr::ListContains { list, elem } => {
            expr_has_str_method_2arg(list, target) || expr_has_str_method_2arg(elem, target)
        }
        Expr::ListQuery { list, arg, .. } => {
            expr_has_str_method_2arg(list, target) || expr_has_str_method_2arg(arg, target)
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
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => expr_has_str_eq(elem, scan),
        Stmt::ListInsert { index, elem, .. } => {
            expr_has_str_eq(index, scan) || expr_has_str_eq(elem, scan)
        }
        Stmt::SideEffectCall { call } => expr_has_str_eq(call, scan),
        // PMAT-1234: `del d[1 if a == b else 2]` — the KEY can host a str eq/cmp.
        Stmt::DelItem { key, .. } => expr_has_str_eq(key, scan),
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
        // PMAT-1223: `d.get(k, default)` — recurse into all three operands.
        Expr::DictGetOr { dict, key, default } => {
            expr_has_str_eq(dict, scan)
                || expr_has_str_eq(key, scan)
                || expr_has_str_eq(default, scan)
        }
        // PMAT-1225: `d.pop(k[, default])` — recurse into all operands.
        Expr::DictPop { dict, key, default } => {
            expr_has_str_eq(dict, scan)
                || expr_has_str_eq(key, scan)
                || default.as_deref().is_some_and(|d| expr_has_str_eq(d, scan))
        }
        Expr::ListPop { list, index } => {
            expr_has_str_eq(list, scan)
                || index
                    .as_deref()
                    .map(pop_index_scan_expr)
                    .is_some_and(|i| expr_has_str_eq(i, scan))
        }
        // PMAT-1227: `d.setdefault(k, default)` — recurse into all three operands.
        Expr::DictSetDefault { dict, key, default } => {
            expr_has_str_eq(dict, scan)
                || expr_has_str_eq(key, scan)
                || expr_has_str_eq(default, scan)
        }
        Expr::SetContains { set, elem } => {
            expr_has_str_eq(set, scan) || expr_has_str_eq(elem, scan)
        }
        Expr::ListContains { list, elem } => {
            expr_has_str_eq(list, scan) || expr_has_str_eq(elem, scan)
        }
        Expr::ListQuery { list, arg, .. } => {
            expr_has_str_eq(list, scan) || expr_has_str_eq(arg, scan)
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
        // PMAT-1234: `del d["a" + s]` — the KEY can host a heap-allocating op
        // (concat/chr/slice), which rides `needs_heap`; scan it.
        Stmt::DelItem { key, .. } => expr_has_heap_op(key),
        Stmt::SetAdd { elem, .. }
        | Stmt::SetRemove { elem, .. }
        | Stmt::ListAppend { elem, .. }
        | Stmt::ListRemoveValue { value: elem, .. } => expr_has_heap_op(elem),
        Stmt::ListInsert { index, elem, .. } => expr_has_heap_op(index) || expr_has_heap_op(elem),
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
        // PMAT-1256: a LIST slice `xs[lo:hi]` (`of_str: false, step: None`)
        // likewise bump-allocates a fresh sub-list record via
        // `$__wasm_list_slice_i64` → `$__alloc` (the FOURTH list-VALUED allocating
        // op), so it too forces the bump heap + `(memory)`. A stepped list slice
        // is refused at emit, but over-detecting the heap need is harmless (the
        // refusal aborts codegen before any module is produced), so ANY `Slice`
        // node arms the heap gate directly.
        Expr::Slice { .. } => true,
        // PMAT-1252: `sorted(xs)` bump-allocates a fresh sorted list record (via
        // `$__wasm_list_sorted_*` → `$__alloc`), so it forces the bump heap +
        // `(memory)` — the first list-VALUED op that allocates. The list operand
        // is a bare Ident in the supported shape (no nested allocation), so
        // returning `true` directly is both correct and safe (over-detection is
        // harmless; a miss would emit the sort helper against an undeclared
        // `$__alloc` — a hard wat2wasm failure).
        Expr::Sorted { .. } => true,
        // PMAT-1291: `sorted(s)` over a set materialises the set to a fresh
        // `list[int]` via `$__wasm_set_to_list_i64` → `$__alloc` before sorting,
        // so a `SetToList` node also forces the bump heap + `(memory)`. (In the
        // supported shape it only ever appears inside a `Sorted` node, which
        // already returns `true` above; the arm is defense-in-depth and covers
        // the recursion into `set` — a bare Ident that allocates nothing extra.)
        Expr::SetToList { .. } => true,
        // PMAT-1253: `reversed(xs)` / `list(reversed(xs))` / `xs[::-1]` over a
        // list likewise bump-allocates a fresh reversed record (via
        // `$__wasm_list_reversed_i64` → `$__alloc`), the SECOND list-VALUED
        // allocating op, so it forces the bump heap + `(memory)`. The supported
        // shape carries a bare-Ident list (no nested allocation), so returning
        // `true` directly is correct and safe (over-detection is harmless; a miss
        // would emit the reverse helper against an undeclared `$__alloc` — a hard
        // wat2wasm failure).
        Expr::Reversed { .. } => true,
        // PMAT-1255: `a + b` over two lists (`Expr::ListConcat`) likewise
        // bump-allocates a fresh concatenated record (via `$__wasm_list_concat_i64`
        // → `$__alloc`), the THIRD list-VALUED allocating op, so it forces the
        // bump heap + `(memory)`. The supported shape carries bare-Ident operands
        // (no nested allocation), so returning `true` directly is correct and safe
        // (over-detection is harmless; a miss would emit the concat helper against
        // an undeclared `$__alloc` — a hard wat2wasm failure).
        Expr::ListConcat { .. } => true,
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
        // PMAT-1187: `s.capitalize()` (op `Capitalize`) bump-allocates its
        // first-upper/rest-lower result the same way — a miss would emit
        // `$__wasm_str_capitalize` against an undeclared `$__alloc`.
        // PMAT-1201: `s.swapcase()` (op `SwapCase`) bump-allocates its both-ways
        // case-flipped result the same way — a miss would emit
        // `$__wasm_str_swapcase` against an undeclared `$__alloc`.
        // PMAT-1203: `s.title()` (op `Title`) bump-allocates its title-cased result
        // the same way — a miss would emit `$__wasm_str_title` against an undeclared
        // `$__alloc` (the same hard wat2wasm gate-hole).
        // PMAT-1205: `s.strip()` / `s.lstrip()` / `s.rstrip()` (ops `Strip` /
        // `LStrip` / `RStrip`) bump-allocate their whitespace-trimmed result the same
        // way — a miss would emit `$__wasm_str_strip` against an undeclared
        // `$__alloc` (the same hard wat2wasm gate-hole).
        // PMAT-1209: `s.rjust(w)` / `s.ljust(w)` / `s.center(w)` (ops `RJust` /
        // `LJust` / `Center`) bump-allocate their space-padded result the same way —
        // a miss would emit `$__wasm_str_pad` against an undeclared `$__alloc` (the
        // same hard wat2wasm gate-hole). Their width arg is an int (never heap), but
        // the recurse into `args` covers a heap-constructed receiver either way.
        // PMAT-1213: `s[::-1]` (op `Reverse`) bump-allocates its code-point-reversed
        // result the same way — a miss would emit `$__wasm_str_reverse` against an
        // undeclared `$__alloc` (the same hard wat2wasm gate-hole). 0-arg; the recurse
        // into `recv` covers a heap-constructed receiver (`(a + b)[::-1]`).
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
                    | StrMethodOp::Capitalize
                    | StrMethodOp::SwapCase
                    | StrMethodOp::Title
                    | StrMethodOp::Strip
                    | StrMethodOp::LStrip
                    | StrMethodOp::RStrip
                    | StrMethodOp::RJust
                    | StrMethodOp::LJust
                    | StrMethodOp::Center
                    | StrMethodOp::Reverse
                    // PMAT-1219: `s.expandtabs()` bump-allocates its tab-expanded
                    // result the same way — a miss would emit `$__wasm_str_expandtabs`
                    // against an undeclared `$__alloc` (the same hard wat2wasm
                    // gate-hole). Its tabsize arg is an int (never heap), but the
                    // recurse into `args`/`recv` covers a heap-constructed receiver.
                    | StrMethodOp::ExpandTabs
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
        // PMAT-1223: `d.get(k, default)` allocates nothing itself, but a
        // heap-constructed key/default pulls in the allocator — recurse.
        Expr::DictGetOr { dict, key, default } => {
            expr_has_heap_op(dict) || expr_has_heap_op(key) || expr_has_heap_op(default)
        }
        // PMAT-1225: `d.pop(k[, default])` — recurse into all operands so a
        // heap-constructed key/default still pulls in the allocator. `pop`
        // itself allocates nothing (it shrinks in place), so no extra flag.
        Expr::DictPop { dict, key, default } => {
            expr_has_heap_op(dict)
                || expr_has_heap_op(key)
                || default.as_deref().is_some_and(expr_has_heap_op)
        }
        // PMAT-1289: `xs.pop(i)` itself does NOT allocate (an in-place shrink) —
        // recurse only, so a nested allocating op in the INDEX still forces the
        // heap (`xs.pop(len(sorted(ys)) - 1)`).
        Expr::ListPop { list, index } => {
            expr_has_heap_op(list)
                || index
                    .as_deref()
                    .map(pop_index_scan_expr)
                    .is_some_and(expr_has_heap_op)
        }
        // PMAT-1227: `d.setdefault(k, default)` INSERTS on a miss, and the `set`
        // helper 2x-reallocs (calls `$__alloc`) when the dict is at capacity, so
        // the op ITSELF sets the heap gate — a miss would emit
        // `$__wasm_dict_set_<k>`'s grow path against an undeclared `$__alloc` (the
        // recurring gate-hole class). Unlike `d.get`/`d.pop` (which never grow),
        // this returns `true` unconditionally; the operands are covered anyway.
        Expr::DictSetDefault { .. } => true,
        Expr::SetContains { set, elem } => expr_has_heap_op(set) || expr_has_heap_op(elem),
        Expr::ListContains { list, elem } => expr_has_heap_op(list) || expr_has_heap_op(elem),
        Expr::ListQuery { list, arg, .. } => expr_has_heap_op(list) || expr_has_heap_op(arg),
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
/// `i64`/`f64`/`f32`, or (PMAT-1251) `bool` as an `i32` holding 0/1 (the
/// canonical WASM boolean encoding, a 4-byte element like `f32`). Nested
/// lists, `list[str]`, etc. are still refused.
fn map_list_elem_type(inner: &Type) -> Result<WatTy, BackendError> {
    match inner {
        Type::I64 | Type::CLong => Ok(WatTy::I64),
        Type::F64 => Ok(WatTy::F64),
        Type::F32 => Ok(WatTy::F32),
        // PMAT-1251: `list[bool]` — the third list element type. A bool has no
        // WASM type of its own; it rides an `i32` holding 0/1 (a 4-byte element
        // with a natural `i32.load`/`i32.store`, exactly like `list[f32]`). The
        // whole list surface (literals/index/index-assign/len/for) is already
        // parametrised by the element `WatTy` (byte_size/load/store), so this one
        // line enables it; `any(xs)`/`all(xs)` fold it via `emit_bool_reduce`.
        Type::Bool => Ok(WatTy::I32),
        other => Err(unsupported(&format!(
            "list element type {other:?} — the WASM list subset supports \
             list[int]/list[float]/list[bool] only (i64/f64/f32/i32 elements with \
             a natural *.load); list[str] and nested lists are refused"
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
    /// PMAT-1276: the subset of list locals that are APPEND-safe — bound by a
    /// `Let`/`Assign` to a `ListLit` (which [`emit_list_lit`] over-allocates
    /// with [`LIST_GROWTH_SLACK`] spare slots + a capacity header at
    /// [`LIST_CAP_OFFSET`]) and NEVER rebound to a non-literal list value. A
    /// list PARAM, an aliased list (`ys = xs`), or a `concat`/`sorted`/
    /// `reversed`/`slice` RESULT carries no spare capacity, so appending to it
    /// is refused at emit time — a clean compile-time refusal, never a silent
    /// runtime overrun. Computed once per function by [`collect_growable_lists`].
    growable_lists: Vec<String>,
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
    /// PMAT-1242: the NAMES in [`Scope::heap_maps`] that are `set`s (not dicts).
    /// A set and a dict both ride an `i32` base-pointer and share the
    /// `$__wasm_dict_*_<k>` entry helpers, so `heap_maps` alone cannot tell them
    /// apart — but `==`/`!=` differ: set equality is membership-only (no value
    /// slot), while dict equality must also compare values. This records which
    /// LET-bound heap maps are sets so `emit_binop` routes sets to
    /// `$__wasm_set_eq_<k>` (membership-only) and dicts to `$__wasm_dict_eq_<k>`
    /// (membership + per-key value compare, PMAT-1243).
    heap_sets: Vec<String>,
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

    /// PMAT-1242: `true` if `name` is a LET-bound `set` local (vs a dict).
    /// Both ride an i32 base-pointer and share the entry helpers, but only a
    /// set routes `==`/`!=` to `$__wasm_set_eq_<k>`.
    fn is_set(&self, name: &str) -> bool {
        self.heap_sets.iter().any(|n| n == name)
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

    /// PMAT-1290: the element WAT type if `name` is a LET-bound `set` local,
    /// else `None` — an int set's element loads as `i64` (the stored key), a
    /// str set's as `i32` (the stored str base-pointer, so the loop var behaves
    /// as a str local). Drives the `Expr::Index` over a set NAME that the
    /// `for x in s` desugar ([`desugar_foreach_stmts`]) emits per element.
    /// Distinct from [`Scope::list_elem_of`]: a set entry is a
    /// [`DICT_ENTRY_SIZE`] (16) byte record, NOT an 8-byte packed slot.
    fn set_elem_of(&self, name: &str) -> Option<WatTy> {
        if !self.is_set(name) {
            return None;
        }
        Some(match self.heap_map_kind(name)? {
            KeyKind::Int => WatTy::I64,
            KeyKind::Str => WatTy::I32,
        })
    }

    /// PMAT-1276: `true` if `name` is an APPEND-safe list local — a `ListLit`
    /// binding whose record [`emit_list_lit`] over-allocated with spare
    /// capacity. `xs.append(v)` is emitted only for these; every other list
    /// (a param, an alias, or a helper-allocated result) is refused.
    fn is_growable_list(&self, name: &str) -> bool {
        self.growable_lists.iter().any(|n| n == name)
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
        growable_lists: Vec::new(),
        str_names: Vec::new(),
        heap_maps: Vec::new(),
        heap_sets: Vec::new(),
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
    // PMAT-1276: classify which list locals are APPEND-safe (literal-bound,
    // never rebound to a helper result). Runs AFTER `collect_let_locals` so
    // every list name is registered in `scope.list_elem`.
    collect_growable_lists(&f.body.stmts, &mut scope);

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
                // PMAT-1242: mark this heap map as a SET so `emit_binop` routes
                // `s1 == s2` / `s1 != s2` to the membership-only
                // `$__wasm_set_eq_<k>` rather than the value-comparing
                // `$__wasm_dict_eq_<k>` a dict uses (PMAT-1243).
                scope.heap_sets.push(name.clone());
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
            // PMAT-1234: `del d[k]` (DelItem) removes from an existing dict in
            // place — no new local (the dict base was declared by its `Let`).
            Stmt::Assign { .. }
            | Stmt::IndexAssign { .. }
            | Stmt::DictSet { .. }
            | Stmt::DelItem { .. }
            | Stmt::SetAdd { .. }
            // PMAT-1240: `s.remove(e)`/`s.discard(e)` (SetRemove) removes from an
            // existing set in place — no new local (the set base was declared by
            // its `Let`), like `del d[k]`.
            | Stmt::SetRemove { .. }
            | Stmt::FieldAssign { .. }
            | Stmt::SideEffectCall { .. }
            // PMAT-1236: `d.clear()`/`s.clear()` (→ zero the count header in
            // place) declares no new local; the actual dict/set-vs-list support
            // decision (and the honest sort/reverse/list-clear refusal) is made
            // downstream in `emit_list_mutate`, not here.
            | Stmt::ListMutate { .. }
            | Stmt::Return(_)
            | Stmt::Break
            // PMAT-1276: `xs.append(v)` mutates an EXISTING list record in
            // place (the base-pointer never moves) — it declares no new local.
            // The append-safety decision (literal-bound + spare capacity) is
            // made downstream in `emit_list_append`, exactly as the
            // `emit_list_mutate` clear/sort/reverse decision is.
            | Stmt::ListAppend { .. }
            // PMAT-1282: `xs.insert(i, v)` shifts the tail and writes IN PLACE
            // (the base-pointer never moves) — it declares no new local either.
            // The insert-safety decision (literal-bound + spare capacity, like
            // append) is made downstream in `emit_list_insert`.
            | Stmt::ListInsert { .. }
            // PMAT-1285: `xs.remove(v)` scans+shrinks an EXISTING list record in
            // place (the base-pointer never moves) — it declares no new local. The
            // element-kind support decision is made downstream in
            // `emit_list_remove`, like the append/insert/del in-place mutators.
            | Stmt::ListRemoveValue { .. }
            | Stmt::Continue => {}
            // PMAT-1034: a `raise` (→ `unreachable` trap) introduces no
            // locals — its message expression is never evaluated on WASM.
            Stmt::Raise { .. } => {}
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

/// PMAT-1276: populate [`Scope::growable_lists`] — the list locals it is safe
/// to `xs.append(v)` on. A list name qualifies iff EVERY `Let`/`Assign` that
/// binds it has a [`Expr::ListLit`] value (the only list form
/// [`emit_list_lit`] over-allocates with spare capacity) and it is bound to a
/// literal at least once. A single non-literal binding (an alias `ys = xs`, a
/// `sorted`/`reversed`/`concat`/`slice` result, or any other list-valued
/// expression whose helper allocates NO spare capacity) disqualifies the name:
/// appending to such a record would overrun it, so `emit_list_append` refuses
/// it at compile time. List PARAMS never appear here (they have no `Let`), so
/// they are refused too — the caller sized them exactly.
fn collect_growable_lists(stmts: &[Stmt], scope: &mut Scope) {
    let mut lit_bound: Vec<String> = Vec::new();
    let mut disqualified: Vec<String> = Vec::new();
    scan_list_bindings(stmts, scope, &mut lit_bound, &mut disqualified);
    for name in lit_bound {
        if !disqualified.contains(&name) && !scope.growable_lists.contains(&name) {
            scope.growable_lists.push(name);
        }
    }
}

/// Recursive worker for [`collect_growable_lists`]: record each list-local
/// binding as literal (→ `lit_bound`) or non-literal (→ `disqualified`),
/// descending into `If`/`While` bodies (an append target may be bound inside a
/// branch or loop).
fn scan_list_bindings(
    stmts: &[Stmt],
    scope: &Scope,
    lit_bound: &mut Vec<String>,
    disqualified: &mut Vec<String>,
) {
    for s in stmts {
        match s {
            Stmt::Let { name, value, .. } | Stmt::Assign { name, value } => {
                if scope.list_elem_of(name).is_none() {
                    continue; // not a list local — irrelevant to append safety
                }
                if matches!(value, Expr::ListLit(_)) {
                    lit_bound.push(name.clone());
                } else {
                    disqualified.push(name.clone());
                }
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                scan_list_bindings(then_body, scope, lit_bound, disqualified);
                scan_list_bindings(else_body, scope, lit_bound, disqualified);
            }
            Stmt::While { body, .. } => {
                scan_list_bindings(body, scope, lit_bound, disqualified);
            }
            _ => {}
        }
    }
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
        Stmt::DelItem { .. } => "DelItem",
        Stmt::SetRemove { .. } => "SetRemove",
        Stmt::ListMutate { .. } => "ListMutate",
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
        // PMAT-1234: `del d[k]` — dict entry removal as a STATEMENT. The
        // companion of `d.pop(k)`: it reuses the SAME removal helper
        // (`$__wasm_dict_pop_<k>`, swap-last-into-hole + count--, in place —
        // the base pointer never moves, so there is NO local write-back) and
        // simply DROPS the returned value; the removal IS the point. The
        // helper's not-found tail traps (`unreachable`), matching CPython
        // `del d[missing]` raising KeyError. Only the DICT form is in the WASM
        // subset — `del xs[i]` (list-element deletion) is refused (no
        // list-shrink runtime; a fixed-size heap record cannot shrink+shift in
        // place, the PMAT-1033 relocation-hazard posture).
        Stmt::DelItem { name, key, is_dict } => {
            emit_dict_del(name, key, *is_dict, scope, out, depth)
        }
        // PMAT-995 (slice 3b): `s.add(e)` — insert into a set local (a keys-only
        // dict; the `set` helper is shared, with a 0 sentinel value).
        Stmt::SetAdd { set_name, elem } => emit_set_add(set_name, elem, scope, out, depth),
        // PMAT-1240: `s.remove(e)` / `s.discard(e)` — set-element removal in
        // statement position. A set is a keys-only dict, so removal reuses the
        // shared `$__wasm_dict_pop_<k>` swap-last-into-hole helper exactly as
        // `del d[k]` (`emit_dict_del`) does, dropping the popped dummy value.
        Stmt::SetRemove {
            set_name,
            elem,
            error_if_absent,
        } => emit_set_remove(set_name, elem, *error_if_absent, scope, out, depth),
        // PMAT-1236: `d.clear()` / `s.clear()` — reset a dict/set to EMPTY in
        // place. The frontend lowers `.clear()` (dict, set, and list alike) to
        // `Stmt::ListMutate { op: ListMutateOp::Clear }`; over a dict/set the
        // whole runtime cost is zeroing the live-entry COUNT header at `base+0`
        // (the same `+0` count `len(d)` reads), leaving the capacity + stale
        // entry bytes untouched. No relocation (the region only shrinks, so the
        // base-pointer never moves → NO `local.set` write-back), no helper, no
        // trap: a bare `local.get $d ; i32.const 0 ; i32.store`. A later
        // `d[k] = v` re-inserts from count 0, reusing the existing capacity.
        Stmt::ListMutate { list_name, op, .. } => {
            emit_list_mutate(list_name, *op, scope, out, depth)
        }
        // PMAT-1276: `xs.append(v)` — the FIRST list-mutation-that-GROWS. An
        // in-place write at the live-element count (bounded by the capacity
        // header `emit_list_lit` reserved), then `count++`. The record never
        // relocates, so every alias holding the base-pointer observes the
        // append — the alias-safe posture the old PMAT-1033 growth-refusal
        // could not offer. Only literal-bound lists (spare capacity) qualify.
        Stmt::ListAppend { list_name, elem } => {
            emit_list_append(list_name, elem, scope, out, depth)
        }
        // PMAT-1282: `xs.insert(i, v)` — the FIRST list-mutation that both GROWS
        // the count AND SHIFTS the tail. Clamps the index CPython-style, shifts
        // `[slot, n)` right by one slot, writes the value at `slot`, bumps the
        // count. Done in place (the base-pointer never moves), so every alias
        // observes it — like `append`. Only literal-bound lists (spare capacity)
        // qualify; a full record traps (`unreachable`), decided in
        // `emit_list_insert`.
        Stmt::ListInsert {
            list_name,
            index,
            elem,
        } => emit_list_insert(list_name, index, elem, scope, out, depth),
        // PMAT-1285: `xs.remove(v)` — remove the FIRST element EQUAL to `v`. A
        // linear scan for a typed value match (like `index`) fused with a
        // shrink+shift-left (like `del xs[i]`), trapping (`unreachable` = Python
        // ValueError) when the value is absent. Done in place (the base-pointer
        // never moves), so — like `del`/`pop` — it accepts ANY scalar list local
        // (a param included), no growable-list precondition.
        Stmt::ListRemoveValue { list_name, value } => {
            emit_list_remove(list_name, value, scope, out, depth)
        }
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
                // PMAT-1225: `d.pop(k)` used as a bare statement (its value
                // discarded) — emit the pop (which performs the in-place
                // removal, the point of the statement) and drop the i64 value.
                // PMAT-1227: `d.setdefault(k, default)` as a bare statement
                // (`d.setdefault(k, 0)` to ENSURE a key) — same shape: emit the
                // get-or-insert (the insert-if-absent side effect is the point)
                // and drop the i64 value.
                Expr::DictPop { .. } | Expr::DictSetDefault { .. } => {
                    emit_expr(call, scope, out, depth)?;
                    indent(out, depth);
                    writeln!(out, "drop").expect("write");
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

/// PMAT-1187: lower `s.capitalize()` — a materialising op leaving the i32
/// base-pointer of a fresh heap string whose first ASCII letter is upper-cased
/// and every remaining ASCII letter is lower-cased. The receiver is string-valued
/// (`emit_str_expr`, which refuses a non-str recv honestly); the allocating
/// `$__wasm_str_capitalize` helper does the flip and TRAPS on a non-ASCII byte
/// (the honest ASCII-only boundary — never a silent un-folded pass-through). A
/// heap-constructed receiver (`(a + b).capitalize()`) already pulled in the
/// allocator via `expr_has_heap_op`.
fn emit_str_capitalize(
    recv: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_str_capitalize").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1201: lower `s.swapcase()` — a materialising op leaving the i32
/// base-pointer of a fresh heap string with the case of every ASCII letter flipped
/// BOTH ways (`A`–`Z` → `a`–`z` AND `a`–`z` → `A`–`Z`). The receiver is
/// string-valued (`emit_str_expr`, which refuses a non-str recv honestly); the
/// allocating `$__wasm_str_swapcase` helper does the flip and TRAPS on a non-ASCII
/// byte (the honest ASCII-only boundary — full Unicode case flipping needs a case
/// table this scalar lane does not carry, so it refuses at runtime rather than
/// silently returning a wrongly-flipped string). A heap-constructed receiver
/// (`(a + b).swapcase()`) already pulled in the allocator via `expr_has_heap_op`.
fn emit_str_swapcase(
    recv: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_str_swapcase").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1203: lower `s.title()` — a materialising op leaving the i32 base-pointer
/// of a fresh heap string title-cased word-by-word (the first ASCII letter of each
/// word upper-cased, the rest lower-cased, any non-letter a word boundary). The
/// receiver is string-valued (`emit_str_expr`, which refuses a non-str recv
/// honestly); the allocating `$__wasm_str_title` helper does the stateful flip and
/// TRAPS on a non-ASCII byte (the honest ASCII-only boundary — full Unicode title
/// mapping needs a case table this scalar lane does not carry, so it refuses at
/// runtime rather than silently returning a wrongly-cased string). A
/// heap-constructed receiver (`(a + b).title()`) already pulled in the allocator
/// via `expr_has_heap_op`.
fn emit_str_title(
    recv: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_str_title").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1213: lower `s[::-1]` — a materialising op leaving the i32 base-pointer of a
/// fresh heap string with the CODE POINTS of `s` in reverse order. The receiver is
/// string-valued (`emit_str_expr`, which refuses a non-str recv honestly); the
/// allocating `$__wasm_str_reverse` helper copies each UTF-8 code point as an intact
/// unit, so — unlike the case-fold family — it is char-exact for ANY valid UTF-8 with
/// NO trap arm (reversing by code point needs no Unicode table; the lead byte gives
/// each code point's length). A heap-constructed receiver (`(a + b)[::-1]`) already
/// pulled in the allocator via `expr_has_heap_op`.
fn emit_str_reverse(
    recv: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_str_reverse").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1205: lower `s.strip()` (`left`=1, `right`=1) / `s.lstrip()` (`1`,`0`) /
/// `s.rstrip()` (`0`,`1`) — a materialising op leaving the i32 base-pointer of a
/// fresh heap string with the leading/trailing ASCII-whitespace run removed. The
/// receiver is string-valued (`emit_str_expr`, which refuses a non-str recv
/// honestly); the `left` / `right` direction flags are immediate i32 consts pushed
/// after the receiver pointer (like the `$__wasm_str_upper_lower` `up` flag). The
/// allocating `$__wasm_str_strip` helper copies the retained byte range and TRAPS
/// on a non-ASCII BOUNDARY byte (the honest ASCII-only boundary — the whitespace-ness
/// of a non-ASCII byte is undecidable without a Unicode table this lane lacks, so it
/// refuses at runtime rather than silently keeping/dropping the wrong run). A
/// heap-constructed receiver (`(a + b).strip()`) already pulled in the allocator via
/// `expr_has_heap_op`.
fn emit_str_strip(
    recv: &Expr,
    left: bool,
    right: bool,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "i32.const {}", i32::from(left)).expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {}", i32::from(right)).expect("write");
    indent(out, depth);
    writeln!(out, "call $__wasm_str_strip").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1209: lower `s.rjust(w)` (`mode` = 0) / `s.ljust(w)` (`mode` = 1) /
/// `s.center(w)` (`mode` = 2) — a materialising op leaving the i32 base-pointer of
/// a fresh heap string equal to `s` padded with ASCII space to `w` code points. The
/// receiver is string-valued (`emit_str_expr`, which refuses a non-str recv
/// honestly); the width is int-valued (`emit_expr_typed` as `i64`, like zfill); the
/// `mode` selector is an immediate i32 const pushed last. The allocating
/// `$__wasm_str_pad` helper does the split-and-copy and — unlike the case-fold ops —
/// never inspects a payload byte (pad = ASCII space, `s` copied verbatim), so it is
/// char-exact for any UTF-8 with no trap. A heap-constructed receiver
/// (`(a + b).rjust(w)`) already pulled in the allocator via `expr_has_heap_op`.
fn emit_str_pad(
    recv: &Expr,
    width: &Expr,
    mode: i32,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    emit_expr_typed(width, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "i32.const {mode}").expect("write");
    indent(out, depth);
    writeln!(out, "call $__wasm_str_pad").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1219: lower `s.expandtabs()` / `s.expandtabs(tabsize)` — a materialising op
/// leaving the i32 base-pointer of a fresh heap string with each `\t` expanded to
/// spaces to the next multiple of `tabsize` (column in code points, reset on
/// `\n`/`\r`). The receiver is string-valued (`emit_str_expr`, which refuses a
/// non-str recv honestly); the tabsize is int-valued (`emit_expr_typed` as `i64`,
/// like the pad width). The **optional** arg defaults to `8` (`i64.const 8`) — the
/// bare `s.expandtabs()` form. The allocating `$__wasm_str_expandtabs` helper does
/// the two-pass expansion and — like reverse, unlike the case-fold ops — never
/// folds a payload byte (only ASCII tab/newline bytes are interpreted), so it is
/// char-exact for any UTF-8 with no trap. A heap-constructed receiver
/// (`(a + b).expandtabs()`) already pulled in the allocator via `expr_has_heap_op`.
fn emit_str_expandtabs(
    recv: &Expr,
    tabsize: Option<&Expr>,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    match tabsize {
        Some(e) => {
            emit_expr_typed(e, scope, out, depth, WatTy::I64)?;
        }
        None => {
            indent(out, depth);
            writeln!(out, "i64.const 8").expect("write");
        }
    }
    indent(out, depth);
    writeln!(out, "call $__wasm_str_expandtabs").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1197: lower `s.isupper()` (`want_upper` = 1) / `s.islower()`
/// (`want_upper` = 0) — a NON-allocating bool (i32 0/1) predicate leaving the
/// result directly (no result string, no heap). The receiver is string-valued
/// (`emit_str_expr`, which refuses a non-str recv honestly); the `want_upper`
/// direction flag is an immediate i32 const pushed after the receiver pointer,
/// exactly like the `$__wasm_str_upper_lower` case-fold pair. The shared
/// `$__wasm_str_isupper_islower` helper scans the payload bytes: an opposite-case
/// ASCII letter short-circuits to `0`; a non-ASCII byte reached with no
/// opposite-case letter yet TRAPS (the honest ASCII-only boundary — Unicode-cased
/// chars need a case table this lane lacks). A heap-constructed receiver
/// (`(a + b).isupper()`) already pulled in the allocator via `expr_has_heap_op`.
fn emit_str_iscase(
    recv: &Expr,
    want_upper: bool,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    emit_str_expr(recv, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "i32.const {}", i32::from(want_upper)).expect("write");
    indent(out, depth);
    writeln!(out, "call $__wasm_str_isupper_islower").expect("write");
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
        // PMAT-1189: `s.isdigit()` — a bool (i32) predicate: `1` iff `s` is
        // non-empty and every code point is an ASCII decimal digit '0'..'9'. The
        // receiver lowers to an i32 base-pointer (`emit_str_expr`), then the
        // non-allocating `$__wasm_str_isdigit` helper scans the payload bytes.
        // ASCII-only honest boundary: a non-ASCII byte reached with every prior
        // byte a digit TRAPS (Unicode digits need a table this lane lacks), while
        // a definitively non-digit ASCII byte short-circuits to `0` first.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::IsDigit,
            args,
        } if args.is_empty() => {
            emit_str_expr(recv, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_isdigit").expect("write");
            Ok(WatTy::I32)
        }
        // PMAT-1211: `s.isnumeric()` — reuses the `$__wasm_str_isdigit` byte scan.
        // On the ASCII-decidable domain the only numeric characters are `'0'`–`'9'`
        // (all Unicode Nd), so `isnumeric` ≡ `isdigit` over an all-ASCII string;
        // and on the non-ASCII domain (where `isnumeric`'s Nd/Nl/No superset — `"½"`
        // etc. — would decide differently) the scan TRAPS, exactly as it must (this
        // lane has no Unicode table). A leading ASCII non-digit still short-circuits
        // to `0` before any non-ASCII byte (`"a½".isnumeric()` → False), matching
        // Python. So the isdigit helper is byte-exact for `isnumeric` on precisely
        // the inputs it is byte-exact for `isdigit` — no separate helper (cf.
        // `isupper`/`islower` sharing `$__wasm_str_isupper_islower`).
        Expr::StrMethod {
            recv,
            op: StrMethodOp::IsNumeric,
            args,
        } if args.is_empty() => {
            emit_str_expr(recv, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_isdigit").expect("write");
            Ok(WatTy::I32)
        }
        // PMAT-1191: `s.isalpha()` — a bool (i32) predicate: `1` iff `s` is
        // non-empty and every code point is an ASCII letter 'A'..'Z'/'a'..'z'.
        // The receiver lowers to an i32 base-pointer (`emit_str_expr`), then the
        // non-allocating `$__wasm_str_isalpha` helper scans the payload bytes.
        // ASCII-only honest boundary: a non-ASCII byte reached with every prior
        // byte a letter TRAPS (Unicode letters need a table this lane lacks),
        // while a definitively non-letter ASCII byte short-circuits to `0` first.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::IsAlpha,
            args,
        } if args.is_empty() => {
            emit_str_expr(recv, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_isalpha").expect("write");
            Ok(WatTy::I32)
        }
        // PMAT-1193: `s.isspace()` — a bool (i32) predicate: `1` iff `s` is
        // non-empty and every code point is ASCII whitespace (0x09..0x0d or
        // 0x1c..0x20). The receiver lowers to an i32 base-pointer
        // (`emit_str_expr`), then the non-allocating `$__wasm_str_isspace` helper
        // scans the payload bytes. ASCII-only honest boundary: a non-ASCII byte
        // reached with every prior byte whitespace TRAPS (Unicode whitespace needs
        // a table this lane lacks), while a definitively non-whitespace ASCII byte
        // short-circuits to `0` first.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::IsSpace,
            args,
        } if args.is_empty() => {
            emit_str_expr(recv, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_isspace").expect("write");
            Ok(WatTy::I32)
        }
        // PMAT-1195: `s.isalnum()` — a bool (i32) predicate: `1` iff `s` is
        // non-empty and every code point is ASCII alphanumeric (0x30..0x39,
        // 0x41..0x5a, or 0x61..0x7a — the UNION of the isdigit and isalpha
        // ranges). The receiver lowers to an i32 base-pointer (`emit_str_expr`),
        // then the non-allocating `$__wasm_str_isalnum` helper scans the payload
        // bytes. ASCII-only honest boundary: a non-ASCII byte reached with every
        // prior byte alphanumeric TRAPS (Unicode letters/digits need a table this
        // lane lacks), while a definitively non-alphanumeric ASCII byte
        // short-circuits to `0` first.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::IsAlnum,
            args,
        } if args.is_empty() => {
            emit_str_expr(recv, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_isalnum").expect("write");
            Ok(WatTy::I32)
        }
        // PMAT-1197: `s.isupper()` / `s.islower()` — a bool (i32) predicate: `1`
        // iff `s` has at least one ASCII cased letter in the wanted case AND no
        // ASCII cased letter in the opposite case (Python's rule — `"A1".isupper()`
        // is True, `"".isupper()`/`"123".isupper()` are False). The receiver lowers
        // to an i32 base-pointer (`emit_str_expr`), then the non-allocating shared
        // `$__wasm_str_isupper_islower` helper scans the payload bytes with the
        // `want_upper` flag. ASCII-only honest boundary: a non-ASCII byte reached
        // with no opposite-case letter yet TRAPS (Unicode-cased chars need a table
        // this lane lacks), while an opposite-case ASCII letter short-circuits to
        // `0` first.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::IsUpper,
            args,
        } if args.is_empty() => emit_str_iscase(recv, true, scope, out, depth),
        Expr::StrMethod {
            recv,
            op: StrMethodOp::IsLower,
            args,
        } if args.is_empty() => emit_str_iscase(recv, false, scope, out, depth),
        // PMAT-1199: `s.isascii()` — a bool (i32) predicate: `1` iff every payload
        // byte is ASCII (`< 0x80`). The receiver lowers to an i32 base-pointer
        // (`emit_str_expr`), then the non-allocating `$__wasm_str_isascii` helper
        // scans the payload bytes. The ONE predicate in the `is*` family that is
        // FULLY DECIDABLE at the byte level: a byte `>= 0x80` is the definitive
        // `0` (never a trap — no `unreachable` arm) and the empty string is `1`
        // (Python `"".isascii()` is True — no empty guard), so it is byte-exact
        // against CPython for every input, including non-ASCII (→ False).
        Expr::StrMethod {
            recv,
            op: StrMethodOp::IsAscii,
            args,
        } if args.is_empty() => {
            emit_str_expr(recv, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_str_isascii").expect("write");
            Ok(WatTy::I32)
        }
        Expr::StrMethod { op, .. } => Err(unsupported(&format!(
            "string method {op:?} on the WASM lane — only `len(s)` (CharCount), \
             `.startswith(p)`, `.endswith(p)`, `.count(p)`, `.find(p)`, \
             `.find(p, start)`, `.rfind(p)`, `.rfind(p, start)`, `.index(p)`, \
             `.rindex(p)`, `.removeprefix(p)`, `.removesuffix(p)`, \
             `.replace(old, new)`, `.replace(old, new, count)`, `.isdigit()`, \
             `.isnumeric()`, `.isalpha()`, `.isspace()`, `.isalnum()`, \
             `.isupper()`, `.islower()`, and `.isascii()` are supported; the other \
             is* predicates (istitle/isdecimal/…), upper/lower/strip/split/…, the \
             3-arg `.find`/`.rfind`(p, start, end), and the start/end forms of \
             index/rindex/count are refused"
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
        // PMAT-1223: `d.get(k, default)` — a TOTAL dict read; `if has(p,k) then
        // get(p,k) else default`, so an absent key yields the int `default`
        // instead of trapping (the non-trapping sibling of `d[k]`).
        Expr::DictGetOr { dict, key, default } => {
            emit_dict_get_or(dict, key, default, scope, out, depth)
        }
        // PMAT-1225: `d.pop(k)` / `d.pop(k, default)` — a dict read that ALSO
        // removes the entry. The bare form TRAPS on an absent key (KeyError);
        // the 2-arg form is total (absent → the int `default`, no mutation).
        Expr::DictPop { dict, key, default } => {
            emit_dict_pop(dict, key, default.as_deref(), scope, out, depth)
        }
        // PMAT-1227: `d.setdefault(k, default)` — a get-or-INSERT: on a HIT read
        // the existing value (no mutation); on a MISS insert `default` under `k`
        // (which may grow+relocate the dict) then read it back. Both cases
        // evaluate to `d[k]` — the pre-existing value on a hit, the just-inserted
        // `default` on a miss — exactly CPython's `dict.setdefault`.
        Expr::DictSetDefault { dict, key, default } => {
            emit_dict_set_default(dict, key, default, scope, out, depth)
        }
        // PMAT-995 (slice 3b): `k in d` / `x in s` — i32 bool membership.
        Expr::DictContains { dict, key } => emit_dict_contains(dict, key, scope, out, depth),
        Expr::SetContains { set, elem } => emit_dict_contains(set, elem, scope, out, depth),
        // PMAT-1262: `x in xs` / `x not in xs` over a `list[int]` / `list[float]`
        // — an i32 (0/1) membership test via a non-allocating linear scan. The
        // list NAME lowers to its i32 base-pointer, the needle to the element
        // kind, then `$__wasm_list_contains_{i64,f64}` scans for a match. `not in`
        // arrives as this node under a `UnOp::Not` (the scalar subset lowers that
        // over the i32 result). Refuses a non-name list / non-scalar element
        // honestly (see `emit_list_contains`).
        Expr::ListContains { list, elem } => emit_list_contains(list, elem, scope, out, depth),
        // PMAT-1274: `xs.count(v)` / `xs.index(v)` over a `list[int]`/`list[float]`
        // — the FIRST list-QUERY op the WASM lane lowers, an i64-VALUED
        // (count/index) non-allocating linear scan mirroring `ListContains` but
        // returning the count / first index instead of an i32 bool. The list NAME
        // lowers to its i32 base-pointer, the needle to the element kind, then
        // `$__wasm_list_{count,index}_{i64,f64}` scans. `index` traps on a miss
        // (Python `ValueError`). Refuses a non-name list / non-scalar element
        // honestly (see `emit_list_query`).
        Expr::ListQuery { list, op, arg } => emit_list_query(list, *op, arg, scope, out, depth),
        // PMAT-1278: `xs.pop()` over a `list[int]` / `list[float]` / `list[bool]`
        // — the FIRST list-mutation-that-SHRINKS the WASM lane lowers (`append`
        // GROWS, `xs[i]=v` writes in place; this REMOVES the last element and
        // evaluates to it). An EXPRESSION: guard-empty (`unreachable` trap =
        // Python `IndexError`), load the last element (the result, left on the
        // stack), then decrement the i32 count header at base+0 in place — so
        // every later `len(xs)` / `xs[i]` / `for x in xs` sees the shrink. NO
        // relocation (the base-pointer never moves), so it is alias-safe on ANY
        // named scalar list (param / literal-bound / helper-allocated-then-named),
        // unlike `append` which needs the growth-slack of a literal binding. NO
        // helper + no nested gated sub-expr (the only child is the receiver name),
        // so no gate-walker touch. The INDEXED form `xs.pop(i)` (element-shifting
        // removal) is refused honestly (see `emit_list_pop`).
        Expr::ListPop { list, index } => emit_list_pop(list, index.as_deref(), scope, out, depth),
        // PMAT-1245: set ordering `a <= b` / `a < b` / `a >= b` / `a > b` (subset
        // / proper-subset / superset / proper-superset). The FRONTEND lowers set
        // comparison operators to `SetPred`, NOT `BinOp` — so this is the path
        // that makes set ordering reachable from Python source; it reuses the
        // `$__wasm_set_subset_<k>` membership helper PMAT-1244 already emits.
        // `a.isdisjoint(b)` (`SetPredOp::Disjoint`) routes to its own DUAL helper
        // `$__wasm_set_disjoint_<k>` (return 0 on any shared key) — PMAT-1246.
        Expr::SetPred { lhs, op, rhs } => emit_set_pred(lhs, *op, rhs, scope, out, depth),
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
        // PMAT-1248: `sum(xs)` over a `list[int]` — an i64 reduction. The list
        // NAME lowers to its i32 base-pointer, then `$__wasm_list_sum_i64` folds
        // the payload left-to-right. Refuses the list[float] form, an explicit
        // `start`, and a non-name list honestly (see `emit_list_sum`).
        Expr::Sum {
            list,
            of_float,
            start,
        } => emit_list_sum(list, *of_float, start.as_deref(), scope, out, depth),
        // PMAT-1250: `min(xs)` / `max(xs)` over a `list[int]` / `list[float]` — an
        // i64/f64 reduction. The list NAME lowers to its i32 base-pointer, then
        // `$__wasm_list_minmax_{i64,f64}` folds the payload, `is_max` selecting the
        // direction. Refuses a `key=`, a `default=`, a struct-cmp element, and a
        // non-name list honestly (see `emit_list_minmax`).
        Expr::ListMinMax {
            list,
            is_max,
            of_float,
            of_struct_cmp,
            key,
            default,
        } => emit_list_minmax(
            list,
            *is_max,
            *of_float,
            *of_struct_cmp,
            key.as_ref(),
            default.as_deref(),
            scope,
            out,
            depth,
        ),
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
        // PMAT-1251: `any(xs)` / `all(xs)` over a `list[bool]` — an i32 (0/1)
        // boolean fold. The list NAME lowers to its i32 base-pointer, then
        // `$__wasm_list_bool_reduce` folds the payload with `is_all` selecting the
        // direction. Refuses the short-circuiting GENERATOR form and a non-name /
        // non-bool list honestly (see `emit_bool_reduce`).
        Expr::BoolReduce {
            list,
            is_all,
            short_circuit,
        } => emit_bool_reduce(list, *is_all, *short_circuit, scope, out, depth),
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
    if let Some(elem) = scope.list_elem_of(name) {
        // Emit the bounds-checked element address onto the stack, then read the
        // element at it with the element's natural `*.load`.
        emit_list_elem_addr(name, elem, index, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "{}", elem.load_instr()).expect("write");
        return Ok(elem);
    }
    // PMAT-1290: `for x in s` over a set lowers (in `desugar_foreach_stmts`) to a
    // `while` loop whose per-element read is `s[i]` — an `Expr::Index` on a set
    // NAME. Read entry `i`'s KEY from the 16-byte-stride entry array (see
    // [`emit_set_elem_read`]). Gated on the index being the synthetic foreach
    // counter: a user-written `s[i]` (a Python set is NOT subscriptable —
    // `TypeError`) stays refused, so this is strictly the iteration lowering.
    if let Some(elem) = scope.set_elem_of(name) {
        if !is_foreach_counter(index) {
            return Err(unsupported(&format!(
                "subscripting the set `{name}` — a Python set is not \
                 subscriptable (`TypeError`); set element access exists only as \
                 the internal per-element read of `for x in {name}`"
            )));
        }
        emit_set_elem_read(name, elem, index, scope, out, depth)?;
        return Ok(elem);
    }
    Err(unsupported(&format!(
        "index over `{name}` which is not a `list[scalar]` param/local or a \
         `set[int|str]` local — only a list (i32 base-pointer into linear \
         memory) or a set (iterated via `for x in s`) can be indexed in the \
         WASM subset (no str/dict/tuple indexing)"
    )))
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
    // call. PMAT-1289: a READ-side NEGATIVE-LITERAL `xs[-k]` arrives
    // pre-rewritten to `len(xs) - k` (PMAT-570, for the Rust lane) — recover
    // the raw `-k` so the PMAT-1001 normalise below applies ONCE (passing the
    // rewritten value through double-normalised: `xs[-2]` on a 1-element list
    // silently read slot 0 where CPython raises `IndexError` — found by the
    // PMAT-1289 probe sweep; the store side was never folded, and a raw index
    // is emitted unchanged).
    if let Some(k) = neg_literal_index_k(index, name) {
        indent(out, depth);
        writeln!(out, "i64.const {}", -k).expect("write");
    } else {
        emit_expr_typed(index, scope, out, depth, WatTy::I64)?;
    }
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

/// PMAT-1290: read the KEY of set entry `index`, leaving it (typed `elem`) on
/// the stack — the per-element read the `for x in s` desugar
/// ([`desugar_foreach_stmts`]) emits as an [`Expr::Index`] over a set NAME.
///
/// A set entry is a [`DICT_ENTRY_SIZE`] (16) byte record with the key at entry
/// offset 0 (the value half is unused for a set), so entry `i`'s key address is
/// `base + LIST_ELEMS_OFFSET + i * DICT_ENTRY_SIZE` — a **16-byte stride**, NOT
/// the 8-byte packed-slot stride a `list[scalar]` uses ([`emit_list_elem_addr`]).
/// The key loads as the set's element WAT type: `i64` for an int set (the stored
/// key), `i32` for a str set (the stored str base-pointer, so the loop var
/// behaves as a str local downstream).
///
/// Real Python never subscripts a set, so the ONLY producer is the desugar,
/// whose index is the non-negative loop counter `0..len(s)`; a defensive
/// `i < 0 || i >= count` guard preserves the `list`-`IndexError` fail-loud
/// posture anyway (`count` is the live-entry i32 header at `base+0`, shared with
/// the list/str/dict layout).
///
/// Iteration walks the LIVE-entry region `0..count` in STORAGE order. A
/// `discard`/`remove` swaps the last entry into the hole, so this is NOT
/// CPython's hash-order iteration — but a set has no defined order, and every
/// witness reduces the elements COMMUTATIVELY (sum / count / max / membership),
/// for which storage order is irrelevant: both sides agree on the multiset, and
/// that is all a commutative fold observes. An order-DEPENDENT observation of a
/// set (e.g. building a list of the iteration sequence) is NOT emitted here — it
/// would diverge from CPython and is refused upstream.
fn emit_set_elem_read(
    name: &str,
    elem: WatTy,
    index: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // Evaluate the index once into the per-function scratch i64 `$__wasm_idx`
    // (declared body-driven, like the list path).
    emit_expr_typed(index, scope, out, depth, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "local.set ${IDX_SCRATCH}").expect("write");

    // Defensive bounds guard (the Python IndexError analogue):
    //   if (i < 0) | (i >= count) { unreachable }
    // `count` is the live-entry i32 header at base+0, zero-extended to i64.
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i64.const 0").expect("write");
    indent(out, depth);
    writeln!(out, "i64.lt_s").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load").expect("write"); // live-entry count (header @ base+0)
    indent(out, depth);
    writeln!(out, "i64.extend_i32_u").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${IDX_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i64.le_s").expect("write"); // count <= i  ⇔  i >= count
    indent(out, depth);
    writeln!(out, "i32.or").expect("write");
    indent(out, depth);
    writeln!(out, "if").expect("write");
    indent(out, depth + 1);
    writeln!(out, "unreachable").expect("write");
    indent(out, depth);
    writeln!(out, "end").expect("write");

    // addr = base + LIST_ELEMS_OFFSET + (index as i32) * DICT_ENTRY_SIZE
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
    writeln!(out, "i32.const {DICT_ENTRY_SIZE}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.mul").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");

    // Load entry `i`'s key with its natural width (i64 int / i32 str-pointer).
    indent(out, depth);
    writeln!(out, "{}", elem.load_instr()).expect("write");
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

/// PMAT-1248: emit `sum(xs)` over a `list[int]` — leaves the i64 total on the
/// stack. The list NAME lowers to its i32 base-pointer (`local.get $name`, the
/// PMAT-968 list ABI: a length-prefixed region with the i32 count @ base+0 and
/// packed i64 elements @ base+8), then `$__wasm_list_sum_i64` folds the payload
/// left-to-right — matching CPython's left-to-right `sum` reduction and the
/// rust `.iter().sum::<i64>()` lane. The empty list sums to 0 (Python
/// `sum([]) == 0`), computed inside the helper.
///
/// PMAT-1249 extends this to `list[float]` (`of_float: true`) via the twin f64
/// helper — the accumulator, `*.load`, and `*.add` opcodes are the only
/// difference (a float list shares the int list's header + 8-byte stride). The
/// frontend already tags `of_float` from the argument's element type, so the
/// two forms never cross wires.
///
/// Honest scope (each a hard [`BackendError`], never a silent miscompile):
///   * an explicit `start` (`sum(xs, start)`) is refused — only the 1-arg form
///     (start defaults to 0/0.0) is emitted; a general `start` expression would
///     also need every gate walker to recurse into it (the serial-hotspot toil),
///     so it is deferred rather than half-wired.
///   * a non-name list (a list LITERAL / temporary — `sum([1, 2, 3])`) is
///     refused; bind it to a name first. A name whose element type does not
///     match the summed kind — a `list[float]` under `of_float: false`, an int
///     or `str`/dict/set/scalar list under `of_float: true` — is refused by the
///     element-type check against [`Scope::list_elem_of`].
fn emit_list_sum(
    list: &Expr,
    of_float: bool,
    start: Option<&Expr>,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    if start.is_some() {
        return Err(unsupported(
            "sum(xs, start) with an explicit start — the WASM subset emits the \
             1-arg `sum(xs)` only (start defaults to 0); the start form is \
             deferred (refused honestly)",
        ));
    }
    // The summed element kind, its reduction helper, and the WAT result type.
    let (want_elem, helper, result) = if of_float {
        (WatTy::F64, "$__wasm_list_sum_f64", WatTy::F64)
    } else {
        (WatTy::I64, "$__wasm_list_sum_i64", WatTy::I64)
    };
    let Expr::Ident(name) = list else {
        return Err(unsupported(&format!(
            "sum() of a non-name list — the WASM subset sums a `list[{}]` NAME \
             (an i32 base-pointer into linear memory); a list literal / temporary \
             is refused (bind it to a name first)",
            want_elem.keyword()
        )));
    };
    match scope.list_elem_of(name) {
        Some(elem) if elem == want_elem => {}
        Some(other) => {
            return Err(unsupported(&format!(
                "sum() over `{name}` whose elements load as {} — this `sum` reduces \
                 a `list[{}]` ({} elements); the {} form is emitted separately",
                other.keyword(),
                want_elem.keyword(),
                want_elem.keyword(),
                other.keyword(),
            )));
        }
        None => {
            return Err(unsupported(&format!(
                "sum() over `{name}` which is not a `list[{}]` param/local — only \
                 a list (an i32 base-pointer into linear memory) can be summed in \
                 the WASM subset",
                want_elem.keyword()
            )));
        }
    }
    // Push the list base-pointer and fold the payload via the reduction helper.
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "call {helper}").expect("write");
    Ok(result)
}

/// PMAT-1250: emit `min(xs)` / `max(xs)` over a `list[int]` / `list[float]` —
/// leaves the i64/f64 extremum on the stack. The list NAME lowers to its i32
/// base-pointer (the PMAT-968 list ABI: i32 count @ base+0, packed 8-byte
/// elements @ base+8), then `$__wasm_list_minmax_{i64,f64}` folds the payload,
/// with `is_max` pushed as an i32 immediate (`1` for `max`, `0` for `min`) so
/// one helper per element kind serves both directions. CPython semantics: the
/// first extremal element wins ties (a strict compare in the helper), and an
/// EMPTY list traps (Python raises `ValueError`; the Rust `.unwrap()` lane
/// panics) — computed inside the helper.
///
/// Honest scope (each a hard [`BackendError`], never a silent miscompile):
///   * a `key=lambda …` (`min(xs, key=f)`) is refused — the WASM subset reduces
///     by the ELEMENT only; a key would need to lower an arbitrary lambda body
///     per element (deferred, not half-wired).
///   * a `default=` (`min(xs, default=d)`) is refused — the empty case traps
///     rather than yielding a fallback; wiring a default would also require the
///     gate walkers to recurse into it (already done defensively) plus a runtime
///     empty-branch, deferred.
///   * a struct-comparison element (`of_struct_cmp`) is refused — a struct list
///     has no native i64/f64 payload to fold; only scalar `list[int]`/`list[float]`.
///   * a non-name list (a list LITERAL / temporary — `max([1, 2, 3])`, or the
///     variadic `max(a, b)` which lowers to a `ListLit`) is refused; bind it to a
///     name first. A name whose element type does not match the reduced kind is
///     refused by the element-type check against [`Scope::list_elem_of`].
#[allow(clippy::too_many_arguments)]
fn emit_list_minmax(
    list: &Expr,
    is_max: bool,
    of_float: bool,
    of_struct_cmp: bool,
    key: Option<&SortKey>,
    default: Option<&Expr>,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    if key.is_some() {
        return Err(unsupported(
            "min(xs, key=…) / max(xs, key=…) — the WASM subset reduces a scalar \
             list by its ELEMENT only; a `key=` lambda is deferred (refused honestly)",
        ));
    }
    if default.is_some() {
        return Err(unsupported(
            "min(xs, default=…) / max(xs, default=…) — the WASM subset traps on an \
             empty list (Python `ValueError`); a `default=` fallback is deferred \
             (refused honestly)",
        ));
    }
    if of_struct_cmp {
        return Err(unsupported(
            "min/max over a struct list with a custom `__lt__` — the WASM subset \
             reduces only a scalar `list[int]` / `list[float]` (an i64/f64 payload); \
             a struct element has no native payload to fold (refused honestly)",
        ));
    }
    // The reduced element kind, its reduction helper, and the WAT result type.
    let (want_elem, helper, result) = if of_float {
        (WatTy::F64, "$__wasm_list_minmax_f64", WatTy::F64)
    } else {
        (WatTy::I64, "$__wasm_list_minmax_i64", WatTy::I64)
    };
    let op = if is_max { "max" } else { "min" };
    let Expr::Ident(name) = list else {
        return Err(unsupported(&format!(
            "{op}() of a non-name list — the WASM subset reduces a `list[{}]` NAME \
             (an i32 base-pointer into linear memory); a list literal / temporary \
             (incl. the variadic `{op}(a, b)` form) is refused (bind it to a name first)",
            want_elem.keyword()
        )));
    };
    match scope.list_elem_of(name) {
        Some(elem) if elem == want_elem => {}
        Some(other) => {
            return Err(unsupported(&format!(
                "{op}() over `{name}` whose elements load as {} — this `{op}` reduces \
                 a `list[{}]`; the {} form is emitted separately",
                other.keyword(),
                want_elem.keyword(),
                other.keyword(),
            )));
        }
        None => {
            return Err(unsupported(&format!(
                "{op}() over `{name}` which is not a `list[{}]` param/local — only a \
                 list (an i32 base-pointer into linear memory) can be reduced in the \
                 WASM subset",
                want_elem.keyword()
            )));
        }
    }
    // Push the list base-pointer + the is_max selector, then fold via the helper.
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {}", i32::from(is_max)).expect("write");
    indent(out, depth);
    writeln!(out, "call {helper}").expect("write");
    Ok(result)
}

/// PMAT-1262: emit `x in xs` (`Expr::ListContains`) over a `list[int]` /
/// `list[float]` — leaves the i32 (0/1) membership result on the stack. The list
/// NAME lowers to its i32 base-pointer (the PMAT-968 list ABI), the `needle`
/// lowers TYPED to the element WAT type (so a mismatched-kind needle is an honest
/// error at the typed site), then `$__wasm_list_contains_{i64,f64}` linearly
/// scans for a match. `x not in xs` reaches here as this same node wrapped in a
/// frontend `UnOp::Not` — handled by the scalar subset over the i32 result, so no
/// separate path is needed.
///
/// Honest scope (each a hard [`BackendError`], never a silent miscompile):
///   * a NON-NAME list (a list LITERAL / temporary — `x in [1, 2, 3]`, `x in
///     sorted(ys)`) is refused; the lane needs a declared `list[scalar]` NAME
///     (an i32 base-pointer) to recover the element type. Bind it to a name first.
///   * a name that is not a `list[int]` / `list[float]` — whose elements do not
///     load as i64/f64 — is refused by [`Scope::list_elem_of`]. A `list[str]`
///     membership (`s in words`) needs a per-element string compare (deferred);
///     a `list[bool]` needle would need i32 elements (a distinct helper). Only the
///     i64/f64 scalar element kinds are lowered here.
fn emit_list_contains(
    list: &Expr,
    elem: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let Expr::Ident(name) = list else {
        return Err(unsupported(
            "`x in xs` over a non-name list — the WASM subset tests membership in a \
             `list[int]` / `list[float]` NAME (an i32 base-pointer into linear \
             memory); a list literal / temporary (`x in [1, 2, 3]`, `x in \
             sorted(ys)`) is refused (bind it to a name first)",
        ));
    };
    // The element kind selects the membership helper and the needle's WAT type.
    let (helper, want_elem) = match scope.list_elem_of(name) {
        Some(WatTy::I64) => ("$__wasm_list_contains_i64", WatTy::I64),
        Some(WatTy::F64) => ("$__wasm_list_contains_f64", WatTy::F64),
        Some(other) => {
            return Err(unsupported(&format!(
                "`x in {name}` whose elements load as {} — the WASM subset tests \
                 membership only in a `list[int]` / `list[float]` (an i64/f64 \
                 payload compared with `eq`); this element kind is refused",
                other.keyword()
            )));
        }
        None => {
            return Err(unsupported(&format!(
                "`x in {name}` which is not a `list[int]` / `list[float]` param/local \
                 — only a scalar list (an i32 base-pointer into linear memory) can be \
                 membership-tested in the WASM subset"
            )));
        }
    };
    // Push the list base-pointer, then the needle typed to the element kind
    // (an honest type mismatch if the needle does not lower to that kind), then
    // scan via the helper (i32 0/1 result).
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    emit_expr_typed(elem, scope, out, depth, want_elem)?;
    indent(out, depth);
    writeln!(out, "call {helper}").expect("write");
    Ok(WatTy::I32)
}

/// PMAT-1274: emit `xs.count(v)` / `xs.index(v)` (`Expr::ListQuery`) over a
/// `list[int]` / `list[float]` — leaves the i64 result (the match count, or the
/// first matching index) on the stack. The list NAME lowers to its i32
/// base-pointer (the PMAT-968 list ABI), the `arg` needle lowers TYPED to the
/// element WAT type (so a mismatched-kind needle is an honest error at the typed
/// site), then `$__wasm_list_{count,index}_{i64,f64}` linearly scans. `count`
/// inspects every element and returns the total; `index` returns the FIRST
/// matching position and TRAPS on a miss (Python `ValueError`). Both are
/// non-allocating reads (like `contains`), so each rides its OWN gate
/// (`needs_list_count` / `needs_list_index`), NOT `needs_heap`.
///
/// Honest scope (each a hard [`BackendError`], never a silent miscompile) —
/// identical to `emit_list_contains`:
///   * a NON-NAME list (a list LITERAL / temporary — `[1, 2, 3].count(x)`,
///     `sorted(ys).index(x)`) is refused; the lane needs a declared
///     `list[scalar]` NAME (an i32 base-pointer) to recover the element type.
///   * a name that is not a `list[int]` / `list[float]` — whose elements do not
///     load as i64/f64 — is refused by [`Scope::list_elem_of`]. A `list[str]`
///     query needs a per-element string compare (deferred); a `list[bool]` would
///     need i32 elements (a distinct helper). Only the i64/f64 scalar element
///     kinds are lowered here.
fn emit_list_query(
    list: &Expr,
    op: ListQueryOp,
    arg: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let method = match op {
        ListQueryOp::Count => "count",
        ListQueryOp::Index => "index",
    };
    let Expr::Ident(name) = list else {
        return Err(unsupported(&format!(
            "`.{method}(…)` on a non-name list — the WASM subset queries a \
             `list[int]` / `list[float]` NAME (an i32 base-pointer into linear \
             memory); a list literal / temporary (`[1, 2, 3].{method}(x)`, \
             `sorted(ys).{method}(x)`) is refused (bind it to a name first)"
        )));
    };
    // The element kind selects the query helper and the needle's WAT type.
    let (helper, want_elem) = match (op, scope.list_elem_of(name)) {
        (ListQueryOp::Count, Some(WatTy::I64)) => ("$__wasm_list_count_i64", WatTy::I64),
        (ListQueryOp::Count, Some(WatTy::F64)) => ("$__wasm_list_count_f64", WatTy::F64),
        (ListQueryOp::Index, Some(WatTy::I64)) => ("$__wasm_list_index_i64", WatTy::I64),
        (ListQueryOp::Index, Some(WatTy::F64)) => ("$__wasm_list_index_f64", WatTy::F64),
        (_, Some(other)) => {
            return Err(unsupported(&format!(
                "`{name}.{method}(…)` whose elements load as {} — the WASM subset \
                 queries only a `list[int]` / `list[float]` (an i64/f64 payload \
                 compared with `eq`); this element kind is refused",
                other.keyword()
            )));
        }
        (_, None) => {
            return Err(unsupported(&format!(
                "`{name}.{method}(…)` where `{name}` is not a `list[int]` / \
                 `list[float]` param/local — only a scalar list (an i32 \
                 base-pointer into linear memory) can be queried in the WASM subset"
            )));
        }
    };
    // Push the list base-pointer, then the needle typed to the element kind (an
    // honest type mismatch if it does not lower to that kind), then scan via the
    // helper (i64 count / index result; `index` traps on a miss).
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    emit_expr_typed(arg, scope, out, depth, want_elem)?;
    indent(out, depth);
    writeln!(out, "call {helper}").expect("write");
    Ok(WatTy::I64)
}

/// PMAT-1278: emit `xs.pop()` (`Expr::ListPop` with no index) over a
/// `list[int]` / `list[float]` / `list[bool]` — the FIRST list-mutation-that-
/// SHRINKS the WASM lane lowers. Leaves the removed LAST element on the stack
/// (the expression value) and decrements the i32 count header in place.
///
/// The whole op is inline WAT (no `$__wasm_list_pop_*` helper, unlike
/// `contains`/`count`/`index`): guard-empty, load-last, decrement-count — a
/// handful of instructions referencing only the receiver local. Because the
/// base-pointer NEVER moves (only the count header shrinks), pop is alias-safe
/// on ANY named scalar list — a param (caller-sized), a literal-bound local, or
/// a `sorted`/`reversed`/`concat`/slice result bound to a name (each carries a
/// valid count header at base+0). This is STRICTLY more general than
/// `emit_list_append`, which needs the spare capacity only a literal binding
/// reserves; a shrink reads a header it can only make SMALLER, so no capacity /
/// relocation hazard exists.
///
/// Stack shape: the loaded element (the result) is pushed FIRST, then the
/// `count = count - 1` write-back pushes and fully consumes its own
/// `(addr, value)` on TOP of it — so the op nets exactly one value (the result)
/// on the stack, the element's WAT type.
///
/// Semantics: an empty-list pop TRAPS (`unreachable`) exactly where CPython
/// raises `IndexError` — the same posture as the out-of-range `xs[i]` trap.
///
/// PMAT-1289: the INDEXED form `xs.pop(i)` (previously refused here) lowers via
/// the typed `$__wasm_list_pop_idx_{i64,f64}(base, idx)` helper pair
/// ([`LIST_POP_INDEX_INT_HELPER`] / [`LIST_POP_INDEX_FLOAT_HELPER`]) — the
/// value-RETURNING sibling of `del xs[i]`'s `$__wasm_list_delitem`: the same
/// CPython index normalise (negative `+= n`) + `IndexError` trap (out of
/// `[0, n)` after normalising, empty list included) + low→high left shift +
/// count drop, plus a typed load of the removed element BEFORE the shift,
/// returned as the expression value. The shift moves 8-byte words, so the
/// indexed form serves `list[int]` / `list[float]` only — a `list[bool]`
/// (4-byte i32 stride) refuses (an i32-stride twin is deferred, exactly like
/// `insert`/`del`/`remove`; note the INLINE no-index pop DOES take bool — it
/// never shifts). Like every shrink, no growable-list precondition: params,
/// literal bindings, and helper-results all qualify.
///
/// Honest scope (each a hard [`BackendError`], never a silent miscompile):
///   * a NON-NAME receiver (a list LITERAL / temporary — `[1, 2, 3].pop()`,
///     `sorted(ys).pop()`) is refused; the lane needs a declared `list[scalar]`
///     NAME (an i32 base-pointer whose count header it can decrement).
///   * a name that is not a scalar list (`list_elem_of` → `None`, e.g. a
///     `list[str]` whose elements are not a fixed-width scalar) is refused.
///   * `xs.pop(i)` over a `list[bool]` is refused (i32 stride vs the 8-byte
///     word shift; the no-index form still accepts bool).
fn emit_list_pop(
    list: &Expr,
    index: Option<&Expr>,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    if let Some(index_expr) = index {
        let Expr::Ident(name) = list else {
            return Err(unsupported(
                "`.pop(i)` on a non-name list — the WASM subset pops from a \
                 `list[int]` / `list[float]` NAME (an i32 base-pointer into linear \
                 memory whose payload it shifts and whose count header it \
                 decrements); a list literal / temporary (`[1, 2, 3].pop(0)`, \
                 `sorted(ys).pop(0)`) is refused (bind it to a name first)",
            ));
        };
        // The indexed pop shifts whole 8-byte words, so only the i64/f64 element
        // kinds qualify (the typed helper pair also loads/returns the removed
        // element at that width). A `list[bool]` (i32 stride) refuses — unlike
        // the INLINE no-index pop below, which never shifts.
        let helper = match scope.list_elem_of(name) {
            Some(WatTy::I64) => "$__wasm_list_pop_idx_i64",
            Some(WatTy::F64) => "$__wasm_list_pop_idx_f64",
            Some(other) => {
                return Err(unsupported(&format!(
                    "`{name}.pop(i)` whose elements load as {} — the WASM subset's \
                     INDEXED pop shifts 8-byte words, so it serves only a \
                     `list[int]` / `list[float]` (a `list[bool]` would need an \
                     i32-stride shift twin, deferred like `insert`/`del`; the \
                     no-index `{name}.pop()` does accept it)",
                    other.keyword()
                )));
            }
            None => {
                return Err(unsupported(&format!(
                    "`{name}.pop(i)` where `{name}` is not a `list[int]` / \
                     `list[float]` param/local — only a scalar list (an i32 \
                     base-pointer into linear memory) can be popped by index in \
                     the WASM subset"
                )));
            }
        };
        // base (i32) ; RAW index typed to i64 ; call. The helper applies THE
        // one CPython normalise (negative `+= n`, then the `[0, n)` bounds
        // trap), so the frontend's two PRE-normalised index shapes must be
        // UNWRAPPED back to the raw index first — emitting them as-is would
        // DOUBLE-normalise (a pre-normalised value landing in `[-n, 0)` gets
        // `n` re-added, silently popping where CPython raises `IndexError`;
        // the caught corner: `[5].pop(-2)` → frontend `len - 2` → runtime
        // `-1` → re-add → pops slot 0 instead of trapping).
        indent(out, depth);
        writeln!(out, "local.get ${name}").expect("write");
        if let Some(raw) = unwrap_pop_index_normalize(index_expr) {
            // Shape 3: the PMAT-609 runtime-index normalize Block — emit the
            // RAW inner index; the helper re-applies the identical normalise.
            emit_expr_typed(raw, scope, out, depth, WatTy::I64)?;
        } else if let Some(k) = neg_literal_index_k(index_expr, name) {
            // Shape 2: the PMAT-570 negative-literal rewrite `len(xs) - k`
            // (from `xs.pop(-k)`) — recover the raw `-k` so the helper's
            // normalise is applied ONCE (a user-written `xs.pop(len(xs) - k)`
            // is HIR-identical; on an underflow it traps here exactly where
            // the Rust lane's `(len - k) as usize` panics — the safe,
            // cross-backend-consistent posture for that corner).
            indent(out, depth);
            writeln!(out, "i64.const {}", -k).expect("write");
        } else if matches!(index_expr, Expr::Block(_)) {
            // A Block that is NOT the known normalize shape (a frontend
            // change would land here) — refuse rather than guess.
            return Err(unsupported(&format!(
                "`{name}.pop(i)` whose index lowered to an unrecognised Block \
                 shape — the WASM subset unwraps only the frontend's \
                 `__pidx` normalize wrap (PMAT-609); this shape is refused \
                 rather than risking a double-normalised index"
            )));
        } else {
            // Shape 1: a bare (non-negative) literal, or any raw expression —
            // the helper's normalise + bounds trap IS the CPython semantics.
            emit_expr_typed(index_expr, scope, out, depth, WatTy::I64)?;
        }
        indent(out, depth);
        writeln!(out, "call {helper}").expect("write");
        return Ok(if helper.ends_with("f64") {
            WatTy::F64
        } else {
            WatTy::I64
        });
    }
    let Expr::Ident(name) = list else {
        return Err(unsupported(
            "`.pop()` on a non-name list — the WASM subset pops from a \
             `list[int]` / `list[float]` / `list[bool]` NAME (an i32 base-pointer \
             into linear memory whose count header it decrements); a list literal \
             / temporary (`[1, 2, 3].pop()`, `sorted(ys).pop()`) is refused (bind \
             it to a name first)",
        ));
    };
    let Some(elem) = scope.list_elem_of(name) else {
        return Err(unsupported(&format!(
            "`{name}.pop()` where `{name}` is not a `list[int]` / `list[float]` / \
             `list[bool]` param/local — only a scalar list (an i32 base-pointer \
             into linear memory) can be popped in the WASM subset"
        )));
    };
    let stride = elem.byte_size();
    // Guard: if count(base+0) == 0 → trap (Python IndexError on empty pop).
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load ;; count @ base+0").expect("write");
    indent(out, depth);
    writeln!(out, "i32.eqz").expect("write");
    indent(out, depth);
    writeln!(out, "if").expect("write");
    indent(out, depth + 1);
    writeln!(out, "unreachable ;; pop from empty list (IndexError)").expect("write");
    indent(out, depth);
    writeln!(out, "end").expect("write");
    // Load the last element (the result) — addr = base + (count-1)*stride, read
    // at offset=LIST_ELEMS_OFFSET. Left on the stack as the expression value.
    indent(out, depth);
    writeln!(out, "local.get ${name} ;; base (last-elem addr)").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load ;; count").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const 1").expect("write");
    indent(out, depth);
    writeln!(out, "i32.sub ;; count-1").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {stride}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.mul").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add ;; base + (count-1)*stride").expect("write");
    indent(out, depth);
    writeln!(
        out,
        "{} offset={LIST_ELEMS_OFFSET} ;; -> removed element (result)",
        elem.load_instr()
    )
    .expect("write");
    // Decrement the count header in place: base+0 = count - 1. Pushes and fully
    // consumes (addr, value) ON TOP of the result, leaving the result on stack.
    indent(out, depth);
    writeln!(out, "local.get ${name} ;; base (count store addr)").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load ;; count").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const 1").expect("write");
    indent(out, depth);
    writeln!(out, "i32.sub").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store ;; count = count - 1").expect("write");
    Ok(elem)
}

/// PMAT-1251: emit `any(xs)` / `all(xs)` over a `list[bool]` — leaves the i32
/// (0/1) boolean result on the stack. The list NAME lowers to its i32
/// base-pointer (the PMAT-968 list ABI over the PMAT-1251 `list[bool]` element
/// type: i32 count @ base+0, packed i32 0/1 elements @ base+8, a 4-byte stride),
/// then `$__wasm_list_bool_reduce` folds the payload with `is_all` pushed as an
/// i32 immediate (`1` for `all`, `0` for `any`) so one helper serves both
/// directions. Semantics match CPython/the iterator adaptors: `all([]) == True`,
/// `any([]) == False`, `all` short-circuits False on the first falsey element,
/// `any` short-circuits True on the first truthy one — all computed inside the
/// helper.
///
/// Honest scope (each a hard [`BackendError`], never a silent miscompile):
///   * the SHORT-CIRCUITING generator form (`any(P(x) for x in xs)`, which the
///     frontend tags `short_circuit` and wraps in an `Expr::Map` mapping each
///     element through a predicate lambda) is refused — it needs to lower an
///     arbitrary per-element lambda body (deferred, not half-wired). Only the
///     direct `list[bool]` reduction (a bare-Ident list) is emitted.
///   * a non-name list (a list LITERAL / temporary — `all([True, False])`) is
///     refused; bind it to a name first.
///   * a name that is not a `list[bool]` — whose elements do not load as i32 —
///     is refused by the element-type check against [`Scope::list_elem_of`].
///     (`any`/`all` over a `list[int]`/`list[float]` is lowered by the frontend
///     as a truthiness map + reduce, whose `Expr::Map` the WASM subset refuses.)
fn emit_bool_reduce(
    list: &Expr,
    is_all: bool,
    short_circuit: bool,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let op = if is_all { "all" } else { "any" };
    if short_circuit {
        return Err(unsupported(&format!(
            "{op}(<generator>) — the WASM subset folds a materialised `list[bool]` \
             only; the lazy short-circuiting generator form (a per-element \
             predicate lambda) is deferred (refused honestly)"
        )));
    }
    let Expr::Ident(name) = list else {
        return Err(unsupported(&format!(
            "{op}() of a non-name list — the WASM subset reduces a `list[bool]` NAME \
             (an i32 base-pointer into linear memory); a list literal / temporary \
             is refused (bind it to a name first)"
        )));
    };
    match scope.list_elem_of(name) {
        // A `list[bool]` loads its elements as i32 (0/1) — the truthiness fold.
        Some(WatTy::I32) => {}
        Some(other) => {
            return Err(unsupported(&format!(
                "{op}() over `{name}` whose elements load as {} — the WASM subset \
                 folds a `list[bool]` (i32 0/1 elements); `any`/`all` over a \
                 list[int]/list[float] wraps a per-element truthiness map that the \
                 subset refuses",
                other.keyword()
            )));
        }
        None => {
            return Err(unsupported(&format!(
                "{op}() over `{name}` which is not a `list[bool]` param/local — only \
                 a list (an i32 base-pointer into linear memory) can be reduced in \
                 the WASM subset"
            )));
        }
    }
    // Push the list base-pointer + the is_all selector, then fold via the helper.
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {}", i32::from(is_all)).expect("write");
    indent(out, depth);
    writeln!(out, "call $__wasm_list_bool_reduce").expect("write");
    Ok(WatTy::I32)
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
        // PMAT-1256: a LIST slice IS supported — but only when it is BOUND to a
        // `list[scalar]` local (`ys = xs[i:j]`, routed through the list-valued
        // path `emit_list_slice`). Reaching HERE means the slice sits in a
        // scalar/str value position (e.g. a direct `-> list` return or a str
        // context), which the WASM list subset does not carry — refuse honestly.
        return Err(unsupported(
            "a LIST slice `xs[i:j]` in a scalar/str position — the WASM list \
             subset materialises `xs[lo:hi]` only when it is BOUND to a \
             `list[scalar]` local (`ys = xs[i:j]`); refused here",
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
        // PMAT-1290: `s[i]` where `s` is a `set[str]` — the per-element read the
        // `for w in s` desugar emits, arriving in a STRING position because the
        // loop var is str-typed. Load entry `i`'s stored str base-pointer (i32)
        // from the 16-byte-stride set entry array (see `emit_set_elem_read`), so
        // the loop var behaves as an ordinary str local downstream (`len(w)`,
        // concat, `==`). Only a `set[str]` reaches here; a non-name collection or
        // a `set[int]`/list element in a str position is refused honestly.
        Expr::Index { collection, index } => {
            let Expr::Ident(name) = collection.as_ref() else {
                return Err(unsupported(
                    "indexing a non-name collection in a string position — only \
                     a `set[str]` local (iterated via `for w in s`) yields a str \
                     element in the WASM subset",
                ));
            };
            match scope.set_elem_of(name) {
                Some(WatTy::I32) if is_foreach_counter(index) => {
                    emit_set_elem_read(name, WatTy::I32, index, scope, out, depth)
                }
                Some(WatTy::I32) => Err(unsupported(&format!(
                    "subscripting the set `{name}` — a Python set is not \
                     subscriptable (`TypeError`); set element access exists only \
                     as the internal per-element read of `for w in {name}`"
                ))),
                _ => Err(unsupported(&format!(
                    "string-position index over `{name}` — only a `set[str]` \
                     local yields a str element in the WASM subset (a `set[int]` \
                     or list element is not a str)"
                ))),
            }
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
        // PMAT-1187: `s.capitalize()` in a string position — a fresh heap string
        // (like upper/lower, a materialising op) with the first ASCII letter
        // upper-cased and the rest lower-cased. 0-arg; the allocating
        // `$__wasm_str_capitalize` helper does the flip and TRAPS on a non-ASCII
        // byte (the honest ASCII-only boundary — full Unicode case mapping needs a
        // case table this scalar lane does not carry, so it refuses at runtime
        // rather than silently returning a wrongly-mapped string).
        Expr::StrMethod {
            recv,
            op: StrMethodOp::Capitalize,
            args,
        } if args.is_empty() => {
            emit_str_capitalize(recv, scope, out, depth)?;
            Ok(())
        }
        // PMAT-1201: `s.swapcase()` in a string position — a fresh heap string
        // (like upper/lower/capitalize, a materialising op) with the case of every
        // ASCII letter flipped BOTH ways. 0-arg; the allocating
        // `$__wasm_str_swapcase` helper does the flip and TRAPS on a non-ASCII byte
        // (the honest ASCII-only boundary — full Unicode case flipping needs a case
        // table this scalar lane does not carry, so it refuses at runtime rather
        // than silently returning a wrongly-flipped string).
        Expr::StrMethod {
            recv,
            op: StrMethodOp::SwapCase,
            args,
        } if args.is_empty() => {
            emit_str_swapcase(recv, scope, out, depth)?;
            Ok(())
        }
        // PMAT-1203: `s.title()` in a string position — a fresh heap string (like
        // capitalize/swapcase, a materialising op) title-cased word-by-word: the
        // first ASCII letter of each word upper-cased, the rest lower-cased, any
        // non-letter a word boundary. 0-arg; the allocating `$__wasm_str_title`
        // helper does the stateful flip and TRAPS on a non-ASCII byte (the honest
        // ASCII-only boundary — full Unicode title mapping needs a case table this
        // scalar lane does not carry, so it refuses at runtime rather than silently
        // returning a wrongly-cased string).
        Expr::StrMethod {
            recv,
            op: StrMethodOp::Title,
            args,
        } if args.is_empty() => {
            emit_str_title(recv, scope, out, depth)?;
            Ok(())
        }
        // PMAT-1213: `s[::-1]` in a string position — a fresh heap string (like the
        // case-fold family, a materialising op) with the CODE POINTS of `s` in reverse
        // order. The frontend lowers the `s[::-1]` reversed-slice to `StrMethod{op:
        // Reverse}`. 0-arg; the allocating `$__wasm_str_reverse` helper copies each
        // UTF-8 code point as an intact unit — unlike the case-fold ops it needs NO
        // Unicode table, so it is char-exact for any valid UTF-8 with NO trap arm
        // (`"café"[::-1] == "éfac"`), matching CPython and the rust/ruchy
        // `.chars().rev()` lane.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::Reverse,
            args,
        } if args.is_empty() => {
            emit_str_reverse(recv, scope, out, depth)?;
            Ok(())
        }
        // PMAT-1219: `s.expandtabs()` / `s.expandtabs(tabsize)` in a string position
        // — a fresh heap string (a materialising op) with each `\t` expanded to
        // spaces to the next multiple of `tabsize` (column counted in CODE POINTS,
        // reset on `\n`/`\r`). 0 or 1 arg: the omitted tabsize defaults to 8. The
        // allocating `$__wasm_str_expandtabs` helper copies each non-tab code point
        // verbatim (only ASCII tab/newline bytes are interpreted), so — unlike the
        // case-fold ops — it needs NO Unicode table and is char-exact for any valid
        // UTF-8 with NO trap arm (`"é\t".expandtabs(4)` → `"é   "`), matching CPython
        // and the rust/ruchy `.chars()` walk.
        Expr::StrMethod {
            recv,
            op: StrMethodOp::ExpandTabs,
            args,
        } if args.len() <= 1 => {
            emit_str_expandtabs(recv, args.first(), scope, out, depth)?;
            Ok(())
        }
        // PMAT-1205: `s.strip()` / `s.lstrip()` / `s.rstrip()` in a string position
        // — a fresh heap string (like the case-fold family, a materialising op) with
        // the leading (`Strip`/`LStrip`) and/or trailing (`Strip`/`RStrip`) run of
        // ASCII whitespace removed, the retained byte range copied verbatim. 0-arg
        // (the no-arg whitespace form; the 1-arg char-set form `s.strip(chars)` is
        // rejected upstream by the frontend arity check, so it never reaches here).
        // The allocating `$__wasm_str_strip` helper does the trim and TRAPS on a
        // non-ASCII BOUNDARY byte (the honest ASCII-only boundary — the
        // whitespace-ness of a non-ASCII byte is undecidable without a Unicode table
        // this scalar lane lacks, so it refuses at runtime rather than silently
        // keeping or dropping the wrong run).
        Expr::StrMethod {
            recv,
            op: op @ (StrMethodOp::Strip | StrMethodOp::LStrip | StrMethodOp::RStrip),
            args,
        } if args.is_empty() => {
            let left = matches!(op, StrMethodOp::Strip | StrMethodOp::LStrip);
            let right = matches!(op, StrMethodOp::Strip | StrMethodOp::RStrip);
            emit_str_strip(recv, left, right, scope, out, depth)?;
            Ok(())
        }
        // PMAT-1209: `s.rjust(w)` / `s.ljust(w)` / `s.center(w)` in a string position
        // — a fresh heap string (like zfill, a materialising op) equal to `s` padded
        // with ASCII space to `w` code points. The 1-arg (default-space) form; the
        // frontend allows an optional 2-arg fill-char form (`s.rjust(w, "*")`), which
        // is refused below (a non-space fill needs a variable-width fill byte this
        // shared space-pad helper does not carry). The allocating `$__wasm_str_pad`
        // helper splits the pad by `mode` (rjust=0 left / ljust=1 right / center=2
        // CPython-biased) and — unlike the case-fold ops — never inspects a payload
        // byte, so it is char-exact for any UTF-8 with NO trap.
        Expr::StrMethod {
            recv,
            op: op @ (StrMethodOp::RJust | StrMethodOp::LJust | StrMethodOp::Center),
            args,
        } if args.len() == 1 => {
            let mode = match op {
                StrMethodOp::RJust => 0,
                StrMethodOp::LJust => 1,
                StrMethodOp::Center => 2,
                _ => unreachable!("guarded by the arm's op pattern"),
            };
            emit_str_pad(recv, &args[0], mode, scope, out, depth)?;
            Ok(())
        }
        // PMAT-1209: the 2-arg fill-char form `s.rjust(w, fill)` / `.ljust` /
        // `.center` — refused honestly. The shared `$__wasm_str_pad` helper pads with
        // a fixed ASCII space (a single `memory.fill` byte); a non-space fill char
        // (which may itself be multi-byte UTF-8) is not modelled on this lane.
        Expr::StrMethod {
            op: StrMethodOp::RJust | StrMethodOp::LJust | StrMethodOp::Center,
            args,
            ..
        } if args.len() == 2 => Err(unsupported(
            "the 2-arg fill-char form of `.rjust(w, fill)` / `.ljust(w, fill)` / \
             `.center(w, fill)` on the WASM lane — the space-pad helper pads with a \
             fixed ASCII space; drop the fill char to pad with spaces, or build the \
             padding explicitly",
        )),
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
             count])`, `.zfill(width)`, `.rjust(w)` / `.ljust(w)` / \
             `.center(w)` (space-pad, char-exact), `s[::-1]` (reverse, \
             char-exact), `.expandtabs([n])` (tab-expand, char-exact), \
             `.upper()` / `.lower()` / \
             `.capitalize()` / `.swapcase()` / `.title()` / `.strip()` / \
             `.lstrip()` / `.rstrip()` (ASCII-only — a non-ASCII byte traps), \
             or a str-returning call; stepped slicing / str(float) / bare \
             f-strings are refused",
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
        // PMAT-1247: SET ALGEBRA — `a | b` / `a & b` / `a - b` / `a ^ b` yields a
        // NEW set (never mutating an operand), so it is a valid set-binding value.
        // Dispatches to the per-op allocating runtime helper.
        Expr::SetOp { lhs, op, rhs } => emit_set_op(lhs, *op, rhs, kind, scope, out, depth),
        other => Err(unsupported(&format!(
            "a `dict`/`set` binding must be a dict/set LITERAL or a set-algebra \
             expression (`a | b` / `a & b` / `a - b` / `a ^ b`) in the WASM subset \
             (a dict/set-returning call, comprehension, or copy is refused) — got {}",
            expr_kind(other)
        ))),
    }
}

/// PMAT-1247: lower a SET-ALGEBRA binding value — `a | b` (union) / `a & b`
/// (intersection) / `a - b` (difference) / `a ^ b` (symmetric difference),
/// carried by [`Expr::SetOp`] — leaving the NEW set's `i32` base-pointer on the
/// stack (Python set algebra yields a fresh set, never mutating an operand; the
/// caller `local.set`s it, exactly like a `SetLit`). Both operands must be set
/// NAMES of the binding's key kind — the runtime helper walks two entry regions
/// of a shared key encoding, so a non-name or kind-mismatched operand is refused
/// honestly (never a base-pointer combine). Dispatches to the per-op allocating
/// helper `$__wasm_set_<op>_<k>(a, b) -> i32` co-emitted by [`dict_helpers_for`].
fn emit_set_op(
    lhs: &Expr,
    op: SetOp,
    rhs: &Expr,
    kind: KeyKind,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let (Expr::Ident(ln), Expr::Ident(rn)) = (lhs, rhs) else {
        return Err(unsupported(
            "set algebra (`a | b` / `a & b` / `a - b` / `a ^ b`) with a non-name \
             operand — the WASM subset combines two set LOCALS; bind a set literal \
             or other set-valued expression to a local first",
        ));
    };
    if !(scope.is_set(ln) && scope.is_set(rn)) {
        return Err(unsupported(
            "set algebra mixing a `set` operand with a non-`set` operand — a set \
             op only ever combines two sets; refused honestly rather than \
             combining base-pointers",
        ));
    }
    let lk = scope.heap_map_kind(ln).expect("a set local has a key kind");
    let rk = scope.heap_map_kind(rn).expect("a set local has a key kind");
    if lk != rk || lk != kind {
        return Err(unsupported(&format!(
            "set algebra {op:?} over sets whose key kinds disagree (a {} {} a {} \
             into a {} set) — the result and both operands must share one key \
             encoding; refused honestly",
            lk.suffix(),
            set_op_symbol(op),
            rk.suffix(),
            kind.suffix()
        )));
    }
    indent(out, depth);
    writeln!(out, "local.get ${ln}").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${rn}").expect("write");
    indent(out, depth);
    writeln!(
        out,
        "call $__wasm_set_{}_{}",
        set_op_name(op),
        kind.suffix()
    )
    .expect("write");
    Ok(())
}

/// The `$__wasm_set_<name>_<k>` stem for a set-algebra op.
fn set_op_name(op: SetOp) -> &'static str {
    match op {
        SetOp::Union => "union",
        SetOp::Intersection => "intersection",
        SetOp::Difference => "difference",
        SetOp::SymmetricDifference => "symdiff",
    }
}

/// The Python operator glyph for a set-algebra op (diagnostics only).
fn set_op_symbol(op: SetOp) -> &'static str {
    match op {
        SetOp::Union => "|",
        SetOp::Intersection => "&",
        SetOp::Difference => "-",
        SetOp::SymmetricDifference => "^",
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

/// PMAT-1223: lower `d.get(k, default)` (`Expr::DictGetOr`) — a TOTAL dict read
/// that never traps. Emits `if has(p, k) then get(p, k) else default`: the
/// membership helper (`$__wasm_dict_has_<k>`, i32, never traps) gates the
/// TRAPPING value helper (`$__wasm_dict_get_<k>`, i64) so `get` runs ONLY when
/// the key is present; an absent key falls to the int `default` instead of
/// `unreachable`-trapping, exactly like CPython's `d.get(k, default)` vs the
/// bare `d[k]` KeyError. Both helpers already exist (shared with
/// [`emit_dict_get`]/[`emit_dict_contains`]) and are gated on the dict's
/// declared `Type::Dict` local ([`module_dict_key_kinds`]), so this op declares
/// no new helper. The WASM dict value type is `i64`, so both `if` arms and the
/// default lower to `i64`. The key expression is emitted twice (once per helper
/// call) — cheap and side-effect-free for the literal/ident keys this subset
/// admits, and it lets `get` reuse the same encoded key `has` just tested.
fn emit_dict_get_or(
    dict: &Expr,
    key: &Expr,
    default: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let (name, kind) = dict_ident_kind(dict, scope)?;
    // condition: has(p, k) -> i32 (never traps)
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    emit_dict_key(key, kind, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_dict_has_{}", kind.suffix()).expect("write");
    // if (result i64) get(p, k) else default
    indent(out, depth);
    writeln!(out, "if (result i64)").expect("write");
    indent(out, depth + 1);
    writeln!(out, "local.get ${name}").expect("write");
    emit_dict_key(key, kind, scope, out, depth + 1)?;
    indent(out, depth + 1);
    writeln!(out, "call $__wasm_dict_get_{}", kind.suffix()).expect("write");
    indent(out, depth);
    writeln!(out, "else").expect("write");
    emit_expr_typed(default, scope, out, depth + 1, WatTy::I64)?;
    indent(out, depth);
    writeln!(out, "end").expect("write");
    Ok(WatTy::I64)
}

/// PMAT-1225: lower `d.pop(k)` / `d.pop(k, default)` (`Expr::DictPop`) — a dict
/// read that ALSO REMOVES the entry. The keyed `pop` helper
/// (`$__wasm_dict_pop_<k>`, i64) scans for the key, captures its value, swaps
/// the last entry into the hole, decrements the count, and returns the value;
/// removal shrinks the region in place, so the dict's base pointer never moves
/// and — unlike `d[k] = v` ([`emit_dict_set`]) — there is NO local write-back.
///
/// The bare `d.pop(k)` (`default: None`) traps (unreachable) on an absent key,
/// exactly CPython's KeyError. The 2-arg `d.pop(k, default)` never traps: the
/// membership helper (`$__wasm_dict_has_<k>`, i32) gates the MUTATING `pop` so
/// it runs ONLY when the key is present; an absent key falls to the int
/// `default` WITHOUT mutating — the same `if has then … else default` shape as
/// [`emit_dict_get_or`], but the present branch pops (removes+returns) rather
/// than reads. Both `if` arms and the value lower to i64. As in `emit_dict_get_or`
/// the key expression is emitted twice on the 2-arg path (once per helper call)
/// — cheap and side-effect-free for the literal/ident keys this subset admits.
fn emit_dict_pop(
    dict: &Expr,
    key: &Expr,
    default: Option<&Expr>,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let (name, kind) = dict_ident_kind(dict, scope)?;
    match default {
        // d.pop(k): unconditional pop; the helper's not-found tail traps.
        None => {
            indent(out, depth);
            writeln!(out, "local.get ${name}").expect("write");
            emit_dict_key(key, kind, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_dict_pop_{}", kind.suffix()).expect("write");
        }
        // d.pop(k, default): if has(p,k) then pop(p,k) else default.
        Some(default) => {
            indent(out, depth);
            writeln!(out, "local.get ${name}").expect("write");
            emit_dict_key(key, kind, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_dict_has_{}", kind.suffix()).expect("write");
            indent(out, depth);
            writeln!(out, "if (result i64)").expect("write");
            indent(out, depth + 1);
            writeln!(out, "local.get ${name}").expect("write");
            emit_dict_key(key, kind, scope, out, depth + 1)?;
            indent(out, depth + 1);
            writeln!(out, "call $__wasm_dict_pop_{}", kind.suffix()).expect("write");
            indent(out, depth);
            writeln!(out, "else").expect("write");
            emit_expr_typed(default, scope, out, depth + 1, WatTy::I64)?;
            indent(out, depth);
            writeln!(out, "end").expect("write");
        }
    }
    Ok(WatTy::I64)
}

/// PMAT-1234: lower `del d[k]` (`Stmt::DelItem`, `is_dict`) — dict entry removal
/// in STATEMENT position. It is exactly the bare `d.pop(k)` ([`emit_dict_pop`]
/// with `default: None`) with the returned value discarded: the shared keyed
/// removal helper (`$__wasm_dict_pop_<k>`, swap-last-into-hole + count--) does
/// the mutation IN PLACE (the region only shrinks, so the base pointer never
/// moves and there is NO local write-back — unlike `d[k] = v`), and the trailing
/// `drop` throws away the i64 value nobody asked for. The helper's not-found
/// tail traps (`unreachable`), matching CPython `del d[missing]` → KeyError.
///
/// The key's `KeyKind` is read from the dict local's declared type
/// (`heap_map_kind`), exactly as [`emit_dict_set`] does for `d[k] = v` — a
/// non-dict `name` is refused. The LIST form (`del xs[i]`, `is_dict == false`)
/// is delegated to [`emit_list_delitem`] (PMAT-1284): the list runtime DOES have
/// a shrink-and-shift now (the in-place mirror of `insert`), so a `list[int]` /
/// `list[float]` element deletion is supported (a `list[bool]` is refused there,
/// pending the i32-stride twin).
fn emit_dict_del(
    name: &str,
    key: &Expr,
    is_dict: bool,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    if !is_dict {
        return emit_list_delitem(name, key, scope, out, depth);
    }
    let kind = scope.heap_map_kind(name).ok_or_else(|| {
        unsupported(&format!(
            "`del {name}[k]` over `{name}` which is not a `dict` local in the \
             WASM subset"
        ))
    })?;
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    emit_dict_key(key, kind, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_dict_pop_{}", kind.suffix()).expect("write");
    // Discard the removed value — `del` is a statement, the removal is the point.
    indent(out, depth);
    writeln!(out, "drop").expect("write");
    Ok(())
}

/// PMAT-1284: lower `del xs[i]` over a LIST (`Stmt::DelItem`, `is_dict == false`)
/// — element deletion at an arbitrary index, the in-place MIRROR of
/// [`emit_list_insert`] (grow+shift-right ↔ shrink+shift-left).
///
/// All the real work (CPython index normalise + IndexError trap + low→high tail
/// shift-left + count--) lives in the single [`LIST_DELITEM_HELPER`]
/// (`$__wasm_list_delitem`); this call site just (1) resolves the element kind —
/// accepting a `list[int]`/`list[float]` (both 8-byte, one shared helper),
/// refusing a `list[bool]` (i32 stride, deferred like `insert`) or a non-list
/// name — pushes the base-pointer and the index TYPED to i64 (the frontend
/// guarantees an int index; the helper normalises in signed i64), and `call`s the
/// helper. Unlike `insert`/`append`, deletion SHRINKS, so it imposes NO
/// growable-list precondition: it accepts ANY list local with a valid
/// base-pointer (a param included — the record only gets smaller, the
/// base-pointer never moves, so no overrun and every alias observes it), exactly
/// like `pop`.
fn emit_list_delitem(
    list_name: &str,
    index_expr: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // The element kind must be a fixed-width 8-byte scalar (i64/f64) — the delete
    // helper moves whole 8-byte words. A `list[bool]` (i32 elements) would need an
    // i32-stride shift twin (deferred, exactly like `insert`/`append`), and a
    // `list[str]` is not a fixed-width scalar list.
    match scope.list_elem_of(list_name) {
        Some(WatTy::I64) | Some(WatTy::F64) => {}
        Some(other) => {
            return Err(unsupported(&format!(
                "`del {list_name}[i]` whose elements load as {} — the WASM subset \
                 deletes only from a `list[int]` / `list[float]` (an 8-byte i64/f64 \
                 payload); this element kind is refused (a `list[bool]` would need an \
                 i32-stride shift twin, deferred like `insert`)",
                other.keyword()
            )));
        }
        None => {
            return Err(unsupported(&format!(
                "`del {list_name}[i]` where `{list_name}` is not a `list[int]` / \
                 `list[float]` param/local — only a scalar list (an i32 base-pointer \
                 into linear memory) can be deleted from in the WASM subset"
            )));
        }
    };
    // base (i32) ; RAW index typed to i64 (the helper normalises + bounds-checks
    // in signed i64) ; then the shrink-and-shift helper does the work (void
    // statement). PMAT-1289: a NEGATIVE-LITERAL `del xs[-k]` arrives
    // pre-rewritten to `len(xs) - k` (PMAT-570, for the Rust lane's `usize`
    // remove) — recover the raw `-k` so the helper's normalise applies ONCE
    // (passing the rewritten value through double-normalised: `del xs[-4]` on a
    // 3-element list silently deleted slot 2 where CPython raises `IndexError`
    // — REFUTED by the PMAT-1289 differential fuzz).
    indent(out, depth);
    writeln!(out, "local.get ${list_name}").expect("write");
    if let Some(k) = neg_literal_index_k(index_expr, list_name) {
        indent(out, depth);
        writeln!(out, "i64.const {}", -k).expect("write");
    } else {
        emit_expr_typed(index_expr, scope, out, depth, WatTy::I64)?;
    }
    indent(out, depth);
    writeln!(out, "call $__wasm_list_delitem").expect("write");
    Ok(())
}

/// PMAT-1285: lower `xs.remove(v)` over a `list[int]`/`list[float]`
/// (`Stmt::ListRemoveValue`) — remove the FIRST element EQUAL to `v` (a VALUE
/// delete, not an index delete like `del xs[i]`).
///
/// All the real work (a linear scan for the first typed match + the same
/// left-shrinking tail shift as `del` + count-- + the ValueError trap on a miss)
/// lives in the `$__wasm_list_remove_{i64,f64}` helper. This call site (1) resolves
/// the element kind to pick the typed helper — accepting a `list[int]` (i64) /
/// `list[float]` (f64), refusing a `list[bool]` (i32 stride, deferred like
/// `insert`) or a non-list name — then (2) pushes the base-pointer and the value
/// TYPED to the element kind, and `call`s the helper.
///
/// Unlike `insert`/`append`, `remove` only SHRINKS (the base-pointer never moves,
/// no overrun), so — exactly like `del`/`pop` — it imposes NO growable-list
/// precondition: it accepts ANY scalar list local, a PARAM included (every alias
/// observes the removal).
fn emit_list_remove(
    list_name: &str,
    value_expr: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // The element kind selects the typed helper and the value's WAT type. Only the
    // i64/f64 scalar element kinds are lowered; a `list[bool]` (i32 elements) would
    // need an i32 helper twin (deferred, exactly like `insert`), and a `list[str]`
    // is not a fixed-width scalar list.
    let (helper, want_elem) = match scope.list_elem_of(list_name) {
        Some(WatTy::I64) => ("$__wasm_list_remove_i64", WatTy::I64),
        Some(WatTy::F64) => ("$__wasm_list_remove_f64", WatTy::F64),
        Some(other) => {
            return Err(unsupported(&format!(
                "`{list_name}.remove(v)` whose elements load as {} — the WASM \
                 subset removes only from a `list[int]` / `list[float]` (an i64/f64 \
                 payload); this element kind is refused (a `list[bool]` would need an \
                 i32 helper twin, deferred like `insert`)",
                other.keyword()
            )));
        }
        None => {
            return Err(unsupported(&format!(
                "`{list_name}.remove(v)` where `{list_name}` is not a `list[int]` / \
                 `list[float]` param/local — only a scalar list (an i32 base-pointer \
                 into linear memory) can be removed from in the WASM subset"
            )));
        }
    };
    // base (i32) ; value typed to the element kind ; then the scan-and-shift helper
    // finds the first match, closes the hole, and drops the count (or traps on a
    // miss = Python ValueError). `remove` only SHRINKS in place, so — unlike
    // `insert`/`append` — it imposes NO growable-list precondition (a param is fine).
    indent(out, depth);
    writeln!(out, "local.get ${list_name}").expect("write");
    emit_expr_typed(value_expr, scope, out, depth, want_elem)?;
    indent(out, depth);
    writeln!(out, "call {helper}").expect("write");
    Ok(())
}

/// PMAT-1227: lower `d.setdefault(k, default)` (`Expr::DictSetDefault`) — a
/// get-or-INSERT. On a HIT the key's value is read unchanged; on a MISS
/// `default` is inserted under `k` and returned. Unlike [`emit_dict_get_or`]
/// (a total READ) and [`emit_dict_pop`] (removal, which only shrinks in place),
/// the miss path MUTATES *and* may GROW the dict: `$__wasm_dict_set_<k>`
/// 2x-reallocs when the region is full and returns the (possibly relocated)
/// base-pointer, which is written back into the dict local — exactly like
/// [`emit_dict_set`]'s `d[k] = v`.
///
/// Emits `if not has(p, k): p = set(p, k, default)` (the membership helper
/// `$__wasm_dict_has_<k>` never traps and GATES the insert, so a HIT never
/// overwrites — CPython keeps the existing value) then reads back
/// `get(p, k)` (now guaranteed present, i64). The trailing `get` re-scans, but
/// that keeps a SINGLE i64 return path and emits `default` exactly once (the
/// insert value) — simpler than the double-emit `if (result i64)` shape, and
/// correct because setdefault's post-condition is `k in d`. The key expression
/// is emitted 2–3× (has, [set], get) — cheap and side-effect-free for the
/// literal/ident keys this subset admits, as in
/// [`emit_dict_get_or`]/[`emit_dict_pop`]. All three helpers already exist
/// (shared, gated on the dict's declared `Type::Dict` local), so this op
/// declares no new helper; it only rides the `$__alloc` heap gate (the insert
/// can grow), forced true for `DictSetDefault` in [`expr_has_heap_op`].
fn emit_dict_set_default(
    dict: &Expr,
    key: &Expr,
    default: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let (name, kind) = dict_ident_kind(dict, scope)?;
    let suffix = kind.suffix();
    // if not has(p, k): p = set(p, k, default)  — insert-if-absent (never overwrites).
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    emit_dict_key(key, kind, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_dict_has_{suffix}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.eqz").expect("write");
    indent(out, depth);
    writeln!(out, "if").expect("write");
    indent(out, depth + 1);
    writeln!(out, "local.get ${name}").expect("write");
    emit_dict_key(key, kind, scope, out, depth + 1)?;
    emit_expr_typed(default, scope, out, depth + 1, WatTy::I64)?;
    indent(out, depth + 1);
    writeln!(out, "call $__wasm_dict_set_{suffix}").expect("write");
    indent(out, depth + 1);
    writeln!(out, "local.set ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "end").expect("write");
    // return d[k] — present after the insert-if-absent above (i64).
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    emit_dict_key(key, kind, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_dict_get_{suffix}").expect("write");
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

/// PMAT-1236 / PMAT-1286: lower an in-place list/dict/set mutator
/// (`Stmt::ListMutate`). The frontend routes `.sort()`/`.reverse()`/`.clear()`
/// on a dict, a set, and a list ALIKE to this node, so the operation AND the
/// target's runtime kind decide support:
///   * `.reverse()` on a `list[int]`/`list[float]` (PMAT-1286) → an in-place
///     two-pointer 8-byte word swap via [`emit_list_reverse`]. The count is
///     unchanged, so the base-pointer never moves (every alias observes it) and
///     ANY scalar list local qualifies — a param included — with no capacity
///     guard. Delegated below.
///   * `.clear()` on a dict/set local (`heap_map_kind(name)` → `Some(_)`) →
///     zeroes the live-entry COUNT header at `base+0` (the `+0` count `len(d)`
///     reads), leaving the capacity + stale bytes as garbage below `count`. That
///     is the entire cost — the region only shrinks, so the base-pointer never
///     moves (no `local.set` write-back), and no helper/trap is involved. A later
///     `d[k] = v` re-inserts from count 0 into the existing capacity.
///   * `.sort()`/`.sort(reverse=…)`, and `.clear()` on a LIST, are refused
///     honestly: an in-place SORT needs the typed compare the two `sorted`
///     helpers carry wired to an in-place pass (a clean follow-up; use
///     `xs = sorted(xs)` for now), and a list `.clear()` count-reset is a
///     separate slice (though `.append` — PMAT-1276 — could now re-grow one).
fn emit_list_mutate(
    name: &str,
    op: ListMutateOp,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    match op {
        // PMAT-1286: in-place `xs.reverse()`. Accepts any scalar list local.
        ListMutateOp::Reverse => return emit_list_reverse(name, scope, out, depth),
        // PMAT-1288: in-place `xs.sort()` / `xs.sort(reverse=True)` — the typed
        // stable insertion sort run directly over the receiver's payload.
        ListMutateOp::Sort | ListMutateOp::SortDesc => {
            return emit_list_sort_inplace(name, op == ListMutateOp::SortDesc, scope, out, depth);
        }
        ListMutateOp::Clear => {}
    }
    // PMAT-1288: `.clear()` accepts a dict/set (PMAT-1236) OR any scalar-list
    // local — a list record shares the dict/set posture: the live-element count
    // is the i32 header at base+0, so a clear is the SAME bare header-zero, and
    // it is STRIDE-AGNOSTIC (no payload touch), so every list element kind
    // qualifies. The capacity header at LIST_CAP_OFFSET is untouched, so a
    // cleared literal-bound list stays appendable from count 0 (reusing its
    // existing slack), and a cleared param/alias shrinks in place (the
    // base-pointer never moves → every alias observes `len == 0`).
    if scope.heap_map_kind(name).is_none() && scope.list_elem_of(name).is_none() {
        return Err(unsupported(&format!(
            "`{name}.clear()` over `{name}` which is not a `dict`/`set`/`list` \
             param/local — only a length-prefixed heap record (an i32 live-count \
             header at base+0) can be count-reset in place in the WASM subset"
        )));
    }
    // Zero the live-entry count header at base+0 (`len` reads this same word).
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const 0").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store").expect("write");
    Ok(())
}

/// PMAT-1286: lower `xs.reverse()` (`Stmt::ListMutate` with `ListMutateOp::Reverse`)
/// — reverse a `list[int]`/`list[float]` local IN PLACE. All the work (the
/// two-pointer 8-byte word swap, empty/single-element no-op) lives in the single
/// `$__wasm_list_reverse` helper ([`LIST_REVERSE_INPLACE_HELPER`]); this call site
/// only (1) resolves the element kind to enforce a scalar-list receiver — a swap
/// MOVES words verbatim, so ONE helper handles both i64 and f64, but a
/// `list[bool]` (4-byte i32 stride) is refused for parity with `sorted`/`reversed`
/// (a distinct-stride helper is deferred), and a non-list name is refused — and
/// (2) pushes the base-pointer and `call`s the helper (which returns nothing —
/// `reverse` is a void statement). Because a reversal leaves the count unchanged,
/// there is NO growable-list precondition: any scalar list local (a param
/// included) is accepted, exactly like `del`/`remove`.
fn emit_list_reverse(
    name: &str,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    match scope.list_elem_of(name) {
        Some(WatTy::I64) | Some(WatTy::F64) => {}
        Some(other) => {
            return Err(unsupported(&format!(
                "`{name}.reverse()` whose elements load as {} — the WASM subset \
                 reverses only a `list[int]` / `list[float]` (an i64/f64 payload) \
                 in place; this element kind is refused (a `list[bool]` would need \
                 an i32-stride helper twin, deferred like `sorted`/`reversed`)",
                other.keyword()
            )));
        }
        None => {
            return Err(unsupported(&format!(
                "`{name}.reverse()` where `{name}` is not a `list[int]` / \
                 `list[float]` param/local — only a scalar list (an i32 \
                 base-pointer into linear memory) can be reversed in place in the \
                 WASM subset"
            )));
        }
    }
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "call $__wasm_list_reverse").expect("write");
    Ok(())
}

/// PMAT-1288: lower `xs.sort()` / `xs.sort(reverse=True)` (`Stmt::ListMutate`
/// with `ListMutateOp::Sort`/`SortDesc`) — stable-sort a `list[int]`/`list[float]`
/// local IN PLACE. All the work (the strict-compare insertion sort — the SAME
/// compare opcodes as the allocating `sorted` pair, so `xs.sort()` and
/// `xs = sorted(xs)` order a payload identically) lives in the typed
/// `$__wasm_list_sort_{i64,f64}` helpers; this call site only (1) resolves the
/// element kind from [`Scope::list_elem_of`] to pick the typed helper — refusing
/// a `list[bool]` (4-byte i32 stride; a distinct-stride twin is deferred, like
/// `sorted`) and a non-list name — and (2) pushes the base-pointer plus the
/// direction flag (`i32.const 1` for `reverse=True`, `0` ascending) and `call`s
/// the helper (which returns nothing — `sort` is a void statement). Because a
/// sort leaves the count unchanged and the record never relocates, there is NO
/// growable-list precondition: any scalar list local (a PARAM included) is
/// accepted, exactly like `reverse`/`del`/`remove`, and every alias observes
/// the new order. The stmt's `of_float` flag is intentionally ignored — the
/// scope resolution is the single source of truth for the element kind, and
/// the gate emits BOTH twins so the two can never disagree.
fn emit_list_sort_inplace(
    name: &str,
    desc: bool,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let method = if desc { "sort(reverse=True)" } else { "sort()" };
    let helper = match scope.list_elem_of(name) {
        Some(WatTy::I64) => "$__wasm_list_sort_i64",
        Some(WatTy::F64) => "$__wasm_list_sort_f64",
        Some(other) => {
            return Err(unsupported(&format!(
                "`{name}.{method}` whose elements load as {} — the WASM subset \
                 sorts only a `list[int]` / `list[float]` (an i64/f64 payload) in \
                 place; this element kind is refused (a `list[bool]` would need an \
                 i32-stride helper twin, deferred like `sorted`/`reversed`)",
                other.keyword()
            )));
        }
        None => {
            return Err(unsupported(&format!(
                "`{name}.{method}` where `{name}` is not a `list[int]` / \
                 `list[float]` param/local — only a scalar list (an i32 \
                 base-pointer into linear memory) can be sorted in place in the \
                 WASM subset"
            )));
        }
    };
    indent(out, depth);
    writeln!(out, "local.get ${name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {}", i32::from(desc)).expect("write");
    indent(out, depth);
    writeln!(out, "call {helper}").expect("write");
    Ok(())
}

/// PMAT-1282: lower `xs.insert(i, v)` (`Stmt::ListInsert`) — insert `v` before
/// position `i` in a literal-bound `list[int]`/`list[float]` local IN PLACE.
///
/// All the real work (CPython index clamp + high→low tail shift + count bump +
/// capacity trap) lives in the `$__wasm_list_insert_{i64,f64}` helper; this call
/// site just (1) resolves the element kind to pick the typed helper — refusing a
/// `list[bool]`/`list[str]` or a non-list name — and (2) enforces the SAME
/// growable-list precondition as [`emit_list_append`] (a `ListInsert` grows the
/// count, so the list must be bound to a LITERAL that reserved spare capacity; a
/// param / alias / `sorted`/`reversed`/`concat`/slice result carries no slack and
/// is refused rather than overrunning the record). It then pushes the base-pointer,
/// the index TYPED to i64 (the frontend already guarantees an int index; the helper
/// clamps in signed i64), and the value typed to the element kind, and `call`s the
/// helper (which returns nothing — `insert` is a void statement).
fn emit_list_insert(
    list_name: &str,
    index_expr: &Expr,
    elem_expr: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // The element kind selects the typed helper and the value's WAT type. Only the
    // i64/f64 scalar element kinds are lowered; a `list[bool]` (i32 elements) would
    // need an i32 helper twin (deferred, exactly like `append`, which is likewise
    // int/float only), and a `list[str]` is not a fixed-width scalar list.
    let (helper, want_elem) = match scope.list_elem_of(list_name) {
        Some(WatTy::I64) => ("$__wasm_list_insert_i64", WatTy::I64),
        Some(WatTy::F64) => ("$__wasm_list_insert_f64", WatTy::F64),
        Some(other) => {
            return Err(unsupported(&format!(
                "`{list_name}.insert(i, v)` whose elements load as {} — the WASM \
                 subset inserts only into a `list[int]` / `list[float]` (an i64/f64 \
                 payload); this element kind is refused (a `list[bool]` would need an \
                 i32 helper twin, deferred like `append`)",
                other.keyword()
            )));
        }
        None => {
            return Err(unsupported(&format!(
                "`{list_name}.insert(i, v)` where `{list_name}` is not a `list[int]` / \
                 `list[float]` param/local — only a scalar list (an i32 base-pointer \
                 into linear memory) can be inserted into in the WASM subset"
            )));
        }
    };
    // `insert` GROWS the list, so — exactly like `append` — it requires the spare
    // capacity only a LITERAL binding reserves. A param, an alias, or a
    // `sorted`/`reversed`/`concat`/slice result has no slack; inserting would
    // overrun the record, so it is refused at compile time rather than corrupting
    // adjacent heap.
    if !scope.is_growable_list(list_name) {
        return Err(unsupported(&format!(
            "`{list_name}.insert(i, v)` — the WASM subset inserts only into a list \
             bound to a LITERAL (`{list_name} = []` / `{list_name} = [..]`), which \
             reserves spare capacity. `{list_name}` is a param, an alias, or a \
             `sorted`/`reversed`/`concat`/slice result (no spare capacity); inserting \
             into it is refused rather than overrunning the record"
        )));
    }
    // base (i32) ; index typed to i64 (the helper clamps in signed i64) ; value
    // typed to the element kind ; then the shift-and-insert helper does the work.
    indent(out, depth);
    writeln!(out, "local.get ${list_name}").expect("write");
    emit_expr_typed(index_expr, scope, out, depth, WatTy::I64)?;
    emit_expr_typed(elem_expr, scope, out, depth, want_elem)?;
    indent(out, depth);
    writeln!(out, "call {helper}").expect("write");
    Ok(())
}

/// PMAT-1276: lower `xs.append(v)` (`Stmt::ListAppend`) — append `v` to a
/// literal-bound `list[scalar]` local IN PLACE.
///
/// The list record carries an i32 live-element **count** at `base+0` and a
/// FIXED slot **capacity** at `base+4` ([`LIST_CAP_OFFSET`]), the slack
/// [`emit_list_lit`] reserved. Append:
///   1. loads `count` and `capacity`; if `count >= capacity`, traps
///      (`unreachable`) — the honest bounded-capacity boundary (never a heap
///      overrun, and — because the base-pointer never moves — never the
///      alias-invalidating relocation the PMAT-1033 refusal warned about);
///   2. else stores `v` at `base + LIST_ELEMS_OFFSET + count*stride` (a
///      natural-width `*.store`, matching the element read/write path);
///   3. writes `count + 1` back to `base+0` so every subsequent `len(xs)` /
///      `xs[i]` / `for x in xs` / reduction sees the appended element.
///
/// `count` is re-read from memory each time rather than cached in a scratch
/// local (it is unchanged until the final write-back), so no per-function
/// scratch declaration is needed. Only an APPEND-safe list (a `ListLit`
/// binding — see [`Scope::is_growable_list`]) is accepted; a param, an alias,
/// or a helper-allocated result (`sorted`/`reversed`/`concat`/`slice`, none of
/// which reserve spare capacity) is refused here at compile time.
fn emit_list_append(
    list_name: &str,
    elem_expr: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let Some(elem) = scope.list_elem_of(list_name) else {
        return Err(unsupported(&format!(
            "`{list_name}.append(v)` over `{list_name}` which is not a \
             `list[scalar]` local — the WASM subset appends to a named \
             `list[int]`/`list[float]`/`list[bool]` only"
        )));
    };
    if !scope.is_growable_list(list_name) {
        return Err(unsupported(&format!(
            "`{list_name}.append(v)` — the WASM subset appends only to a list \
             bound to a LITERAL (`{list_name} = []` / `{list_name} = [..]`), \
             which reserves spare capacity. `{list_name}` is a param, an alias, \
             or a `sorted`/`reversed`/`concat`/slice result (no spare capacity); \
             appending to it is refused rather than overrunning the record"
        )));
    }
    let stride = elem.byte_size();
    // Bounds/capacity guard: if count(base+0) >= capacity(base+4) → trap.
    indent(out, depth);
    writeln!(out, "local.get ${list_name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load ;; count @ base+0").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${list_name}").expect("write");
    indent(out, depth);
    writeln!(
        out,
        "i32.load offset={LIST_CAP_OFFSET} ;; capacity @ base+4"
    )
    .expect("write");
    indent(out, depth);
    writeln!(out, "i32.ge_u").expect("write");
    indent(out, depth);
    writeln!(out, "if").expect("write");
    indent(out, depth + 1);
    writeln!(
        out,
        "unreachable ;; append past fixed capacity (bounded bump heap)"
    )
    .expect("write");
    indent(out, depth);
    writeln!(out, "end").expect("write");
    // addr = base + count*stride ; then store v at offset=LIST_ELEMS_OFFSET.
    indent(out, depth);
    writeln!(out, "local.get ${list_name}").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${list_name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load ;; count").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {stride}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.mul").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add ;; base + count*stride").expect("write");
    emit_expr_typed(elem_expr, scope, out, depth, elem)?;
    indent(out, depth);
    writeln!(out, "{} offset={LIST_ELEMS_OFFSET}", elem.store_instr()).expect("write");
    // count = count + 1 (write back to base+0).
    indent(out, depth);
    writeln!(out, "local.get ${list_name}").expect("write");
    indent(out, depth);
    writeln!(out, "local.get ${list_name}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.load").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const 1").expect("write");
    indent(out, depth);
    writeln!(out, "i32.add").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store ;; count = count + 1").expect("write");
    Ok(())
}

/// The Python method spelling for a [`ListMutateOp`], for refusal messages.
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

/// PMAT-1240: lower `s.remove(e)` / `s.discard(e)` (`Stmt::SetRemove`) — set
/// element removal in STATEMENT position. A set is a keys-only dict (16-byte
/// entries, a dummy value), so removal reuses the shared keyed removal helper
/// `$__wasm_dict_pop_<k>` (swap-last-into-hole + count--) exactly as `del d[k]`
/// ([`emit_dict_del`]) does, with the popped dummy value dropped — the removal,
/// not the value, is the point. Both mutate IN PLACE: the region only shrinks,
/// so the base pointer never moves and there is NO local write-back (unlike
/// [`emit_set_add`], which can 2x-grow).
///
/// The two Python semantics differ ONLY on an absent element:
/// * `s.remove(e)` (`error_if_absent`) lets the helper's not-found tail TRAP
///   (`unreachable`) — CPython `set.remove(missing)` raises `KeyError`, the
///   same analogue as `del d[missing]`.
/// * `s.discard(e)` (total) GATES the pop behind `$__wasm_dict_has_<k>` (which
///   never traps), so an absent element is a silent no-op — CPython
///   `set.discard(missing)` returns `None`. The element is emitted twice
///   (has + pop); for the literal/ident/allocating keys this subset admits that
///   is side-effect-free (a str `e` re-allocs a fresh heap copy, but `has`
///   compares by CONTENT via `$__wasm_str_eq`, so it still matches), exactly as
///   [`emit_dict_set_default`] re-emits its key against a content-compare `has`.
///
/// The element's [`KeyKind`] comes from the set local's declared type
/// (`heap_map_kind`), exactly as [`emit_set_add`] does — a non-set `name` is
/// refused. The `$__wasm_dict_pop_<k>` / `$__wasm_dict_has_<k>` helpers are
/// already emitted whenever the set is LET-bound ([`dict_helpers_for`] emits
/// get/has/set/pop as a unit, gated by [`module_dict_key_kinds`] off the
/// `Let` type), so this op declares no new helper.
fn emit_set_remove(
    set_name: &str,
    elem: &Expr,
    error_if_absent: bool,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    let kind = scope.heap_map_kind(set_name).ok_or_else(|| {
        unsupported(&format!(
            "`{set_name}.remove(e)`/`.discard(e)` over `{set_name}` which is not \
             a `set` local in the WASM subset"
        ))
    })?;
    let suffix = kind.suffix();
    if error_if_absent {
        // s.remove(e): pop-and-drop; the helper's not-found tail traps (KeyError).
        indent(out, depth);
        writeln!(out, "local.get ${set_name}").expect("write");
        emit_dict_key(elem, kind, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "call $__wasm_dict_pop_{suffix}").expect("write");
        indent(out, depth);
        writeln!(out, "drop").expect("write");
    } else {
        // s.discard(e): if has(s, e): pop-and-drop — an absent element is a no-op.
        indent(out, depth);
        writeln!(out, "local.get ${set_name}").expect("write");
        emit_dict_key(elem, kind, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "call $__wasm_dict_has_{suffix}").expect("write");
        indent(out, depth);
        writeln!(out, "if").expect("write");
        indent(out, depth + 1);
        writeln!(out, "local.get ${set_name}").expect("write");
        emit_dict_key(elem, kind, scope, out, depth + 1)?;
        indent(out, depth + 1);
        writeln!(out, "call $__wasm_dict_pop_{suffix}").expect("write");
        indent(out, depth + 1);
        writeln!(out, "drop").expect("write");
        indent(out, depth);
        writeln!(out, "end").expect("write");
    }
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
        // PMAT-1252: `sorted(xs)` / `sorted(xs, reverse=True)` over a
        // `list[int]` / `list[float]` — the FIRST list-VALUED op that
        // ALLOCATES: `$__wasm_list_sorted_{i64|f64}(base, reverse)` bump-allocates
        // a fresh record, copies `xs`, insertion-sorts the copy, and leaves the
        // new base-pointer (the source is never mutated). The destination `elem`
        // fixes the helper kind, cross-checked against `of_float`; the source
        // must be a NAMED list of the SAME element type.
        Expr::Sorted {
            list,
            reverse,
            key,
            of_float,
        } => emit_list_sorted(
            list,
            *reverse,
            key.as_ref(),
            *of_float,
            elem,
            scope,
            out,
            depth,
        ),
        // PMAT-1253: `reversed(xs)` / `list(reversed(xs))` / `xs[::-1]` over a
        // `list[int]` / `list[float]` — the SECOND allocating list-VALUED op.
        // `$__wasm_list_reversed_i64(base)` bump-allocates a fresh record and
        // copies `xs` back-to-front; ONE helper serves both int and float
        // (reversal moves 8-byte words verbatim, never interpreting them). The
        // source is never mutated (Python's `reversed`/`[::-1]` yields a new seq).
        Expr::Reversed { list } => emit_list_reversed(list, elem, scope, out, depth),
        // PMAT-1255: `a + b` over two `list[int]`/`list[float]` — the THIRD
        // allocating list-VALUED op. `$__wasm_list_concat_i64(a, b)`
        // bump-allocates a fresh record holding `a`'s then `b`'s elements and
        // leaves the new base-pointer; ONE helper serves both int and float
        // (concat moves 8-byte words verbatim). Neither operand is mutated
        // (Python's `a + b` yields a new list).
        Expr::ListConcat { lhs, rhs } => emit_list_concat(lhs, rhs, elem, scope, out, depth),
        // PMAT-1256: `xs[lo:hi]` over a `list[int]`/`list[float]` — the FOURTH
        // allocating list-VALUED op. `$__wasm_list_slice_i64(base, lo, hi)`
        // bump-allocates a fresh record holding the sub-list and leaves the new
        // base-pointer; ONE helper serves both int and float (slicing moves
        // 8-byte words verbatim). The source is never mutated (Python's
        // `xs[lo:hi]` yields a new list). A stepped slice / non-name list /
        // bool-or-mismatched-kind list refuses inside `emit_list_slice`.
        Expr::Slice {
            collection,
            lo,
            hi,
            of_str,
            step,
        } => emit_list_slice(collection, lo, hi, *of_str, *step, elem, scope, out, depth),
        // PMAT-1291: a bare `list(s)` over a set (`Expr::SetToList`) inherits the
        // set's ARBITRARY hash/storage order — observing it element-by-element
        // could diverge from CPython — so it is refused. The materialisation IS
        // supported, but ONLY as the source of `sorted(s)` (handled inside
        // `emit_list_sorted` via `emit_set_to_list`), where the re-sort makes the
        // order deterministic and CPython-exact.
        Expr::SetToList { .. } => Err(unsupported(
            "`list(s)` over a set — the WASM subset materialises a set to a list \
             only inside `sorted(s)` (which re-sorts to a deterministic order); a \
             bare `list(s)` inherits the set's arbitrary storage order and is \
             refused (use `sorted(s)`)",
        )),
        other => Err(unsupported(&format!(
            "binding a list local from {} — the WASM subset materialises a \
             list LITERAL, shares another named list local/param, sorts a \
             named list (`sorted(xs)`) or a set (`sorted(s)`), reverses one \
             (`reversed(xs)` / `xs[::-1]`), concatenates two named lists \
             (`a + b`), or slices one (`xs[lo:hi]`); other list-returning calls \
             are refused",
            expr_kind(other)
        ))),
    }
}

/// PMAT-1256: lower `xs[lo:hi]` over a `list[int]`/`list[float]` producing a NEW
/// `list[scalar]` on the bump heap. Leaves the fresh record's `i32` base-pointer
/// on the stack.
///
/// `elem` is the DESTINATION list's element type (from the bound name); it must be
/// I64 (int) or F64 (float) — a `list[bool]` (I32 4-byte stride) is REFUSED for
/// parity with `sorted`/`reversed`/`concat` (the one 8-byte-word helper cannot
/// serve a 4-byte stride). The source must be a NAMED `list[scalar]` local/param
/// of exactly `elem` (a non-name list, or a kind mismatch, refuses honestly). A
/// STEPPED slice (`xs[i:j:k]`, incl. the `xs[::-1]` reverse idiom which lowers via
/// `Expr::Reversed`, not here) is refused. `of_str` must be false (a string slice
/// lowers via `emit_str_slice`). A missing `lo` defaults to `0` and a missing `hi`
/// to `i64::MAX` (the helper clamps both into `[0, n]`), so `xs[:]` / `xs[a:]` /
/// `xs[:b]` all lower.
#[allow(clippy::too_many_arguments)]
fn emit_list_slice(
    collection: &Expr,
    lo: &Option<Box<Expr>>,
    hi: &Option<Box<Expr>>,
    of_str: bool,
    step: Option<i64>,
    elem: WatTy,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    if of_str {
        // Defensive: a `str` slice never reaches the list-valued path (it binds a
        // str local, routed through `emit_str_expr` → `emit_str_slice`). Refuse
        // rather than emit the list helper against a string base.
        return Err(unsupported(
            "a STRING slice bound as a list — internal routing error; a `str` \
             slice lowers via the string path",
        ));
    }
    if step.is_some() {
        return Err(unsupported(
            "a STEPPED list slice `xs[i:j:k]` — the WASM list subset slices \
             `xs[lo:hi]` (step 1) only; the `xs[::-1]` reverse idiom lowers via \
             `reversed`, refused here",
        ));
    }
    // The destination element type fixes the (single) helper; a bool list (I32
    // 4-byte stride) or any non-scalar refuses — never a silent stride misread.
    if !matches!(elem, WatTy::I64 | WatTy::F64) {
        return Err(unsupported(&format!(
            "`xs[lo:hi]` into a `list[{}]` — the WASM subset slices `list[int]` \
             / `list[float]` (8-byte stride) only (a bool/other-kind list is \
             refused)",
            elem.keyword()
        )));
    }
    let Expr::Ident(src) = collection else {
        return Err(unsupported(
            "`…[lo:hi]` over a non-name list — the WASM subset slices a named \
             `list[scalar]` local/param (bind the list to a name first)",
        ));
    };
    let Some(src_elem) = scope.list_elem_of(src) else {
        return Err(unsupported(&format!(
            "`{src}[lo:hi]` where `{src}` is not a `list[scalar]` local/param in \
             the WASM subset"
        )));
    };
    if src_elem != elem {
        return Err(unsupported(&format!(
            "`{src}[lo:hi]` slices a `list[{}]` into a `list[{}]` — the WASM \
             subset keeps the element type (slice a list into a list of the same \
             element type)",
            src_elem.keyword(),
            elem.keyword()
        )));
    }
    // base pointer of the source list.
    indent(out, depth);
    writeln!(out, "local.get ${src}").expect("write");
    // lo (i64 element index) — a missing `lo` defaults to 0.
    match lo {
        Some(b) => emit_expr_typed(b, scope, out, depth, WatTy::I64)?,
        None => {
            indent(out, depth);
            writeln!(out, "i64.const 0").expect("write");
        }
    }
    // hi (i64 element index) — a missing `hi` defaults to i64::MAX, which the
    // helper clamps down to the list's element count.
    match hi {
        Some(b) => emit_expr_typed(b, scope, out, depth, WatTy::I64)?,
        None => {
            indent(out, depth);
            writeln!(out, "i64.const 9223372036854775807").expect("write");
        }
    }
    indent(out, depth);
    writeln!(out, "call $__wasm_list_slice_i64").expect("write");
    Ok(())
}

/// PMAT-1252: lower `sorted(list[, reverse])` producing a NEW `list[scalar]`
/// on the bump heap. Leaves the fresh record's `i32` base-pointer on the stack.
///
/// `elem` is the DESTINATION list's element type (from the bound name), and it
/// drives the helper kind (`WatTy::I64` → `$__wasm_list_sorted_i64`, `WatTy::F64`
/// → `_f64`) — cross-checked against the frontend's `of_float` tag so the emit
/// and the [`module_uses_list_sorted`] gate (which keys on `of_float`) always
/// pick the SAME helper (a disagreement would emit a call to an ungated,
/// undeclared helper — a hard wat2wasm failure). The source must be a NAMED
/// `list[scalar]` of exactly `elem` (a non-name list, or a bool/mismatched-kind
/// list, refuses honestly). `key=` sorting is deferred.
#[allow(clippy::too_many_arguments)]
fn emit_list_sorted(
    list: &Expr,
    reverse: bool,
    key: Option<&SortKey>,
    of_float: bool,
    elem: WatTy,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    if key.is_some() {
        return Err(unsupported(
            "`sorted(xs, key=…)` — the WASM subset sorts by element value only \
             (a `key=` sort is deferred)",
        ));
    }
    // The destination element type fixes the helper; it must agree with the
    // frontend's `of_float` tag (F64 ⇔ float, I64 ⇔ int). A bool list (I32) or a
    // kind disagreement refuses — never a silent bit-misread.
    let helper = match (elem, of_float) {
        (WatTy::I64, false) => "__wasm_list_sorted_i64",
        (WatTy::F64, true) => "__wasm_list_sorted_f64",
        _ => {
            return Err(unsupported(&format!(
                "`sorted(xs)` over a `list[{}]` — the WASM subset sorts \
                 `list[int]` / `list[float]` only (a bool/other-kind list, or a \
                 float/int tag mismatch, is refused)",
                elem.keyword()
            )));
        }
    };
    // Push the SOURCE list's base-pointer. Two shapes are supported:
    //   * `sorted(xs)` — a NAMED `list[scalar]` local/param: `local.get $xs`.
    //   * `sorted(s)` over a `set[int]` — the frontend lowers this to
    //     `Sorted { list: SetToList { set } }` (PMAT-520). The set is
    //     materialised to a FRESH `list[int]` on the heap (its keys, dup-free)
    //     via `$__wasm_set_to_list_i64`, leaving that record's base on the stack;
    //     the sort helper then copies-and-sorts it into the final result. Because
    //     the result is ALWAYS sorted, the set's arbitrary storage order is
    //     irrelevant → CPython-exact (PMAT-1291).
    match list {
        Expr::Ident(src) => {
            let Some(src_elem) = scope.list_elem_of(src) else {
                return Err(unsupported(&format!(
                    "`sorted({src})` where `{src}` is not a `list[scalar]` \
                     local/param in the WASM subset"
                )));
            };
            if src_elem != elem {
                return Err(unsupported(&format!(
                    "`sorted({src})` sorts a `list[{}]` into a `list[{}]` — the \
                     WASM subset keeps the element type (sort a list into a list \
                     of the same element type)",
                    src_elem.keyword(),
                    elem.keyword()
                )));
            }
            indent(out, depth);
            writeln!(out, "local.get ${src}").expect("write");
        }
        // PMAT-1291: `sorted(s)` over a `set[int]` — materialise the set to a
        // fresh `list[int]` (leaves its base on the stack), then sort a copy.
        Expr::SetToList { set } => emit_set_to_list(set, elem, scope, out, depth)?,
        _ => {
            return Err(unsupported(
                "`sorted(…)` over a non-name list — the WASM subset sorts a \
                 named `list[scalar]` local/param or a `set[int]` (`sorted(s)`); \
                 bind other list-returning values to a name first",
            ));
        }
    }
    // Push the reverse selector, then call the helper; it leaves the fresh
    // sorted record's i32 base-pointer on the stack.
    indent(out, depth);
    writeln!(out, "i32.const {}", i32::from(reverse)).expect("write");
    indent(out, depth);
    writeln!(out, "call ${helper}").expect("write");
    Ok(())
}

/// PMAT-1291: lower the `set → list[int]` materialisation that is the source of
/// `sorted(s)` over a `set[int]` (the frontend `Expr::SetToList { set }` inside
/// `Expr::Sorted`, PMAT-520). Emits `local.get $set` then a call to
/// `$__wasm_set_to_list_i64`, leaving a FRESH `list[int]` record's `i32`
/// base-pointer on the stack (the caller — [`emit_list_sorted`] — then sorts a
/// copy of it).
///
/// The destination `elem` is the sorted result's element type; it must be I64
/// (an `int` set → a `list[int]`). A `set[str]` would materialise a `list[str]`,
/// which the WASM list subset does not model — that case is already refused
/// upstream when the destination `list[str]` local is typed
/// ([`map_list_elem_type`]), and refused defensively here too. The `set` operand
/// must be a NAMED `set[int]` local (a non-name / non-set / str-set source is a
/// hard `BackendError`, never a silent base-pointer misread).
fn emit_set_to_list(
    set: &Expr,
    elem: WatTy,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // The result of `sorted(s)` over an int set is a `list[int]` (i64 stride);
    // the materialisation helper is int-only (a str set → `list[str]`, unmodelled).
    if elem != WatTy::I64 {
        return Err(unsupported(&format!(
            "`sorted(s)` over a set into a `list[{}]` — the WASM subset \
             materialises an `int` set (`set[int]` → `list[int]`) only; a str \
             set (→ `list[str]`) is unmodelled",
            elem.keyword()
        )));
    }
    let Expr::Ident(sname) = set else {
        return Err(unsupported(
            "`sorted(set(...))` over a non-name set — the WASM subset sorts a \
             NAMED `set[int]` local (`sorted(s)`); bind the set to a name first",
        ));
    };
    if !scope.is_set(sname) {
        return Err(unsupported(&format!(
            "`sorted({sname})` where `{sname}` is not a `set` local in the WASM \
             subset"
        )));
    }
    // A set materialised to a `list[int]` must be an INT set (i64 keys); a str
    // set (`set_elem_of` → I32 str-pointer) would need a `list[str]`, refused.
    match scope.set_elem_of(sname) {
        Some(WatTy::I64) => {}
        _ => {
            return Err(unsupported(&format!(
                "`sorted({sname})` over a non-int set — the WASM subset sorts a \
                 `set[int]` into a `list[int]` only (a str set → `list[str]` is \
                 unmodelled)"
            )));
        }
    }
    indent(out, depth);
    writeln!(out, "local.get ${sname}").expect("write");
    indent(out, depth);
    writeln!(out, "call $__wasm_set_to_list_i64").expect("write");
    Ok(())
}

/// PMAT-1253: lower `reversed(list)` / `list(reversed(list))` / `list[::-1]`
/// producing a NEW `list[scalar]` on the bump heap. Leaves the fresh record's
/// `i32` base-pointer on the stack.
///
/// `elem` is the DESTINATION list's element type (from the bound name). Reversal
/// is a verbatim 8-byte-word move that NEVER interprets element values, so ONE
/// helper (`$__wasm_list_reversed_i64`) serves BOTH `list[int]` (I64) and
/// `list[float]` (F64) — unlike [`emit_list_sorted`], whose typed compares force
/// two helpers. A `list[bool]` (I32, 4-byte stride) is refused for parity with
/// `sorted` (a distinct-stride helper is deferred). The source must be a NAMED
/// `list[scalar]` of exactly `elem`; the `reversed(s)` STR form
/// (`Reversed(StrChars(s))`, a `list[str]`) is refused here — it is not a scalar
/// list and has no supported WASM local.
fn emit_list_reversed(
    list: &Expr,
    elem: WatTy,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // Int and float share the one 8-byte-word helper; a bool list (4-byte i32
    // stride) is refused — a distinct-stride reverse helper is deferred.
    if !matches!(elem, WatTy::I64 | WatTy::F64) {
        return Err(unsupported(&format!(
            "`reversed(xs)` / `xs[::-1]` over a `list[{}]` — the WASM subset \
             reverses `list[int]` / `list[float]` (8-byte stride) only (a \
             bool/other-kind list is refused)",
            elem.keyword()
        )));
    }
    let Expr::Ident(src) = list else {
        return Err(unsupported(
            "`reversed(…)` over a non-name list — the WASM subset reverses a \
             named `list[scalar]` local/param (bind the list to a name first)",
        ));
    };
    let Some(src_elem) = scope.list_elem_of(src) else {
        return Err(unsupported(&format!(
            "`reversed({src})` where `{src}` is not a `list[scalar]` local/param \
             in the WASM subset"
        )));
    };
    if src_elem != elem {
        return Err(unsupported(&format!(
            "`reversed({src})` reverses a `list[{}]` into a `list[{}]` — the WASM \
             subset keeps the element type (reverse a list into a list of the \
             same element type)",
            src_elem.keyword(),
            elem.keyword()
        )));
    }
    // Push the source base-pointer, then call the single 8-byte-word helper; it
    // leaves the fresh reversed record's i32 base-pointer on the stack.
    indent(out, depth);
    writeln!(out, "local.get ${src}").expect("write");
    indent(out, depth);
    writeln!(out, "call $__wasm_list_reversed_i64").expect("write");
    Ok(())
}

/// PMAT-1255: lower `a + b` over two `list[scalar]` (`Expr::ListConcat`) — the
/// THIRD allocating list-VALUED op. Pushes `a`'s then `b`'s base-pointer and
/// calls `$__wasm_list_concat_i64`, which bump-allocates a fresh record holding
/// `a`'s elements followed by `b`'s and leaves ITS base-pointer on the stack.
///
/// `elem` is the DESTINATION list's element type (from the bound name).
/// Concatenation is a verbatim 8-byte-word move that NEVER interprets element
/// values, so ONE helper (`$__wasm_list_concat_i64`) serves BOTH `list[int]`
/// (I64) and `list[float]` (F64) — mirroring [`emit_list_reversed`]. A
/// `list[bool]` (I32, 4-byte stride) is refused for parity with `sorted`/
/// `reversed`.
///
/// PMAT-1259: each operand is any list-VALUED expression of exactly `elem`,
/// lowered through [`emit_list_expr`] — so beyond a bare NAMED list it now
/// accepts a `list` LITERAL (`xs + [3, 1, 2]`), a nested concat
/// (`a + b + c` = `(a + b) + c`), and the other allocating list-valued ops
/// (`sorted(xs) + ys`, `reversed(xs) + ys`, `xs[1:] + ys`). This is SAFE
/// despite the operands allocating: [`emit_list_expr`] leaves each operand's
/// base-pointer on the WASM OPERAND STACK, and a later operand's fresh
/// bump-allocation only GROWS the heap (never touching an earlier operand's
/// record or the operand-stack pointer to it), so evaluating `b` after `a`'s
/// pointer is already stacked cannot invalidate it — the same discipline that
/// makes `[1] + [2]` (two `emit_list_lit` records) correct. A non-list operand
/// (never produced by the frontend for `ListConcat`) refuses honestly inside
/// [`emit_list_expr`]; an element-type mismatch (`list[int] + list[float]`)
/// refuses there too (each operand is lowered AS `elem`).
fn emit_list_concat(
    lhs: &Expr,
    rhs: &Expr,
    elem: WatTy,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    // Int and float share the one 8-byte-word helper; a bool list (4-byte i32
    // stride) is refused — a distinct-stride concat helper is deferred.
    if !matches!(elem, WatTy::I64 | WatTy::F64) {
        return Err(unsupported(&format!(
            "`a + b` list concatenation over a `list[{}]` — the WASM subset \
             concatenates `list[int]` / `list[float]` (8-byte stride) only (a \
             bool/other-kind list is refused)",
            elem.keyword()
        )));
    }
    // Push `a`'s then `b`'s base-pointer (helper params $a, $b) — each operand
    // is any list-valued expr of `elem`, lowered through the shared
    // `emit_list_expr` dispatcher — then the single 8-byte-word helper; it
    // leaves the fresh concatenated record's i32 base-pointer.
    emit_list_expr(lhs, elem, scope, out, depth)?;
    emit_list_expr(rhs, elem, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_list_concat_i64").expect("write");
    Ok(())
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
    // PMAT-1276: over-allocate `LIST_GROWTH_SLACK` spare slots past the literal
    // entries so a later `xs.append(v)` has room in the realloc-free bump heap;
    // record that fixed capacity at base+4. `capacity` cannot overflow i32 for
    // any list a WASM module realistically emits (n is already bounded above).
    let cap = n.saturating_add(LIST_GROWTH_SLACK);
    let size = LIST_ELEMS_OFFSET + cap * elem.byte_size();
    // dst = __alloc(8 + (n + slack)*elem_size)
    indent(out, depth);
    writeln!(out, "i32.const {size}").expect("write");
    indent(out, depth);
    writeln!(out, "call $__alloc").expect("write");
    indent(out, depth);
    writeln!(out, "local.set ${LIST_DST_SCRATCH}").expect("write");
    // Header: the i32 live-element count at base+0 …
    indent(out, depth);
    writeln!(out, "local.get ${LIST_DST_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {n}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store").expect("write");
    // … and the fixed slot-capacity at base+4 (`append` bounds writes to it).
    indent(out, depth);
    writeln!(out, "local.get ${LIST_DST_SCRATCH}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.const {cap}").expect("write");
    indent(out, depth);
    writeln!(out, "i32.store offset={LIST_CAP_OFFSET}").expect("write");
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

/// PMAT-1242: `true` if `e` is a SET-valued binop operand — a LET-bound `set`
/// local `Ident`. Like a struct (and a str) a set rides an i32 base-pointer
/// indistinguishable from a bool/int i32 in the opcode table, so a naive
/// `s1 == s2` would silently compare BASE-POINTERS — while Python `==` over
/// sets is STRUCTURAL (membership + size). `emit_binop` intercepts on this to
/// route equality to `$__wasm_set_eq_<k>`. A `SetLit` operand is deliberately
/// NOT matched: it has no base-pointer local, so `{1} == s` is refused honestly
/// (bind the literal to a name first).
fn binop_operand_is_set(e: &Expr, scope: &Scope) -> bool {
    matches!(e, Expr::Ident(name) if scope.is_set(name))
}

/// PMAT-1242/1243: `true` if `e` is a DICT-valued binop operand — a LET-bound
/// heap map `Ident` that is NOT a set. Like a set it rides an i32 base-pointer,
/// so a naive `d1 == d2` would silently compare BASE-POINTERS while Python `==`
/// over dicts is STRUCTURAL (keys AND values). `emit_binop` intercepts on this
/// to route equality to `$__wasm_dict_eq_<k>` (size check + per-key membership +
/// i64 value compare); ordering and dict algebra stay refused honestly.
fn binop_operand_is_dict(e: &Expr, scope: &Scope) -> bool {
    matches!(e, Expr::Ident(name)
        if scope.heap_map_kind(name).is_some() && !scope.is_set(name))
}

/// PMAT-1245: `Expr::SetPred` — Python set ordering `a <= b` / `a < b` / `a >= b`
/// / `a > b` (subset / proper-subset / superset / proper-superset). The FRONTEND
/// lowers set comparison operators to `SetPred` (never `BinOp`), so this — NOT
/// the `emit_binop` set-ordering arm (PMAT-1244, reachable only from a hand-built
/// meta-HIR or another frontend) — is the path that makes set ordering reachable
/// from Python source. It reuses the `$__wasm_set_subset_<k>(sub,sup)->i32`
/// membership helper PMAT-1244 already emits:
///
/// ```text
///   a <= b  ⇔  subset(a, b)                 (non-strict subset)
///   a >= b  ⇔  subset(b, a)                 (containment flipped)
///   a <  b  ⇔  subset(a, b) ∧ |a| < |b|     (PROPER subset — inline size AND)
///   a >  b  ⇔  subset(b, a) ∧ |b| < |a|
/// ```
///
/// The strict variants AND on an inline header size compare (a subset of unequal
/// size is a proper subset, since `p ⊆ q ⟹ |p| ≤ |q|` for sets). Both operands
/// are set Idents (the WASM subset only orders set locals), so re-emitting one
/// for the size reload is a pure `local.get`. PMAT-1246: `isdisjoint`
/// (`Disjoint`) routes to its own `$__wasm_set_disjoint_<k>` helper — the DUAL
/// walk (return 0 on any SHARED key), no size gate, no operand swap.
fn emit_set_pred(
    lhs: &Expr,
    op: SetPredOp,
    rhs: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    // Both operands must be set NAMES of the SAME key kind — a set only orders
    // against a set, and comparing two entry regions needs a shared key encoding.
    let (Expr::Ident(ln), Expr::Ident(rn)) = (lhs, rhs) else {
        return Err(unsupported(
            "set predicate with a non-name operand — set ordering needs both \
             sides bound to a name; bind a set literal or other set-valued \
             expression to a local first",
        ));
    };
    if !(scope.is_set(ln) && scope.is_set(rn)) {
        return Err(unsupported(
            "set predicate mixing a `set` operand with a non-`set` operand — a \
             set only ever orders against another set; refused honestly rather \
             than comparing base-pointers",
        ));
    }
    let lk = scope.heap_map_kind(ln).expect("a set local has a key kind");
    let rk = scope.heap_map_kind(rn).expect("a set local has a key kind");
    if lk != rk {
        return Err(unsupported(&format!(
            "set predicate over sets with different key kinds ({} vs {}) — a \
             set[int] and a set[str] have no subset relation; refused honestly",
            lk.suffix(),
            rk.suffix()
        )));
    }
    let sfx = lk.suffix();
    // PMAT-1246: `a.isdisjoint(b)` — the no-common-element predicate. Its own
    // helper (return 0 on ANY shared key, the DUAL of subset's return-0-on-any-
    // absent-key walk), NOT the subset helper: disjoint has no cardinality
    // relation, so there is no size-gate or operand-swap to share. Symmetric, so
    // walk lhs vs rhs directly.
    if matches!(op, SetPredOp::Disjoint) {
        emit_expr(lhs, scope, out, depth)?;
        emit_expr(rhs, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "call $__wasm_set_disjoint_{sfx}").expect("write");
        return Ok(WatTy::I32);
    }
    // (sub, sup) = the (⊆-inner, ⊇-outer) operands for THIS predicate — subset
    // asks a ⊆ b; superset flips the containment to b ⊆ a.
    let (sub, sup) = match op {
        SetPredOp::Subset | SetPredOp::ProperSubset => (lhs, rhs),
        SetPredOp::Superset | SetPredOp::ProperSuperset => (rhs, lhs),
        SetPredOp::Disjoint => unreachable!("Disjoint handled above"),
    };
    emit_expr(sub, scope, out, depth)?;
    emit_expr(sup, scope, out, depth)?;
    indent(out, depth);
    writeln!(out, "call $__wasm_set_subset_{sfx}").expect("write");
    // Strict `<`/`>` also require |sub| < |sup| (proper subset).
    if matches!(op, SetPredOp::ProperSubset | SetPredOp::ProperSuperset) {
        emit_expr(sub, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "i32.load").expect("write"); // |sub| (size header @ +0)
        emit_expr(sup, scope, out, depth)?;
        indent(out, depth);
        writeln!(out, "i32.load").expect("write"); // |sup|
        indent(out, depth);
        writeln!(out, "i32.lt_s").expect("write"); // |sub| < |sup|
        indent(out, depth);
        writeln!(out, "i32.and").expect("write"); // (sub ⊆ sup) ∧ proper
    }
    Ok(WatTy::I32)
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

    // PMAT-1242: a SET operand rides an i32 base-pointer, indistinguishable
    // from a bool/int i32 in the opcode table below — so a naive `s1 == s2`
    // would silently compare BASE-POINTERS (two structurally-equal sets built
    // from different literals get distinct heap addresses → wrongly `!=`),
    // while Python `==` over sets is STRUCTURAL. Route equality to the real
    // membership helper `$__wasm_set_eq_<k>`; refuse the set ops that are not
    // wired (ordering = subset/superset, algebra = union/…) rather than let the
    // fall-through compare pointers.
    if binop_operand_is_set(lhs, scope) || binop_operand_is_set(rhs, scope) {
        // Both operands must be set NAMES of the SAME key kind — a set only
        // equals another set (`{1} == 1` and `{1} == {"a"}` are False, never
        // equal), and comparing two entry regions needs a shared key encoding.
        // A set-literal / non-name / non-set / mixed-kind operand is refused.
        let (Expr::Ident(ln), Expr::Ident(rn)) = (lhs, rhs) else {
            return Err(unsupported(&format!(
                "binary op {op:?} with a `set` operand and a non-name operand — \
                 set comparison needs both sides bound to a name; bind a set \
                 literal or other set-valued expression to a local first"
            )));
        };
        if !(scope.is_set(ln) && scope.is_set(rn)) {
            return Err(unsupported(&format!(
                "binary op {op:?} mixing a `set` operand with a non-`set` \
                 operand — a set only ever equals another set; refused honestly \
                 rather than comparing base-pointers"
            )));
        }
        let lk = scope.heap_map_kind(ln).expect("a set local has a key kind");
        let rk = scope.heap_map_kind(rn).expect("a set local has a key kind");
        if lk != rk {
            return Err(unsupported(&format!(
                "binary op {op:?} over sets with different key kinds ({} vs {}) \
                 — a set[int] and a set[str] can never be equal; refused honestly",
                lk.suffix(),
                rk.suffix()
            )));
        }
        if matches!(op, BinOp::Eq | BinOp::NotEq) {
            // s1 == s2 ⇔ |s1| == |s2| AND s1 ⊆ s2 — the helper returns an
            // i32 bool. Push both base-pointers, call, invert for `!=`.
            emit_expr(lhs, scope, out, depth)?;
            emit_expr(rhs, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_set_eq_{}", lk.suffix()).expect("write");
            if matches!(op, BinOp::NotEq) {
                indent(out, depth);
                writeln!(out, "i32.eqz").expect("write"); // != is !(==)
            }
            return Ok(WatTy::I32);
        }
        // PMAT-1244: set ORDERING — subset/superset. Python:
        //   `p <= q` ⇔ p ⊆ q            `p >= q` ⇔ q ⊆ p (operands swapped)
        //   `p <  q` ⇔ p ⊆ q ∧ |p|<|q|  `p >  q` ⇔ q ⊆ p ∧ |q|<|p|
        // `$__wasm_set_subset_<k>(a,b)` returns (a ⊆ b); the strict variants AND
        // on an inline header size compare (a subset of unequal size is a PROPER
        // subset, since p ⊆ q ⟹ |p| ≤ |q| for sets). Both operands are set
        // Idents (guarded above), so re-emitting one is a pure `local.get` — no
        // double side effect from the size reloads.
        if matches!(op, BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq) {
            let sfx = lk.suffix();
            // (sub, sup) = the (⊆-inner, ⊇-outer) operands for THIS op — `<=`/`<`
            // ask lhs ⊆ rhs; `>=`/`>` flip the containment to rhs ⊆ lhs.
            let (sub, sup) = match op {
                BinOp::LtEq | BinOp::Lt => (lhs, rhs),
                BinOp::GtEq | BinOp::Gt => (rhs, lhs),
                _ => unreachable!("guarded by the matches! above"),
            };
            emit_expr(sub, scope, out, depth)?;
            emit_expr(sup, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_set_subset_{sfx}").expect("write");
            // Strict `<`/`>` also require |sub| < |sup| (proper subset).
            if matches!(op, BinOp::Lt | BinOp::Gt) {
                emit_expr(sub, scope, out, depth)?;
                indent(out, depth);
                writeln!(out, "i32.load").expect("write"); // |sub| (size header @ +0)
                emit_expr(sup, scope, out, depth)?;
                indent(out, depth);
                writeln!(out, "i32.load").expect("write"); // |sup|
                indent(out, depth);
                writeln!(out, "i32.lt_s").expect("write"); // |sub| < |sup|
                indent(out, depth);
                writeln!(out, "i32.and").expect("write"); // (sub ⊆ sup) ∧ proper
            }
            return Ok(WatTy::I32);
        }
        return Err(unsupported(&format!(
            "binary op {op:?} over `set` operands — structural equality `==`/`!=` \
             (PMAT-1242) and subset/superset ordering `<`/`<=`/`>`/`>=` \
             (PMAT-1244) are wired; set algebra (union/intersection/difference) \
             is not yet in the WASM set subset, refused honestly"
        )));
    }

    // PMAT-1243: a DICT operand rides an i32 base-pointer just like a set, so a
    // naive `d1 == d2` would silently compare BASE-POINTERS while Python `==`
    // over dicts is STRUCTURAL (keys AND values). Route equality to the real
    // `$__wasm_dict_eq_<k>` helper (size check + per-key membership + i64 value
    // compare); refuse the dict ops that are not wired (ordering, algebra)
    // rather than let the fall-through compare pointers. (`k in d` is
    // `Expr::DictContains`, not a binop, so it never reaches here.)
    if binop_operand_is_dict(lhs, scope) || binop_operand_is_dict(rhs, scope) {
        // Both operands must be dict NAMES of the SAME key kind — a dict only
        // equals another dict (`{1:2} == 1` and `{1:2} == {"a":2}` are False),
        // and comparing two entry regions needs a shared key encoding. A
        // dict-literal / non-name / non-dict / mixed-kind operand is refused.
        // (The set branch above already caught any set operand, so here neither
        // side is a set.)
        let (Expr::Ident(ln), Expr::Ident(rn)) = (lhs, rhs) else {
            return Err(unsupported(&format!(
                "binary op {op:?} with a `dict` operand and a non-name operand — \
                 dict comparison needs both sides bound to a name; bind a dict \
                 literal or other dict-valued expression to a local first"
            )));
        };
        let l_is_dict = scope.heap_map_kind(ln).is_some() && !scope.is_set(ln);
        let r_is_dict = scope.heap_map_kind(rn).is_some() && !scope.is_set(rn);
        if !(l_is_dict && r_is_dict) {
            return Err(unsupported(&format!(
                "binary op {op:?} mixing a `dict` operand with a non-`dict` \
                 operand — a dict only ever equals another dict; refused \
                 honestly rather than comparing base-pointers"
            )));
        }
        let lk = scope
            .heap_map_kind(ln)
            .expect("a dict local has a key kind");
        let rk = scope
            .heap_map_kind(rn)
            .expect("a dict local has a key kind");
        if lk != rk {
            return Err(unsupported(&format!(
                "binary op {op:?} over dicts with different key kinds ({} vs {}) \
                 — a dict[int,_] and a dict[str,_] can never be equal; refused \
                 honestly",
                lk.suffix(),
                rk.suffix()
            )));
        }
        if matches!(op, BinOp::Eq | BinOp::NotEq) {
            // d1 == d2 ⇔ |d1| == |d2| AND ∀k∈d1: k∈d2 ∧ d1[k]==d2[k] — the
            // helper returns an i32 bool. Push both base-pointers, call, invert
            // for `!=`.
            emit_expr(lhs, scope, out, depth)?;
            emit_expr(rhs, scope, out, depth)?;
            indent(out, depth);
            writeln!(out, "call $__wasm_dict_eq_{}", lk.suffix()).expect("write");
            if matches!(op, BinOp::NotEq) {
                indent(out, depth);
                writeln!(out, "i32.eqz").expect("write"); // != is !(==)
            }
            return Ok(WatTy::I32);
        }
        return Err(unsupported(&format!(
            "binary op {op:?} over `dict` operands — only structural equality \
             `==`/`!=` is wired (PMAT-1243); ordering and dict algebra are not \
             in the WASM dict subset, refused honestly"
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
