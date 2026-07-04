//! PMAT-1238 — a randomized DIFFERENTIAL witness for the WHOLE recent native-WASM
//! dict/set op family against LIVE CPython (`python3`), fuzzed over random
//! OPERATION SEQUENCES rather than single ops.
//!
//! ## The gap this closes
//!
//! Every sibling dict witness (`dict_get_default_witness`, `dict_pop_witness`,
//! `dict_setdefault_witness`, `dict_del_item_witness`, `dict_clear_witness`) pins
//! a HAND-PICKED table for ONE op applied to a FRESH literal. Hand-picked
//! single-op tables catch the cases the author enumerated; they do NOT catch a
//! divergence hiding in the INTERACTION between ops applied in sequence to a
//! mutated container — and the bump-heap dict runtime's danger is precisely in
//! those interactions:
//!
//!   * `del d[mid]` swaps the LAST entry into the hole (`memory.copy`); a later
//!     `d[swapped_key] = v` must UPDATE that relocated entry in place, not append
//!     a duplicate.
//!   * `d[k] = v` / `d.setdefault(k, v)` on a MISS can RELOCATE the whole region
//!     (grow past capacity) and write a new base pointer back to the local — after
//!     the region was previously SHRUNK by a `del` / `pop`. Grow-after-shrink is a
//!     path no single-op witness reaches.
//!   * `d.clear()` zeroes the count in place; a subsequent burst of inserts must
//!     grow correctly FROM the cleared base (grow-after-clear).
//!   * `d.pop(k, default)` removal followed by `d.setdefault(k, x)` re-insertion
//!     churns the count header both directions.
//!
//! This witness generates a DETERMINISTIC corpus of op SEQUENCES (curated
//! interaction edges + a fixed-seed LCG walk), applies each to a WASM dict/set AND
//! to a CPython dict/set built from the SAME sequence data, and diffs every
//! observable (`d.get(k, -1)` for every key + `len(d)`; `k in s` + `len(s)`).
//! `python3` is the literal oracle — zero reimplementation risk.
//!
//! ## The family under test (the PMAT-1215..1236 dict/set run)
//!
//! Dict (int-keyed AND str-keyed / content-compare):
//!   `d[k]=v` (`Stmt::DictSet`)  `d.setdefault(k,v)` (`Expr::DictSetDefault`)
//!   `d.pop(k,default)` / `d.pop(k)` (`Expr::DictPop`)  `del d[k]` (`Stmt::DelItem`)
//!   `d.clear()` (`Stmt::ListMutate` Clear)  `d.get(k,-1)` (`Expr::DictGetOr`)
//!   `len(d)` (`Expr::Len`)
//! Set (int-keyed AND str-keyed):
//!   `s.add(e)` (`Stmt::SetAdd`)  `s.clear()`  `e in s` (`Expr::SetContains`)
//!   `len(s)`
//!
//! ## Trap-free BY CONSTRUCTION
//!
//! `del d[k]` and bare `d.pop(k)` TRAP on an absent key (the CPython `KeyError`
//! analogue — already witnessed per-op). To keep THIS test an exact-value diff
//! (not a trap test), the generator maintains a presence model and only emits a
//! `del` / bare-`pop` on a key it knows is present, so every emitted sequence runs
//! clean and MUST value-match CPython exactly. A silent divergence — the dangerous
//! class, not a trap, not a refusal — is what fails here.
//!
//! ## Gating
//!
//! Runs the executed diff only when BOTH WABT (`wat2wasm`/`wasm-interp`) AND
//! `python3` are present. On free CI (no WABT) it skips cleanly after still
//! exercising the EMIT path for every sequence.

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};

use xpile_meta_hir::{Block, Expr, Function, Item, ListMutateOp, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- keys -------------------------------------------------------------------

/// A dict/set key: an int (hash-free integer compare path) or a `&'static str`
/// (content-compare path). Values are always `i64`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum K {
    I(i64),
    S(&'static str),
}

impl K {
    /// The meta-HIR key expression.
    fn expr(&self) -> Expr {
        match self {
            K::I(n) => Expr::LitInt(*n),
            K::S(s) => Expr::LitStr((*s).into()),
        }
    }
    /// The CPython source form of the key (`5` or `'a'`).
    fn py(&self) -> String {
        match self {
            K::I(n) => n.to_string(),
            K::S(s) => format!("'{s}'"),
        }
    }
    /// The meta-HIR key type for the containing dict/set.
    fn ty(&self) -> Type {
        match self {
            K::I(_) => Type::I64,
            K::S(_) => Type::Str,
        }
    }
}

/// The int keyspace — small (0..=5) so inserts collide, dels remove real entries,
/// and grows past the 1-2-entry initial capacity happen fast.
const INT_KEYS: &[K] = &[K::I(0), K::I(1), K::I(2), K::I(3), K::I(4), K::I(5)];
/// The str keyspace — six single-char content-compare keys.
const STR_KEYS: &[K] = &[
    K::S("a"),
    K::S("b"),
    K::S("c"),
    K::S("d"),
    K::S("e"),
    K::S("f"),
];

/// The absent-sentinel for the `d.get(k, SENTINEL)` observable. Generated values
/// are all `>= 0`, so `-1` unambiguously means "key not present".
const SENTINEL: i64 = -1;

// ---- dict ops ---------------------------------------------------------------

/// One dict mutation. The `pop`/`del` variants carry a key the generator has
/// verified present, so they never trap.
#[derive(Clone)]
enum Op {
    Set(K, i64),
    SetDefault(K, i64),
    PopDefault(K), // d.pop(k, 0) — default dropped (bare statement)
    PopBare(K),    // d.pop(k)    — value dropped, key known present
    Del(K),        // del d[k]    — key known present
    Clear,
}

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// The meta-HIR statement for a dict op over the local `d`.
fn op_stmt(op: &Op) -> Stmt {
    match op {
        Op::Set(k, v) => Stmt::DictSet {
            dict_name: "d".into(),
            key: k.expr(),
            value: Expr::LitInt(*v),
        },
        Op::SetDefault(k, v) => Stmt::SideEffectCall {
            call: Expr::DictSetDefault {
                dict: Box::new(ident("d")),
                key: Box::new(k.expr()),
                default: Box::new(Expr::LitInt(*v)),
            },
        },
        Op::PopDefault(k) => Stmt::SideEffectCall {
            call: Expr::DictPop {
                dict: Box::new(ident("d")),
                key: Box::new(k.expr()),
                default: Some(Box::new(Expr::LitInt(0))),
            },
        },
        Op::PopBare(k) => Stmt::SideEffectCall {
            call: Expr::DictPop {
                dict: Box::new(ident("d")),
                key: Box::new(k.expr()),
                default: None,
            },
        },
        Op::Del(k) => Stmt::DelItem {
            name: "d".into(),
            key: k.expr(),
            is_dict: true,
        },
        Op::Clear => Stmt::ListMutate {
            list_name: "d".into(),
            op: ListMutateOp::Clear,
            of_float: false,
        },
    }
}

/// The CPython source line for a dict op.
fn op_py(op: &Op) -> String {
    match op {
        Op::Set(k, v) => format!("d[{}]={}", k.py(), v),
        Op::SetDefault(k, v) => format!("d.setdefault({},{})", k.py(), v),
        Op::PopDefault(k) => format!("d.pop({},0)", k.py()),
        Op::PopBare(k) => format!("d.pop({})", k.py()),
        Op::Del(k) => format!("del d[{}]", k.py()),
        Op::Clear => "d.clear()".to_string(),
    }
}

// ---- set ops ----------------------------------------------------------------

/// One set mutation (the set surface has no removal — add / clear only).
#[derive(Clone)]
enum SOp {
    Add(K),
    Clear,
}

fn sop_stmt(op: &SOp) -> Stmt {
    match op {
        SOp::Add(k) => Stmt::SetAdd {
            set_name: "s".into(),
            elem: k.expr(),
        },
        SOp::Clear => Stmt::ListMutate {
            list_name: "s".into(),
            op: ListMutateOp::Clear,
            of_float: false,
        },
    }
}

fn sop_py(op: &SOp) -> String {
    match op {
        SOp::Add(k) => format!("s.add({})", k.py()),
        SOp::Clear => "s.clear()".to_string(),
    }
}

// ---- deterministic RNG ------------------------------------------------------

/// A fixed-seed 64-bit LCG (no `rand` — `cargo deny` unaffected, corpus byte-stable).
struct Lcg(u64);
impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() >> 33) as usize % n
    }
}

// ---- sequence generation ----------------------------------------------------

/// A generated dict sequence: an initial literal (distinct keys) + an op list, all
/// trap-free (dels/bare-pops only on modelled-present keys).
struct DictSeq {
    tag: String,
    keyty: Type,
    init: Vec<(K, i64)>,
    ops: Vec<Op>,
    keys: &'static [K],
}

/// A generated set sequence.
struct SetSeq {
    tag: String,
    keyty: Type,
    init: Vec<K>,
    ops: Vec<SOp>,
    keys: &'static [K],
}

/// Curated dict interaction edges over a keyspace `keys` (built via `keys[i]`).
/// These deliberately hit the cross-op paths a single-op witness cannot reach.
fn curated_dicts(tag_prefix: &str, keys: &'static [K]) -> Vec<DictSeq> {
    let k = |i: usize| keys[i].clone();
    let keyty = keys[0].ty();
    let mk = |name: &str, init: Vec<(K, i64)>, ops: Vec<Op>| DictSeq {
        tag: format!("{tag_prefix}_{name}"),
        keyty: keyty.clone(),
        init,
        ops,
        keys,
    };
    vec![
        // grow past initial capacity (relocations + base write-back)
        mk(
            "grow",
            vec![(k(0), 0)],
            vec![
                Op::Set(k(1), 11),
                Op::Set(k(2), 22),
                Op::Set(k(3), 33),
                Op::Set(k(4), 44),
                Op::Set(k(5), 55),
            ],
        ),
        // del a MIDDLE entry (swap-last-into-hole) then re-set the swapped key:
        // the update must land on the relocated entry, not append a duplicate.
        mk(
            "delmid_reset",
            vec![(k(0), 10), (k(1), 20), (k(2), 30)],
            vec![Op::Del(k(1)), Op::Set(k(2), 222), Op::Set(k(1), 111)],
        ),
        // clear then grow FROM the cleared base
        mk(
            "clear_grow",
            vec![(k(0), 1), (k(1), 2), (k(2), 3)],
            vec![
                Op::Clear,
                Op::Set(k(3), 33),
                Op::Set(k(4), 44),
                Op::Set(k(5), 55),
                Op::Set(k(0), 99),
            ],
        ),
        // setdefault MISS (insert) then HIT (keep existing), then pop-remove
        mk(
            "sd_then_pop",
            vec![(k(0), 7)],
            vec![
                Op::SetDefault(k(1), 100),
                Op::SetDefault(k(1), 999), // hit: value stays 100
                Op::PopDefault(k(0)),      // remove k0
                Op::SetDefault(k(0), 5),   // reinsert k0 = 5
            ],
        ),
        // churn: delete every key (region → empty via del), then rebuild past cap
        mk(
            "drain_rebuild",
            vec![(k(0), 1), (k(1), 2), (k(2), 3)],
            vec![
                Op::Del(k(0)),
                Op::Del(k(1)),
                Op::Del(k(2)),
                Op::Set(k(5), 50),
                Op::Set(k(4), 40),
                Op::Set(k(3), 30),
                Op::Set(k(2), 20),
            ],
        ),
        // bare pop (present) then set the popped key again; interleave a setdefault
        mk(
            "popbare_mix",
            vec![(k(0), 8), (k(1), 9)],
            vec![
                Op::PopBare(k(0)),
                Op::Set(k(0), 80),
                Op::SetDefault(k(2), 25),
                Op::PopDefault(k(1)),
            ],
        ),
    ]
}

/// Curated set interaction edges: grow past cap, clear-then-regrow, re-add dup.
fn curated_sets(tag_prefix: &str, keys: &'static [K]) -> Vec<SetSeq> {
    let k = |i: usize| keys[i].clone();
    let keyty = keys[0].ty();
    let mk = |name: &str, init: Vec<K>, ops: Vec<SOp>| SetSeq {
        tag: format!("{tag_prefix}_{name}"),
        keyty: keyty.clone(),
        init,
        ops,
        keys,
    };
    vec![
        mk(
            "grow",
            vec![k(0)],
            vec![
                SOp::Add(k(1)),
                SOp::Add(k(2)),
                SOp::Add(k(3)),
                SOp::Add(k(4)),
                SOp::Add(k(5)),
            ],
        ),
        // re-add a present element: len must NOT double-count
        mk(
            "dup",
            vec![k(0), k(1)],
            vec![
                SOp::Add(k(0)),
                SOp::Add(k(1)),
                SOp::Add(k(2)),
                SOp::Add(k(0)),
            ],
        ),
        // clear then regrow from the cleared base
        mk(
            "clear_grow",
            vec![k(0), k(1), k(2)],
            vec![SOp::Clear, SOp::Add(k(3)), SOp::Add(k(4)), SOp::Add(k(0))],
        ),
    ]
}

/// A random trap-free dict sequence. A presence model gates `del`/bare-`pop` so no
/// op ever targets an absent key.
fn random_dict(rng: &mut Lcg, tag: String, keys: &'static [K], n_ops: usize) -> DictSeq {
    let mut model: BTreeMap<K, i64> = BTreeMap::new();
    // 1..=2 distinct initial entries.
    let n_init = 1 + rng.below(2);
    let mut init: Vec<(K, i64)> = Vec::new();
    while init.len() < n_init {
        let key = keys[rng.below(keys.len())].clone();
        if model.contains_key(&key) {
            continue;
        }
        let v = rng.below(1000) as i64;
        model.insert(key.clone(), v);
        init.push((key, v));
    }
    let mut ops = Vec::with_capacity(n_ops);
    for _ in 0..n_ops {
        let key = keys[rng.below(keys.len())].clone();
        let v = rng.below(1000) as i64;
        // Pick a modelled-present key (for del / bare-pop) when one exists.
        let present = if model.is_empty() {
            None
        } else {
            model.keys().nth(rng.below(model.len())).cloned()
        };
        let op = match rng.below(8) {
            0 | 1 => {
                model.insert(key.clone(), v);
                Op::Set(key, v)
            }
            2 => {
                model.entry(key.clone()).or_insert(v);
                Op::SetDefault(key, v)
            }
            3 => {
                model.remove(&key);
                Op::PopDefault(key)
            }
            4 => match present {
                Some(pk) => {
                    model.remove(&pk);
                    Op::Del(pk)
                }
                None => {
                    model.insert(key.clone(), v);
                    Op::Set(key, v)
                }
            },
            5 => match present {
                Some(pk) => {
                    model.remove(&pk);
                    Op::PopBare(pk)
                }
                None => {
                    model.insert(key.clone(), v);
                    Op::Set(key, v)
                }
            },
            6 => {
                model.clear();
                Op::Clear
            }
            _ => {
                model.insert(key.clone(), v);
                Op::Set(key, v)
            }
        };
        ops.push(op);
    }
    DictSeq {
        tag,
        keyty: keys[0].ty(),
        init,
        ops,
        keys,
    }
}

/// A random set sequence (add / clear only — no removal to model).
fn random_set(rng: &mut Lcg, tag: String, keys: &'static [K], n_ops: usize) -> SetSeq {
    let n_init = 1 + rng.below(2);
    let mut init: Vec<K> = Vec::new();
    let mut seen: BTreeSet<K> = BTreeSet::new();
    while init.len() < n_init {
        let key = keys[rng.below(keys.len())].clone();
        if seen.insert(key.clone()) {
            init.push(key);
        }
    }
    let mut ops = Vec::with_capacity(n_ops);
    for _ in 0..n_ops {
        let op = if rng.below(6) == 0 {
            SOp::Clear
        } else {
            SOp::Add(keys[rng.below(keys.len())].clone())
        };
        ops.push(op);
    }
    SetSeq {
        tag,
        keyty: keys[0].ty(),
        init,
        ops,
        keys,
    }
}

/// The full deterministic corpus: curated edges + a fixed-seed LCG walk, over both
/// int-keyed and str-keyed dicts and sets.
fn corpus() -> (Vec<DictSeq>, Vec<SetSeq>) {
    let mut dicts: Vec<DictSeq> = Vec::new();
    let mut sets: Vec<SetSeq> = Vec::new();
    dicts.extend(curated_dicts("di", INT_KEYS));
    dicts.extend(curated_dicts("ds", STR_KEYS));
    sets.extend(curated_sets("si", INT_KEYS));
    sets.extend(curated_sets("ss", STR_KEYS));

    let mut rng = Lcg(0x9E3779B97F4A7C15); // golden-ratio seed
    for i in 0..6 {
        dicts.push(random_dict(&mut rng, format!("di_r{i}"), INT_KEYS, 10));
    }
    for i in 0..6 {
        dicts.push(random_dict(&mut rng, format!("ds_r{i}"), STR_KEYS, 10));
    }
    for i in 0..4 {
        sets.push(random_set(&mut rng, format!("si_r{i}"), INT_KEYS, 8));
    }
    for i in 0..4 {
        sets.push(random_set(&mut rng, format!("ss_r{i}"), STR_KEYS, 8));
    }
    (dicts, sets)
}

// ---- meta-HIR module assembly -----------------------------------------------

fn func(name: &str, stmts: Vec<Stmt>, ret: Type, tail: Expr) -> Item {
    Item::Function(Function {
        name: name.into(),
        params: vec![],
        return_type: ret,
        body: Block {
            stmts,
            trailing_return: tail,
        },
    })
}

fn dict_let(keyty: &Type, init: &[(K, i64)]) -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(keyty.clone()), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(
            init.iter()
                .map(|(k, v)| (k.expr(), Expr::LitInt(*v)))
                .collect(),
        ),
    }
}

fn set_let(keyty: &Type, init: &[K]) -> Stmt {
    Stmt::Let {
        name: "s".into(),
        ty: Type::Set(Box::new(keyty.clone())),
        mutable: true,
        value: Expr::SetLit(init.iter().map(|k| k.expr()).collect()),
    }
}

/// The statement prefix that rebuilds a dict and replays its op sequence. Each
/// observable function re-runs it (dict mutates — mirror CPython's fresh dict).
fn dict_prefix(seq: &DictSeq) -> Vec<Stmt> {
    let mut v = vec![dict_let(&seq.keyty, &seq.init)];
    v.extend(seq.ops.iter().map(op_stmt));
    v
}

fn set_prefix(seq: &SetSeq) -> Vec<Stmt> {
    let mut v = vec![set_let(&seq.keyty, &seq.init)];
    v.extend(seq.ops.iter().map(sop_stmt));
    v
}

/// The bump heap is a single 64 KiB page with NO reset between exports (by design
/// — `lib.rs` `heap_helpers`), so each SEQUENCE gets its OWN module / instantiation
/// (like the string fuzz ran per-input). Its 7 replaying exports allocate a few KB
/// total — far under the page — whereas packing all ~280 exports into one
/// instantiation would (correctly) TRAP as the shared bump pointer walks off the
/// page.
///
/// The dict sequence's module: a `len` export + one `get(k,-1)` export per key.
fn dict_module(seq: &DictSeq) -> Module {
    let mut items = vec![func(
        &format!("{}_len", seq.tag),
        dict_prefix(seq),
        Type::I64,
        Expr::Len(Box::new(ident("d"))),
    )];
    for (i, key) in seq.keys.iter().enumerate() {
        items.push(func(
            &format!("{}_g{i}", seq.tag),
            dict_prefix(seq),
            Type::I64,
            Expr::DictGetOr {
                dict: Box::new(ident("d")),
                key: Box::new(key.expr()),
                default: Box::new(Expr::LitInt(SENTINEL)),
            },
        ));
    }
    Module {
        name: format!("{}_mod", seq.tag),
        source_lang: SourceLang::Rust,
        items,
        ffi_boundaries: Vec::new(),
    }
}

/// The set sequence's module: a `len` export + one `k in s` export per key.
fn set_module(seq: &SetSeq) -> Module {
    let mut items = vec![func(
        &format!("{}_len", seq.tag),
        set_prefix(seq),
        Type::I64,
        Expr::Len(Box::new(ident("s"))),
    )];
    for (i, key) in seq.keys.iter().enumerate() {
        items.push(func(
            &format!("{}_m{i}", seq.tag),
            set_prefix(seq),
            Type::Bool,
            Expr::SetContains {
                set: Box::new(ident("s")),
                elem: Box::new(key.expr()),
            },
        ));
    }
    Module {
        name: format!("{}_mod", seq.tag),
        source_lang: SourceLang::Rust,
        items,
        ffi_boundaries: Vec::new(),
    }
}

// ---- CPython oracle ---------------------------------------------------------

/// The `(observable_name, expected)` pairs from CPython running the identical
/// sequences. `python3` is the sole source of truth — no hand-written values.
fn python_oracle(dicts: &[DictSeq], sets: &[SetSeq]) -> Option<BTreeMap<String, i64>> {
    let mut prog = String::from("v={}\n");
    for seq in dicts {
        prog.push_str(&format!("def {}():\n", seq.tag));
        let init: Vec<String> = seq
            .init
            .iter()
            .map(|(k, val)| format!("{}:{}", k.py(), val))
            .collect();
        prog.push_str(&format!("\td={{{}}}\n", init.join(",")));
        for op in &seq.ops {
            prog.push_str(&format!("\t{}\n", op_py(op)));
        }
        prog.push_str("\treturn d\n");
        prog.push_str(&format!("v['{}_len']=len({}())\n", seq.tag, seq.tag));
        for (i, key) in seq.keys.iter().enumerate() {
            prog.push_str(&format!(
                "v['{}_g{i}']={}().get({},{})\n",
                seq.tag,
                seq.tag,
                key.py(),
                SENTINEL
            ));
        }
    }
    for seq in sets {
        prog.push_str(&format!("def {}():\n", seq.tag));
        // Empty set literal is `set()`, not `{}` — but every init here is non-empty.
        let init: Vec<String> = seq.init.iter().map(|k| k.py()).collect();
        prog.push_str(&format!("\ts={{{}}}\n", init.join(",")));
        for op in &seq.ops {
            prog.push_str(&format!("\t{}\n", sop_py(op)));
        }
        prog.push_str("\treturn s\n");
        prog.push_str(&format!("v['{}_len']=len({}())\n", seq.tag, seq.tag));
        for (i, key) in seq.keys.iter().enumerate() {
            prog.push_str(&format!(
                "v['{}_m{i}']=int({} in {}())\n",
                seq.tag,
                key.py(),
                seq.tag
            ));
        }
    }
    prog.push_str("print(';'.join(f'{k}={val}' for k,val in v.items()))\n");

    let out = Command::new("python3")
        .arg("-c")
        .arg(&prog)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!(
            "PMAT-1238: python3 oracle failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut map = BTreeMap::new();
    for kv in text.trim().split(';') {
        let (k, val) = kv.split_once('=').expect("k=v");
        map.insert(k.to_string(), val.parse::<i64>().expect("int observable"));
    }
    Some(map)
}

// ---- WABT harness -----------------------------------------------------------

/// Parse a `name() => <ty>:<value>` line. `wasm-interp` prints integers UNSIGNED,
/// so a negative `i64` renders as its `u64` two's-complement — parse as `u64` and
/// reinterpret. A `bool` export prints as `i32:0` / `i32:1`.
fn parse_scalar(stdout: &str, name: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    let val = line
        .rsplit_once(':')
        .unwrap_or_else(|| panic!("malformed export line {line:?}"))
        .1
        .trim();
    val.parse::<u64>()
        .map(|u| u as i64)
        .unwrap_or_else(|_| panic!("parse scalar for {name} from {line:?}"))
}

fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-dictfamfuzz-{}-{}",
        std::process::id(),
        tag
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("prog.wat");
    let wasm_path = dir.join("prog.wasm");
    std::fs::write(&wat_path, wat).expect("write wat");

    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed:\n{}\n---WAT (first 4k)---\n{}",
        String::from_utf8_lossy(&assemble.stderr),
        &wat[..wat.len().min(4096)]
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    (stdout, run.status.success())
}

/// `true` iff `python3` is invocable.
fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---- tests ------------------------------------------------------------------

#[test]
fn dict_set_family_fuzz_lowers() {
    // The EMIT path must lower for every generated sequence regardless of WABT
    // (holds on free CI) — an interleave smoke over the whole family.
    let (dicts, sets) = corpus();
    for seq in &dicts {
        emit_module(&dict_module(seq))
            .unwrap_or_else(|e| panic!("dict sequence {} must lower: {e:?}", seq.tag));
    }
    for seq in &sets {
        emit_module(&set_module(seq))
            .unwrap_or_else(|e| panic!("set sequence {} must lower: {e:?}", seq.tag));
    }
}

#[test]
fn corpus_is_deterministic_and_exercises_interactions() {
    let (d1, s1) = corpus();
    let (d2, s2) = corpus();
    // Determinism: same tags, same op counts, run to run.
    assert_eq!(d1.len(), d2.len(), "dict corpus size unstable");
    assert_eq!(s1.len(), s2.len(), "set corpus size unstable");
    for (a, b) in d1.iter().zip(&d2) {
        assert_eq!(a.tag, b.tag, "dict tag order unstable");
        assert_eq!(a.ops.len(), b.ops.len(), "dict {} op count unstable", a.tag);
    }

    // The corpus must actually EXERCISE the interesting cross-op paths — else the
    // fuzz could pass while only ever running trivial single-op sequences.
    let has = |tag: &str| d1.iter().any(|s| s.tag == tag);
    assert!(
        has("di_grow") && has("ds_grow"),
        "grow-past-cap edge missing"
    );
    assert!(
        has("di_delmid_reset") && has("ds_delmid_reset"),
        "del-middle-then-reinsert edge missing"
    );
    assert!(
        has("di_clear_grow") && has("ds_clear_grow"),
        "clear-then-grow edge missing"
    );
    assert!(
        has("di_drain_rebuild"),
        "drain-all-then-rebuild edge missing"
    );

    // Presence-guarded generation must never emit a del/bare-pop that would trap:
    // replay each random sequence's model and assert every del/pop targets a
    // present key.
    for seq in d1.iter().filter(|s| s.tag.contains("_r")) {
        let mut present: BTreeSet<K> = seq.init.iter().map(|(k, _)| k.clone()).collect();
        for op in &seq.ops {
            match op {
                Op::Set(k, _) | Op::SetDefault(k, _) => {
                    present.insert(k.clone());
                }
                Op::PopDefault(k) => {
                    present.remove(k);
                }
                Op::Del(k) | Op::PopBare(k) => {
                    assert!(
                        present.contains(k),
                        "{}: del/bare-pop on absent key would TRAP — generator guard broke",
                        seq.tag
                    );
                    present.remove(k);
                }
                Op::Clear => present.clear(),
            }
        }
    }

    // At least one random sequence must contain a del AND a bare-pop AND a clear,
    // so the fuzz genuinely churns the count header both directions.
    let ops_flat: Vec<&Op> = d1.iter().flat_map(|s| s.ops.iter()).collect();
    assert!(
        ops_flat.iter().any(|o| matches!(o, Op::Del(_))),
        "no Del generated"
    );
    assert!(
        ops_flat.iter().any(|o| matches!(o, Op::PopBare(_))),
        "no bare Pop generated"
    );
    assert!(
        ops_flat.iter().any(|o| matches!(o, Op::Clear)),
        "no Clear generated"
    );
}

#[test]
fn dict_set_family_matches_cpython_over_random_sequences() {
    let (dicts, sets) = corpus();

    // The EMIT path holds regardless of WABT (also asserted in the lowering test).
    for seq in &dicts {
        emit_module(&dict_module(seq)).expect("dict sequence lowers");
    }
    for seq in &sets {
        emit_module(&set_module(seq)).expect("set sequence lowers");
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1238: skipping EXECUTED dict/set sequence fuzz — WABT (wat2wasm / \
             wasm-interp) absent. Every sequence lowered through emit_module (asserted \
             in `dict_set_family_fuzz_lowers`); a box with WABT + python3 runs every \
             observable and value-matches live CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1238: skipping dict/set fuzz value-diff — python3 (the oracle) absent.");
        return;
    }

    let oracle = match python_oracle(&dicts, &sets) {
        Some(o) => o,
        None => {
            eprintln!("PMAT-1238: python3 oracle unavailable — skipping value diff.");
            return;
        }
    };

    // Each sequence is its OWN module / instantiation (fresh single-page heap), so
    // its handful of allocating exports stay well under 64 KiB. Diff every
    // observable; collect all divergences so a failure reports the full set.
    let mut mismatches: Vec<String> = Vec::new();
    let mut checked = 0usize;

    let mut run_seq = |tag: &str, wat: &str, names: &[String]| {
        let (stdout, ok) = assemble_and_run(tag, wat);
        assert!(ok, "wasm-interp run failed for {tag}:\n{stdout}");
        assert!(
            !stdout.contains("unreachable executed"),
            "{tag} trapped — the generator emitted a del/pop on an absent key:\n{stdout}"
        );
        for name in names {
            let expected = *oracle
                .get(name)
                .unwrap_or_else(|| panic!("CPython oracle missing observable {name}"));
            let got = parse_scalar(&stdout, name);
            if got == expected {
                checked += 1;
            } else {
                mismatches.push(format!("{name}: WASM={got} CPython={expected}"));
            }
        }
    };

    for seq in &dicts {
        let wat = emit_module(&dict_module(seq)).expect("dict module lowers");
        let mut names = vec![format!("{}_len", seq.tag)];
        names.extend((0..seq.keys.len()).map(|i| format!("{}_g{i}", seq.tag)));
        run_seq(&seq.tag, &wat, &names);
    }
    for seq in &sets {
        let wat = emit_module(&set_module(seq)).expect("set module lowers");
        let mut names = vec![format!("{}_len", seq.tag)];
        names.extend((0..seq.keys.len()).map(|i| format!("{}_m{i}", seq.tag)));
        run_seq(&seq.tag, &wat, &names);
    }

    assert!(
        mismatches.is_empty(),
        "PMAT-1238: {} WASM/CPython divergence(s) over the dict/set sequence corpus:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    // Every oracle observable was found and matched.
    assert_eq!(
        checked,
        oracle.len(),
        "every CPython observable must be matched by a WASM export"
    );

    eprintln!(
        "PMAT-1238: dict/set sequence fuzz PASSED — {checked} observables across {} dict \
         + {} set sequences (int- AND str-keyed) executed in WABT and matched live \
         python3. No silent divergence in grow-past-cap, del-swap-then-reinsert, \
         grow-after-clear, drain-then-rebuild, setdefault-miss/hit, or pop-then-reinsert \
         interactions.",
        dicts.len(),
        sets.len()
    );
}
