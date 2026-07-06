//! PMAT-1313 — the ADVERSARIAL-VERIFY differential fuzz for the dict/set
//! FUNCTION/METHOD BOUNDARY surface shipped by PMAT-1309 (free-fn params),
//! PMAT-1310 (free-fn returns), PMAT-1311 (method params) and PMAT-1312
//! (method returns) — the scheduled ~4-slices-since-PMAT-1308 skeptic pass
//! over the newest lane. A fixed-seed LCG drives whole MODULES where records
//! flow across boundaries in BOTH directions and BOTH callable kinds at once:
//! a factory (`mk_<t>`, optionally relocating INSIDE the callee before
//! return), 1-2 mutating free-fn helpers (pop-with-default / guarded-del /
//! get / len folds), a two-param helper called ALIASED (`hb(d, d)`) and
//! unaliased (`hb(d, e)`), and a class whose method factory reads `self`
//! state and whose method mutator pops with a `self.base` default that the
//! driver RE-POINTS mid-sequence (`c.base = n`). Drivers interleave calls
//! with caller-side growth (stores + store LOOPS past the slack), caller
//! pops folded positionally into `acc`, re-binding the SAME name to a second
//! factory call, and `d.update(e)` merges of two RETURNED records — every
//! observable value-matched against LIVE CPython on the IDENTICAL source.
//!
//! ## What the skeptic pass targeted (and what it did NOT refute)
//!
//! 1. **Growth-through-param is the lane's load-bearing refusal** — if any
//!    growth-capable op (`d[k] = v` fresh-key store, `setdefault` miss,
//!    `update`/`|=`, `s.add`) slipped through a param, a relocation would
//!    leave the CALLER's base-pointer stale: the classic silent miscompile.
//!    Probed through the FULL pipeline over free-fn AND method params, all
//!    five spellings: every one refuses with the relocation argument named
//!    (`boundary_growth_and_bind_belts_hold`). NOT refuted.
//! 2. **Callee-relocation-before-return composed with content eq** — a
//!    factory that outruns the 16-slot literal slack RELOCATES before
//!    returning; the caller then walks, pops, and `==`-compares the record.
//!    Curated `cg` + the LCG walks with growth loops pin it. NOT refuted.
//! 3. **Aliasing through the boundary** — the same record passed TWICE
//!    (`hb(d, d)`: pop through `a`, read through `b`), del-via-other-alias,
//!    and a returned record handed BACK IN as a method param. NOT refuted.
//! 4. **Eval-order at call sites** — a MUTATING call in an arg list beside a
//!    read of the same record (`two(eat(d), d.get(1, -9))`), caller pops
//!    inline in arithmetic (`acc * 3 + d.pop(k, df)`), recursion draining a
//!    dict param. NOT refuted (`x_argmut` / `x_rec` / the `PopAcc` op).
//! 5. **Re-binding** — `d = mk()` twice (the second bind must re-point, not
//!    merge), loop re-binds, and the KIND belts: re-binding an i-keyed name
//!    from an s-keyed factory refuses, a branch-SELECTED bind refuses (the
//!    Let/no-Let registration is straight-line). NOT refuted.
//!
//! 24 hand-probed shapes + this corpus (12 sequences x 7-8 observables + 19
//! curated extra modules) all match CPython exactly: **the PMAT-1309..1312
//! boundary surface survived the skeptic pass unrefuted.** The verify
//! discipline resets here.
//!
//! ## Mutation-verified teeth
//!
//! Both seeded boundary miscompiles are KILLED by the executed differential
//! (seeded by hand during PMAT-1313, then reverted):
//! * `emit_heap_ret` returning `i32.const 0` instead of the record's local
//!   (a null-pointer return — callers see an EMPTY dict, reads fall back to
//!   defaults, no trap) → dozens of `_acc`/`_lend`/`_eqm` observables
//!   diverge, first kill in seconds;
//! * the value-position call-arg loop passing `i32.const 0` for a dict/set
//!   argument (callee mutations vanish, its reads see count 0, the CALLER's
//!   record silently stops observing pops) → every `CallH`-carrying
//!   sequence's `_acc` + the eq observables diverge.
//!
//! ## Model honesty
//!
//! The Rust-side replay model (needed to spell the `eqm`/`eqn` literal and
//! pick live probe keys) is itself PINNED against CPython inside the
//! executed diff: every `_eqm` observable's ORACLE value must be 1 and every
//! `_eqn` value 0 — if the replay ever drifts from CPython semantics, the
//! test fails on the oracle side before a wasm bug could hide behind a
//! model bug.
//!
//! Every observable is a standalone zero-arg `def NAME() -> int` (valid
//! plain `python3` AND wasm-frontend-lowerable); one sequence = ONE module
//! (its own fresh bump heap); the IDENTICAL text feeds both lanes, so the
//! oracle has ZERO reimplementation risk. All exported helpers are TOTAL
//! (`--run-all-exports` zero-arg-invokes them with addr-0 records: count 0 —
//! only pop-with-default / guarded-del / guarded-clear / get / len ops, no
//! unguarded subscripts, no 1-arg pops). Deterministic fixed-seed LCG — no
//! `rand`, no time, byte-stable corpus.
//!
//! ## Gating
//!
//! The executed diff needs WABT (`wat2wasm` / `wasm-interp`) AND `python3`;
//! without either it skips cleanly after asserting the EMIT path + the i32
//! base-pointer ABI for every sequence. Refusal pins run on the emit path
//! alone. CITES `C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP` (test-only; no new
//! contract).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- frontend lowering (the CLI's `--target wasm` path) ---------------------

fn wasm_profile() -> LoweringProfile {
    LoweringProfile {
        alias_semantics: AliasSemantics::Reference,
        runtime_abort: true,
    }
}

fn lower(src: &str) -> Result<Module, String> {
    PythonFrontend
        .parse_and_lower_profiled(Path::new("witness.py"), src, wasm_profile())
        .map_err(|e| format!("frontend: {e}"))
}

/// FULL pipeline: Python source (one or more `def`s) → meta-HIR → WAT.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- deterministic RNG (byte-stable corpus, no `rand`) ----------------------

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
    fn key(&mut self) -> i64 {
        1 + self.below(8) as i64
    }
    fn val(&mut self) -> i64 {
        self.below(50) as i64 - 10
    }
}

// ---- the helper-op alphabet (all TOTAL: safe on an addr-0 / empty record) ----

/// One op inside a mutating free-fn helper over its dict PARAM. Every op is
/// total — `--run-all-exports` invokes each helper with `d = 0` (count 0).
#[derive(Clone)]
enum HOp {
    /// `p<i>: int = d.pop(k, df)` — in-place removal, CALLER-VISIBLE; folded.
    PopD(i64, i64),
    /// folds `d.get(k, df)`.
    Get(i64, i64),
    /// `if k in d: del d[k]` — guarded (total) del; folds 1/0.
    HasDel(i64),
    /// folds `len(d)`.
    Len,
}

#[derive(Clone)]
struct Helper {
    ops: Vec<HOp>,
}

impl Helper {
    /// Emit `def h<j>_<tag>(d: dict[int, int]) -> int:` with the fold body.
    fn source(&self, name: &str) -> String {
        let mut s = format!("def {name}(d: dict[int, int]) -> int:\n    r: int = 0\n");
        for (i, op) in self.ops.iter().enumerate() {
            match op {
                HOp::PopD(k, df) => {
                    s.push_str(&format!("    p{i}: int = d.pop({k}, {df})\n"));
                    s.push_str(&format!("    r = r * 7 + p{i}\n"));
                }
                HOp::Get(k, df) => {
                    s.push_str(&format!("    r = r * 7 + d.get({k}, {df})\n"));
                }
                HOp::HasDel(k) => {
                    s.push_str(&format!("    t{i}: int = 0\n"));
                    s.push_str(&format!(
                        "    if {k} in d:\n        del d[{k}]\n        t{i} = 1\n"
                    ));
                    s.push_str(&format!("    r = r * 7 + t{i}\n"));
                }
                HOp::Len => s.push_str("    r = r * 7 + len(d)\n"),
            }
        }
        s.push_str("    return r\n");
        s
    }

    /// Replay the helper against a model; returns the fold value.
    fn apply(&self, model: &mut BTreeMap<i64, i64>) -> i64 {
        let mut r = 0i64;
        for op in &self.ops {
            match op {
                HOp::PopD(k, df) => r = r * 7 + model.remove(k).unwrap_or(*df),
                HOp::Get(k, df) => r = r * 7 + model.get(k).copied().unwrap_or(*df),
                HOp::HasDel(k) => r = r * 7 + i64::from(model.remove(k).is_some()),
                HOp::Len => r = r * 7 + model.len() as i64,
            }
        }
        r
    }
}

// ---- the driver-op alphabet ----------------------------------------------------

/// One caller-side action in a sequence's driver. `d` is the free-fn factory
/// binding, `e` (when `use_e`) the METHOD-factory binding, `c` the instance.
#[derive(Clone)]
enum DOp {
    /// `acc = acc * 3 + h<j>_<tag>(d|e)` — a mutating call in VALUE position.
    CallH { h: usize, on_e: bool },
    /// `acc = acc * 3 + hb_<tag>(d, d)` (aliased!) or `hb_<tag>(d, e)`.
    CallBoth { aliased: bool },
    /// `acc = acc * 3 + c.meat(d|e)` — the METHOD boundary, `self.base` default.
    CallMeat { on_e: bool },
    /// `d[k] = v` — caller-side growth (sanctioned on a non-param local).
    Store(i64, i64),
    /// A store LOOP `s..e` (val `i * m`) — outruns the slack when `e - s > 16`.
    StoreLoop { s: i64, e: i64, m: i64 },
    /// `c.base = n` — re-points the method default AND the mfac seed.
    SetBase(i64),
    /// `d = mk_<tag>()` — RE-bind the same name to a fresh record.
    Rebind,
    /// `d.update(e)` — caller-side merge of two RETURNED records.
    UpdateFrom,
    /// `acc = acc * 3 + d.pop(k, df)` — a side-effecting expr in arithmetic.
    PopAcc(i64, i64),
    /// `if k in d: del d[k]` — caller-side guarded del.
    GuardDel(i64),
}

// ---- one fuzz sequence -----------------------------------------------------------

struct Seq {
    tag: String,
    /// Factory literal pairs + optional growth loop `(start, end, mult)`.
    mk_init: Vec<(i64, i64)>,
    mk_loop: Option<(i64, i64, i64)>,
    helpers: Vec<Helper>,
    /// `hb_<tag>` constants: `a.pop(hb_k, -3)` / `b.get(hb_k2, -5)`.
    hb_k: i64,
    hb_k2: i64,
    /// `self.base` initial value; mfac spells `{fk1: self.base, fk2: fv2}`.
    base: i64,
    fk1: i64,
    fk2: i64,
    fv2: i64,
    /// `meat` pops `meat_k` with default `self.base`.
    meat_k: i64,
    use_e: bool,
    ops: Vec<DOp>,
}

/// The replayed driver: final models + the acc fold + relocation counts (for
/// the capacity pins — the corpus must keep outrunning the mirrored slack).
struct Replay {
    d_model: BTreeMap<i64, i64>,
    acc: i64,
    /// Relocations inside the FACTORY body (callee-side, before return).
    mk_relocs: usize,
    /// Relocations on `d` in the DRIVER (caller-side growth on a returned
    /// record — the PMAT-1310 escape hatch under real pressure).
    driver_relocs: usize,
}

/// The dict growth slack (`DICT_GROWTH_SLACK` in the codegen, private) —
/// mirrored so the corpus can PROVE its growth shapes force real relocations.
const MIRRORED_GROWTH_SLACK: usize = 16;

impl Seq {
    fn mk_name(&self) -> String {
        format!("mk_{}", self.tag)
    }
    fn class_name(&self) -> String {
        format!("C_{}", self.tag)
    }
    fn h_name(&self, j: usize) -> String {
        format!("h{j}_{}", self.tag)
    }
    fn hb_name(&self) -> String {
        format!("hb_{}", self.tag)
    }

    /// The factory's returned content + its callee-side relocation count.
    fn mk_model(&self) -> (BTreeMap<i64, i64>, usize) {
        let mut m: BTreeMap<i64, i64> = self.mk_init.iter().copied().collect();
        let mut count = m.len();
        let mut cap = count + MIRRORED_GROWTH_SLACK;
        let mut relocs = 0;
        if let Some((s, e, mult)) = self.mk_loop {
            for i in s..e {
                if m.insert(i, i * mult).is_none() {
                    if count >= cap {
                        cap *= 2;
                        relocs += 1;
                    }
                    count += 1;
                }
            }
        }
        (m, relocs)
    }

    /// Module-level support defs: factory + helpers + hb + the class.
    fn support_source(&self) -> String {
        let mut s = format!("def {}() -> dict[int, int]:\n", self.mk_name());
        let entries: Vec<String> = self
            .mk_init
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        s.push_str(&format!(
            "    d: dict[int, int] = {{{}}}\n",
            entries.join(", ")
        ));
        if let Some((start, end, mult)) = self.mk_loop {
            s.push_str(&format!("    i: int = {start}\n"));
            s.push_str(&format!(
                "    while i < {end}:\n        d[i] = i * {mult}\n        i = i + 1\n"
            ));
        }
        s.push_str("    return d\n\n");
        for (j, h) in self.helpers.iter().enumerate() {
            s.push_str(&h.source(&self.h_name(j)));
            s.push('\n');
        }
        s.push_str(&format!(
            "def {}(a: dict[int, int], b: dict[int, int]) -> int:\n    \
             p: int = a.pop({}, -3)\n    \
             q: int = b.get({}, -5)\n    \
             return p * 100 + q * 10 + len(b)\n\n",
            self.hb_name(),
            self.hb_k,
            self.hb_k2
        ));
        s.push_str(&format!(
            "class {}:\n    \
             def __init__(self) -> None:\n        \
             self.base: int = {}\n\n    \
             def mfac(self) -> dict[int, int]:\n        \
             d: dict[int, int] = {{{}: self.base, {}: {}}}\n        \
             return d\n\n    \
             def meat(self, d: dict[int, int]) -> int:\n        \
             p: int = d.pop({}, self.base)\n        \
             return p * 3 + len(d)\n\n",
            self.class_name(),
            self.base,
            self.fk1,
            self.fk2,
            self.fv2,
            self.meat_k
        ));
        s
    }

    /// The shared driver-prefix lines (each element = one 4-space-indented
    /// logical line; embedded `\n    `/`\n        ` carry block bodies).
    fn driver_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("d = {}()", self.mk_name()),
            format!("c: {} = {}()", self.class_name(), self.class_name()),
        ];
        if self.use_e {
            lines.push("e = c.mfac()".to_string());
        }
        lines.push("acc: int = 0".to_string());
        for (oi, op) in self.ops.iter().enumerate() {
            match op {
                DOp::CallH { h, on_e } => {
                    let t = if *on_e { "e" } else { "d" };
                    lines.push(format!("acc = acc * 3 + {}({t})", self.h_name(*h)));
                }
                DOp::CallBoth { aliased } => {
                    let b = if *aliased { "d" } else { "e" };
                    lines.push(format!("acc = acc * 3 + {}(d, {b})", self.hb_name()));
                }
                DOp::CallMeat { on_e } => {
                    let t = if *on_e { "e" } else { "d" };
                    lines.push(format!("acc = acc * 3 + c.meat({t})"));
                }
                DOp::Store(k, v) => lines.push(format!("d[{k}] = {v}")),
                DOp::StoreLoop { s, e, m } => lines.push(format!(
                    "i{oi}: int = {s}\n    while i{oi} < {e}:\n        \
                     d[i{oi}] = i{oi} * {m}\n        i{oi} = i{oi} + 1"
                )),
                DOp::SetBase(n) => lines.push(format!("c.base = {n}")),
                DOp::Rebind => lines.push(format!("d = {}()", self.mk_name())),
                DOp::UpdateFrom => lines.push("d.update(e)".to_string()),
                DOp::PopAcc(k, df) => lines.push(format!("acc = acc * 3 + d.pop({k}, {df})")),
                DOp::GuardDel(k) => lines.push(format!("if {k} in d:\n        del d[{k}]")),
            }
        }
        lines
    }

    /// Replay the driver against BTreeMap models, mirroring capacity to
    /// count relocations. The RETURN values (acc) are only used for corpus
    /// sanity — CPython is the oracle; the models spell `eqm`/`eqn`.
    fn replay(&self) -> Replay {
        let (mk_model, mk_relocs) = self.mk_model();
        let mut d = mk_model.clone();
        let mut d_count = d.len();
        let mut d_cap = d_count + MIRRORED_GROWTH_SLACK;
        let mut driver_relocs = 0usize;
        let mut base = self.base;
        let mut e: BTreeMap<i64, i64> = if self.use_e {
            [(self.fk1, base), (self.fk2, self.fv2)]
                .into_iter()
                .collect()
        } else {
            BTreeMap::new()
        };
        let mut acc = 0i64;
        let d_insert = |d: &mut BTreeMap<i64, i64>,
                        count: &mut usize,
                        cap: &mut usize,
                        relocs: &mut usize,
                        k: i64,
                        v: i64| {
            if d.insert(k, v).is_none() {
                if *count >= *cap {
                    *cap *= 2;
                    *relocs += 1;
                }
                *count += 1;
            }
        };
        for op in &self.ops {
            match op {
                DOp::CallH { h, on_e } => {
                    let m = if *on_e { &mut e } else { &mut d };
                    acc = acc * 3 + self.helpers[*h].apply(m);
                    if !*on_e {
                        d_count = d.len();
                    }
                }
                DOp::CallBoth { aliased } => {
                    let p = d.remove(&self.hb_k).unwrap_or(-3);
                    let b = if *aliased { &d } else { &e };
                    let q = b.get(&self.hb_k2).copied().unwrap_or(-5);
                    acc = acc * 3 + (p * 100 + q * 10 + b.len() as i64);
                    d_count = d.len();
                }
                DOp::CallMeat { on_e } => {
                    let m = if *on_e { &mut e } else { &mut d };
                    let p = m.remove(&self.meat_k).unwrap_or(base);
                    acc = acc * 3 + (p * 3 + m.len() as i64);
                    if !*on_e {
                        d_count = d.len();
                    }
                }
                DOp::Store(k, v) => {
                    d_insert(&mut d, &mut d_count, &mut d_cap, &mut driver_relocs, *k, *v)
                }
                DOp::StoreLoop { s, e: end, m } => {
                    for i in *s..*end {
                        d_insert(
                            &mut d,
                            &mut d_count,
                            &mut d_cap,
                            &mut driver_relocs,
                            i,
                            i * m,
                        );
                    }
                }
                DOp::SetBase(n) => base = *n,
                DOp::Rebind => {
                    d = mk_model.clone();
                    d_count = d.len();
                    d_cap = d_count + MIRRORED_GROWTH_SLACK;
                }
                DOp::UpdateFrom => {
                    let pairs: Vec<(i64, i64)> = e.iter().map(|(k, v)| (*k, *v)).collect();
                    for (k, v) in pairs {
                        d_insert(&mut d, &mut d_count, &mut d_cap, &mut driver_relocs, k, v);
                    }
                }
                DOp::PopAcc(k, df) => acc = acc * 3 + d.remove(k).unwrap_or(*df),
                DOp::GuardDel(k) => {
                    d.remove(k);
                    d_count = d.len();
                }
            }
        }
        Replay {
            d_model: d,
            acc,
            mk_relocs,
            driver_relocs,
        }
    }

    /// Every observation over the driven state, each a standalone zero-arg
    /// def: the positional call fold, len/sum/get probes, the model eq pair
    /// (`eqm` expect 1, `eqn` expect 0 — BOTH pinned against the oracle),
    /// and the method-factory record when bound.
    fn observables(&self) -> Vec<(String, String)> {
        let replay = self.replay();
        let common = self.driver_lines();
        let mut tails: Vec<(String, String)> = vec![
            ("acc".into(), "return acc".into()),
            ("lend".into(), "return acc * 0 + len(d)".into()),
            ("sumk".into(), "return sum(d)".into()),
        ];
        let probe_key = replay.d_model.keys().next().copied().unwrap_or(42);
        tails.push(("getp".into(), format!("return d.get({probe_key}, -7)")));
        tails.push(("getm".into(), "return d.get(7777, -7)".into()));
        let model_pairs: Vec<(i64, i64)> = replay.d_model.iter().map(|(k, v)| (*k, *v)).collect();
        let spell = |pairs: &[(i64, i64)]| -> String {
            let entries: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}: {v}")).collect();
            format!("{{{}}}", entries.join(", "))
        };
        tails.push((
            "eqm".into(),
            format!(
                "m: dict[int, int] = {}\n    if d == m:\n        return 1\n    return 0",
                spell(&model_pairs)
            ),
        ));
        let flipped: Vec<(i64, i64)> = if model_pairs.is_empty() {
            vec![(9999, 1)]
        } else {
            let mut f = model_pairs.clone();
            f[0].1 += 1;
            f
        };
        tails.push((
            "eqn".into(),
            format!(
                "m: dict[int, int] = {}\n    if d == m:\n        return 1\n    return 0",
                spell(&flipped)
            ),
        ));
        if self.use_e {
            tails.push((
                "lene".into(),
                format!("return len(e) * 100 + e.get({}, -9)", self.fk1),
            ));
        }
        let mut out = Vec::new();
        for (suffix, tail) in tails {
            let name = format!("{}_{suffix}", self.tag);
            let mut src = format!("def {name}() -> int:\n");
            for line in &common {
                src.push_str(&format!("    {line}\n"));
            }
            src.push_str(&format!("    {tail}\n"));
            out.push((name, src));
        }
        out
    }

    /// One wasm module: support defs + every observable def.
    fn module_source(&self) -> String {
        let mut src = self.support_source();
        for (_, def) in self.observables() {
            src.push_str(&def);
            src.push('\n');
        }
        src
    }
}

// ---- corpus -------------------------------------------------------------------

fn corpus() -> Vec<Seq> {
    let mut seqs: Vec<Seq> = vec![
        // cg: CALLEE growth — the factory outruns the slack (1 + 24 inserts
        // past cap 17 → relocation-before-return), then the caller pops,
        // meats, and eq-compares the relocated record.
        Seq {
            tag: "cg".into(),
            mk_init: vec![(0, 9)],
            mk_loop: Some((1, 25, 3)),
            helpers: vec![Helper {
                ops: vec![HOp::PopD(3, -2), HOp::Get(5, -4), HOp::Len],
            }],
            hb_k: 7,
            hb_k2: 9,
            base: 5,
            fk1: 1,
            fk2: 2,
            fv2: 11,
            meat_k: 10,
            use_e: true,
            ops: vec![
                DOp::CallH { h: 0, on_e: false },
                DOp::CallMeat { on_e: false },
                DOp::CallBoth { aliased: true },
                DOp::PopAcc(20, -6),
                DOp::GuardDel(24),
            ],
        },
        // kg: CALLER growth — a small returned record grown past the slack
        // by a driver store loop (25 fresh keys past cap 18 → relocation on
        // the RETURNED record), interleaved with mutating calls.
        Seq {
            tag: "kg".into(),
            mk_init: vec![(1, 4), (2, 6)],
            mk_loop: None,
            helpers: vec![Helper {
                ops: vec![HOp::HasDel(101), HOp::Get(110, -8), HOp::Len],
            }],
            hb_k: 2,
            hb_k2: 120,
            base: 7,
            fk1: 3,
            fk2: 4,
            fv2: 13,
            meat_k: 115,
            use_e: true,
            ops: vec![
                DOp::StoreLoop {
                    s: 100,
                    e: 125,
                    m: 2,
                },
                DOp::CallH { h: 0, on_e: false },
                DOp::CallMeat { on_e: false },
                DOp::CallBoth { aliased: false },
                DOp::UpdateFrom,
            ],
        },
        // al: aliasing-heavy — the same record through hb(d, d) twice around
        // a rebind + meat-on-e with a re-pointed base.
        Seq {
            tag: "al".into(),
            mk_init: vec![(1, 10), (2, 20), (3, 30)],
            mk_loop: None,
            helpers: vec![Helper {
                ops: vec![HOp::PopD(2, -1), HOp::HasDel(3), HOp::Get(1, -9), HOp::Len],
            }],
            hb_k: 1,
            hb_k2: 1,
            base: 4,
            fk1: 2,
            fk2: 5,
            fv2: 15,
            meat_k: 2,
            use_e: true,
            ops: vec![
                DOp::CallBoth { aliased: true },
                DOp::CallH { h: 0, on_e: false },
                DOp::Rebind,
                DOp::SetBase(17),
                DOp::CallMeat { on_e: true },
                DOp::CallBoth { aliased: true },
                DOp::Store(6, 66),
            ],
        },
    ];

    // --- LCG random walks over the FULL alphabet ------------------------------
    let mut rng = Lcg(0x1313_B0DA_5EED_CAFE); // fixed seed → byte-stable corpus
    for w in 0..9 {
        let use_e = rng.below(3) != 0; // ~2/3 of walks bind the method factory
        let n_init = 1 + rng.below(3);
        let mut mk_init: Vec<(i64, i64)> = Vec::new();
        for _ in 0..n_init {
            let k = rng.key();
            if !mk_init.iter().any(|(ik, _)| *ik == k) {
                mk_init.push((k, rng.val()));
            }
        }
        // ~1/3 of factories relocate inside the callee (18+ fresh keys past
        // the n_init + 16 cap).
        let mk_loop = if rng.below(3) == 0 {
            let extra = 18 + rng.below(6) as i64;
            Some((50, 50 + extra, 1 + rng.below(4) as i64))
        } else {
            None
        };
        let n_helpers = 1 + rng.below(2);
        let helpers: Vec<Helper> = (0..n_helpers)
            .map(|_| {
                let n_ops = 2 + rng.below(3);
                let ops = (0..n_ops)
                    .map(|_| match rng.below(4) {
                        0 => HOp::PopD(rng.key(), -(1 + rng.below(9) as i64)),
                        1 => HOp::Get(rng.key(), -(1 + rng.below(9) as i64)),
                        2 => HOp::HasDel(rng.key()),
                        _ => HOp::Len,
                    })
                    .collect();
                Helper { ops }
            })
            .collect();
        let n_ops = 4 + rng.below(5);
        let mut ops: Vec<DOp> = Vec::new();
        for _ in 0..n_ops {
            let pick = rng.below(11);
            ops.push(match pick {
                0 | 1 => DOp::CallH {
                    h: rng.below(n_helpers),
                    on_e: use_e && rng.below(3) == 0,
                },
                2 => DOp::CallBoth {
                    aliased: !use_e || rng.below(2) == 0,
                },
                3 => DOp::CallMeat {
                    on_e: use_e && rng.below(3) == 0,
                },
                4 => DOp::Store(rng.key(), rng.val()),
                5 => DOp::StoreLoop {
                    s: 200 + 40 * w as i64,
                    e: 200 + 40 * w as i64 + 2 + rng.below(5) as i64,
                    m: 1 + rng.below(3) as i64,
                },
                6 => DOp::SetBase(1 + rng.below(20) as i64),
                7 => DOp::Rebind,
                8 => {
                    if use_e {
                        DOp::UpdateFrom
                    } else {
                        DOp::GuardDel(rng.key())
                    }
                }
                9 => DOp::PopAcc(rng.key(), -(1 + rng.below(5) as i64)),
                _ => DOp::GuardDel(rng.key()),
            });
        }
        seqs.push(Seq {
            tag: format!("rw{w}"),
            mk_init,
            mk_loop,
            helpers,
            hb_k: rng.key(),
            hb_k2: rng.key(),
            base: 3 + rng.below(9) as i64,
            fk1: rng.key(),
            fk2: 20 + rng.below(5) as i64,
            fv2: rng.val(),
            meat_k: rng.key(),
            use_e,
            ops,
        });
    }
    seqs
}

// ---- curated extra modules (shapes the Seq machine can't spell) ---------------

/// Each extra = one standalone MODULE (support defs + exactly one zero-arg
/// observable `<tag>_go`), distilled from the 24 PMAT-1313 hand probes. All
/// exported helpers are total on addr-0 records.
fn extra_modules() -> Vec<(&'static str, &'static str)> {
    vec![
        // The same dict passed TWICE — pop through one alias, read + del
        // through the other, called twice for compounding.
        (
            "xal",
            r#"def xal_two(a: dict[int, int], b: dict[int, int]) -> int:
    p: int = a.pop(1, -5)
    q: int = b.get(1, -9)
    if 2 in b:
        del a[2]
    return p * 100 + q * 10 + len(b)

def xal_go() -> int:
    d: dict[int, int] = {1: 6, 2: 8, 3: 9}
    acc: int = xal_two(d, d)
    acc = acc + xal_two(d, d)
    return acc * 10 + len(d) + d.get(3, -1)
"#,
        ),
        // Wide i64 keys AND values through both boundary directions.
        (
            "xw",
            r#"def xw_mkw() -> dict[int, int]:
    d: dict[int, int] = {1: 4000000007}
    d[2] = -4000000009
    d[3000000000] = 5
    return d

def xw_rw(d: dict[int, int]) -> int:
    return d.get(3000000000, -1) * 10 + d.pop(2, 0)

def xw_go() -> int:
    d = xw_mkw()
    acc: int = xw_rw(d)
    return acc + d.get(1, 0) + len(d)
"#,
        ),
        // Per-instance method-factory state: two objects, one field re-pointed.
        (
            "xi",
            r#"class XiAcc:
    def __init__(self) -> None:
        self.base: int = 5

    def mk(self) -> dict[int, int]:
        d: dict[int, int] = {1: self.base}
        return d

def xi_go() -> int:
    a: XiAcc = XiAcc()
    b: XiAcc = XiAcc()
    b.base = 9
    d = a.mk()
    e = b.mk()
    return d.get(1, -1) * 100 + e.get(1, -1)
"#,
        ),
        // A RETURNED set grown caller-side past the slack (2 + 22 adds).
        (
            "xs",
            r#"def xs_mk() -> set[int]:
    s: set[int] = {3, 5}
    return s

def xs_go() -> int:
    s = xs_mk()
    i: int = 10
    while i < 32:
        s.add(i)
        i = i + 1
    t: int = 0
    if 3 in s:
        t = 1
    if 31 in s:
        t = t + 2
    return len(s) * 10 + t
"#,
        ),
        // Re-binding the SAME name to a second factory's record.
        (
            "xr",
            r#"def xr_mk1() -> dict[int, int]:
    d: dict[int, int] = {1: 5}
    return d

def xr_mk2() -> dict[int, int]:
    d: dict[int, int] = {2: 7, 3: 9}
    return d

def xr_go() -> int:
    d = xr_mk1()
    a: int = d.get(1, -1)
    d = xr_mk2()
    return a * 100 + d.get(2, -1) * 10 + len(d)
"#,
        ),
        // Loop re-bind: each iteration re-points `d` at a FRESH record.
        (
            "xlr",
            r#"def xlr_mk() -> dict[int, int]:
    d: dict[int, int] = {1: 5}
    return d

def xlr_go() -> int:
    acc: int = 0
    i: int = 0
    while i < 3:
        d = xlr_mk()
        d[1] = i
        acc = acc + d.get(1, -1)
        i = i + 1
    return acc
"#,
        ),
        // Arg-eval order: a mutating pop and a read of the SAME record in
        // one arg list (CPython evaluates left-to-right).
        (
            "xao",
            r#"def xao_two(a: int, b: int) -> int:
    return a * 10 + b

def xao_go() -> int:
    d: dict[int, int] = {1: 5}
    return xao_two(d.pop(1, -1), d.get(1, -9))
"#,
        ),
        // Depth-3 free-fn chain, each level popping one key.
        (
            "xd",
            r#"def xd_h(d: dict[int, int]) -> int:
    p: int = d.pop(3, -1)
    return p

def xd_g(d: dict[int, int]) -> int:
    p: int = d.pop(2, -1)
    return p + xd_h(d)

def xd_f(d: dict[int, int]) -> int:
    p: int = d.pop(1, -1)
    return p + xd_g(d)

def xd_go() -> int:
    d: dict[int, int] = {1: 1, 2: 2, 3: 4, 4: 8}
    acc: int = xd_f(d)
    return acc * 10 + len(d)
"#,
        ),
        // Two records from the SAME factory: content-eq then diverge (str
        // values — the sv twin over RETURNED records).
        (
            "xe",
            r#"def xe_mk() -> dict[int, str]:
    d: dict[int, str] = {1: "ab"}
    return d

def xe_go() -> int:
    a = xe_mk()
    b = xe_mk()
    t: int = 0
    if a == b:
        t = 1
    b[1] = "ba"
    if a == b:
        t = t + 2
    return t
"#,
        ),
        // Callee relocation (19 inserts past cap 17) then a str-valued
        // content eq against an identically-built local record.
        (
            "xsv",
            r#"def xsv_mk() -> dict[int, str]:
    d: dict[int, str] = {0: "z"}
    i: int = 1
    while i < 20:
        d[i] = "ab"
        i = i + 1
    return d

def xsv_go() -> int:
    a = xsv_mk()
    m: dict[int, str] = {0: "z"}
    j: int = 1
    while j < 20:
        m[j] = "ab"
        j = j + 1
    if a == m:
        return len(a)
    return -1
"#,
        ),
        // A free-fn dict param handed ON to a method (param → method-arg).
        (
            "xh",
            r#"class XhAcc:
    def __init__(self) -> None:
        self.base: int = 3

    def eat(self, d: dict[int, int]) -> int:
        p: int = d.pop(1, -1)
        return p + self.base

def xh_free(d: dict[int, int], a: XhAcc) -> int:
    return a.eat(d) * 10 + d.get(2, -1)

def xh_go() -> int:
    d: dict[int, int] = {1: 6, 2: 8}
    a: XhAcc = XhAcc()
    acc: int = xh_free(d, a)
    return acc * 10 + len(d)
"#,
        ),
        // Recursion draining a dict param (self-call with the same pointer).
        (
            "xrec",
            r#"def xrec_drain(d: dict[int, int], k: int) -> int:
    if k <= 0:
        return len(d)
    p: int = d.pop(k, 0)
    return p + xrec_drain(d, k - 1)

def xrec_go() -> int:
    d: dict[int, int] = {1: 10, 2: 20, 3: 30}
    acc: int = xrec_drain(d, 3)
    return acc * 10 + len(d)
"#,
        ),
        // A MUTATING call in an arg list beside a read of the same record.
        (
            "xam",
            r#"def xam_eat(d: dict[int, int]) -> int:
    p: int = d.pop(1, -1)
    return p

def xam_two(a: int, b: int) -> int:
    return a * 10 + b

def xam_go() -> int:
    d: dict[int, int] = {1: 5, 2: 7}
    return xam_two(xam_eat(d), d.get(1, -9)) * 10 + len(d)
"#,
        ),
        // Guarded del + guarded clear through params, hit AND miss legs.
        (
            "xsc",
            r#"def xsc_scrub(d: dict[int, int], k: int) -> int:
    t: int = 0
    if k in d:
        del d[k]
        t = 1
    return t

def xsc_wipe(d: dict[int, int]) -> int:
    if len(d) > 0:
        d.clear()
    return len(d)

def xsc_go() -> int:
    d: dict[int, int] = {1: 5, 2: 7, 3: 9}
    a: int = xsc_scrub(d, 2)
    b: int = xsc_scrub(d, 77)
    n: int = len(d)
    w: int = xsc_wipe(d)
    d[4] = 1
    return a * 1000 + b * 100 + n * 10 + w + len(d) * 10000
"#,
        ),
        // Str-VALUED content through the boundary: a heap-built value
        // returned, then popped through a param and content-compared.
        (
            "xsp",
            r#"def xsp_sv(d: dict[int, str]) -> int:
    p: str = d.pop(1, "zz")
    if p == "ab":
        return 1
    return 0

def xsp_mkq() -> dict[int, str]:
    h: str = "a"
    d: dict[int, str] = {1: "x"}
    d[1] = h + "b"
    return d

def xsp_go() -> int:
    d = xsp_mkq()
    t: int = xsp_sv(d)
    e = xsp_mkq()
    u: int = xsp_sv(e)
    return t * 10 + u + len(d) * 100
"#,
        ),
        // A set param: discard (hit + miss) with caller-visible effect.
        (
            "xst",
            r#"def xst_chop(s: set[int], x: int) -> int:
    s.discard(x)
    t: int = 0
    if x in s:
        t = 1
    return t

def xst_go() -> int:
    s: set[int] = {1, 2, 3}
    a: int = xst_chop(s, 2)
    b: int = xst_chop(s, 99)
    return a * 100 + b * 10 + len(s)
"#,
        ),
        // A method-RETURNED record mutated caller-side, then handed BACK IN
        // as a method param of the same instance.
        (
            "xb",
            r#"class XbBox:
    def __init__(self) -> None:
        self.k: int = 2

    def mk(self) -> dict[int, int]:
        d: dict[int, int] = {1: 4, 2: 6}
        return d

    def eat(self, d: dict[int, int]) -> int:
        return d.pop(self.k, -1)

def xb_go() -> int:
    b: XbBox = XbBox()
    d = b.mk()
    d[3] = 8
    p: int = b.eat(d)
    return p * 100 + len(d) * 10 + d.get(3, -1)
"#,
        ),
        // Cleared THROUGH a param, then re-grown past the slack caller-side
        // (count 0, capacity kept — the growth path must still write back).
        (
            "xcg",
            r#"def xcg_wipe(d: dict[int, int]) -> int:
    if len(d) > 0:
        d.clear()
    return 0

def xcg_go() -> int:
    d: dict[int, int] = {1: 5}
    xcg_wipe(d)
    i: int = 0
    while i < 25:
        d[i] = i * 2
        i = i + 1
    return len(d) * 100 + d.get(24, -1)
"#,
        ),
        // A read and a mutating CALL compounded in one arithmetic expr.
        (
            "xrs",
            r#"def xrs_rsh(d: dict[int, int]) -> int:
    p: int = d.pop(0, -8)
    return p

def xrs_go() -> int:
    d: dict[int, int] = {0: 5, 1: 7}
    acc: int = d.get(0, -1) + xrs_rsh(d) * 7
    return acc * 10 + len(d)
"#,
        ),
    ]
}

/// Observable names for the extras (each module exports one `<tag>_go`).
fn extra_observables() -> Vec<String> {
    extra_modules()
        .iter()
        .map(|(tag, _)| format!("{tag}_go"))
        .collect()
}

// ---- CPython oracle -----------------------------------------------------------

/// `{observable → expected}` from `python3` running the IDENTICAL defs.
fn python_oracle(seqs: &[Seq]) -> Option<BTreeMap<String, i64>> {
    let mut prog = String::from("v={}\n");
    for seq in seqs {
        prog.push_str(&seq.support_source());
        for (name, def) in seq.observables() {
            prog.push_str(&def);
            prog.push_str(&format!("v['{name}']={name}()\n"));
        }
    }
    for (tag, src) in extra_modules() {
        prog.push_str(src);
        prog.push_str(&format!("v['{tag}_go']={tag}_go()\n"));
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
            "PMAT-1313: python3 oracle failed:\n{}",
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

// ---- WABT harness --------------------------------------------------------------

fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-bdyfuzz-{}-{}", std::process::id(), tag));
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
        "wat2wasm failed for {tag}:\n{}\n---WAT (first 4k)---\n{}",
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

/// Parse a `name() => i64:<value>` line. `wasm-interp` prints i64 UNSIGNED, so
/// a negative renders as its two's-complement `u64` — parse, reinterpret.
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

fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---- corpus-shape pins -----------------------------------------------------------

#[test]
fn corpus_is_deterministic() {
    let a = corpus();
    let b = corpus();
    assert_eq!(a.len(), b.len(), "corpus size unstable");
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.tag, y.tag, "tag order unstable");
        assert_eq!(
            x.module_source(),
            y.module_source(),
            "{}: source unstable",
            x.tag
        );
    }
}

/// The corpus must keep forcing REAL relocations on BOTH sides of the
/// boundary (callee-side before a return; caller-side on a returned record)
/// and covering the full driver-op alphabet — including the aliased
/// two-param call and the mid-sequence re-bind.
#[test]
fn corpus_forces_boundary_relocations_and_covers_the_alphabet() {
    let seqs = corpus();
    let by_tag = |t: &str| seqs.iter().find(|s| s.tag == t).expect("curated tag");

    assert!(
        by_tag("cg").replay().mk_relocs >= 1,
        "cg must relocate INSIDE the factory (callee-side, before the \
         return); if DICT_GROWTH_SLACK changed, update the mirror and \
         re-derive the loop bounds"
    );
    assert!(
        by_tag("kg").replay().driver_relocs >= 1,
        "kg must relocate CALLER-side on the returned record (the PMAT-1310 \
         growth escape hatch under real pressure)"
    );
    let callee_relocating = seqs.iter().filter(|s| s.replay().mk_relocs >= 1).count();
    assert!(
        callee_relocating >= 2,
        "only {callee_relocating} sequence(s) relocate inside a callee — \
         the LCG walks lost their growth loops"
    );

    let covered = |pred: &dyn Fn(&DOp) -> bool| seqs.iter().any(|s| s.ops.iter().any(pred));
    assert!(
        covered(&|o| matches!(o, DOp::CallH { .. })),
        "alphabet lost the mutating helper call"
    );
    assert!(
        covered(&|o| matches!(o, DOp::CallBoth { aliased: true })),
        "alphabet lost the ALIASED two-param call (hb(d, d))"
    );
    assert!(
        covered(&|o| matches!(o, DOp::CallBoth { aliased: false })),
        "alphabet lost the unaliased two-param call"
    );
    assert!(
        covered(&|o| matches!(o, DOp::CallMeat { .. })),
        "alphabet lost the method-boundary call"
    );
    assert!(
        covered(&|o| matches!(o, DOp::Store(..))),
        "alphabet lost the caller-side store"
    );
    assert!(
        covered(&|o| matches!(o, DOp::StoreLoop { .. })),
        "alphabet lost the caller-side store loop"
    );
    assert!(
        covered(&|o| matches!(o, DOp::SetBase(_))),
        "alphabet lost the field re-point (c.base = n)"
    );
    assert!(
        covered(&|o| matches!(o, DOp::Rebind)),
        "alphabet lost the mid-sequence re-bind"
    );
    assert!(
        covered(&|o| matches!(o, DOp::UpdateFrom)),
        "alphabet lost the returned-record merge (d.update(e))"
    );
    assert!(
        covered(&|o| matches!(o, DOp::PopAcc(..))),
        "alphabet lost the inline-arithmetic pop"
    );
    assert!(
        covered(&|o| matches!(o, DOp::GuardDel(_))),
        "alphabet lost the guarded del"
    );
    assert!(
        seqs.iter().any(|s| s.use_e),
        "corpus lost the method-factory binding"
    );
    assert!(
        seqs.iter().any(|s| s.use_e
            && s.ops
                .iter()
                .any(|o| matches!(o, DOp::CallMeat { on_e: true }))),
        "no sequence feeds the METHOD factory's record back into a method"
    );
}

// ---- EMIT-path pins (run without WABT) --------------------------------------------

/// Every sequence lowers through the FULL pipeline and rides the i32
/// base-pointer ABI on BOTH boundary directions and BOTH callable kinds.
#[test]
fn fuzz_corpus_lowers_on_the_i32_boundary_abi() {
    for seq in &corpus() {
        let wat = emit(&seq.module_source())
            .unwrap_or_else(|e| panic!("sequence {} must lower: {e}", seq.tag));
        assert!(
            wat.contains(&format!("(func ${} (result i32)", seq.mk_name())),
            "{}: the factory must RETURN the record as an i32 base-pointer",
            seq.tag
        );
        assert!(
            wat.contains(&format!("(func ${} (param $d i32)", seq.h_name(0))),
            "{}: the helper's dict param must ride the i32 reference ABI",
            seq.tag
        );
        assert!(
            wat.contains(&format!(
                "(func ${}.meat (param $self i32) (param $d i32)",
                seq.class_name()
            )),
            "{}: the METHOD's dict param must ride the same ABI after $self",
            seq.tag
        );
        assert!(
            wat.contains(&format!(
                "(func ${}.mfac (param $self i32) (result i32)",
                seq.class_name()
            )),
            "{}: the METHOD factory must return the record as i32",
            seq.tag
        );
    }
    for (tag, src) in extra_modules() {
        emit(src).unwrap_or_else(|e| panic!("extra module {tag} must lower: {e}"));
    }
}

/// The boundary's load-bearing refusal belts, pinned through the FULL
/// pipeline: every growth-capable op through a param (free-fn AND method,
/// all five spellings), the kind-mismatched re-bind, and the branch-selected
/// bind. If any starts lowering, it must arrive WITH an executed witness —
/// not by a belt silently loosening.
#[test]
fn boundary_growth_and_bind_belts_hold() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "fresh-key store through a free-fn param",
            "def f(d: dict[int, int]) -> int:\n    d[99] = 1\n    return len(d)\n",
            "grow + relocate",
        ),
        (
            "setdefault through a free-fn param",
            "def f(d: dict[int, int]) -> int:\n    d.setdefault(9, 1)\n    return len(d)\n",
            "grow + relocate",
        ),
        (
            "update through a free-fn param",
            "def f(d: dict[int, int], o: dict[int, int]) -> int:\n    d.update(o)\n    return len(d)\n",
            "grow + relocate",
        ),
        (
            "|= through a free-fn param",
            "def f(d: dict[int, int], o: dict[int, int]) -> int:\n    d |= o\n    return len(d)\n",
            "grow + relocate",
        ),
        (
            "set add through a free-fn param",
            "def f(s: set[int]) -> int:\n    s.add(9)\n    return len(s)\n",
            "grow + relocate",
        ),
        (
            "store through a METHOD param",
            "class A:\n    def __init__(self) -> None:\n        self.x: int = 0\n\n    def m(self, d: dict[int, int]) -> int:\n        d[99] = 1\n        return len(d)\n",
            "grow + relocate",
        ),
        (
            "setdefault through a METHOD param",
            "class A:\n    def __init__(self) -> None:\n        self.x: int = 0\n\n    def m(self, d: dict[int, int]) -> int:\n        d.setdefault(9, 1)\n        return len(d)\n",
            "grow + relocate",
        ),
        (
            "re-binding an i-keyed name from an s-keyed factory",
            "def mki() -> dict[int, int]:\n    d: dict[int, int] = {1: 5}\n    return d\n\ndef mks() -> dict[str, int]:\n    d: dict[str, int] = {'a': 7}\n    return d\n\ndef go() -> int:\n    d = mki()\n    d = mks()\n    return len(d)\n",
            "key encoding",
        ),
        (
            "branch-SELECTED dict bind (no straight-line Let)",
            "def mk1() -> dict[int, int]:\n    d: dict[int, int] = {1: 5}\n    return d\n\ndef go() -> int:\n    c: int = 1\n    if c > 0:\n        d = mk1()\n    else:\n        d = mk1()\n    return len(d)\n",
            "LITERAL",
        ),
    ];
    for (what, src, needle) in cases {
        let err = emit(src)
            .map(|_| ())
            .expect_err(&format!("{what} must refuse"));
        assert!(
            err.contains(needle),
            "{what}: refusal should mention {needle:?}, got: {err}"
        );
    }
}

// ---- the executed differential -----------------------------------------------------

#[test]
fn dict_boundary_fuzz_matches_cpython() {
    let seqs = corpus();

    // EMIT path holds regardless of WABT.
    let mut modules: Vec<(String, String)> = seqs
        .iter()
        .map(|seq| (seq.tag.clone(), emit(&seq.module_source()).expect("lowers")))
        .collect();
    for (tag, src) in extra_modules() {
        modules.push((tag.to_string(), emit(src).expect("extra lowers")));
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1313: skipping EXECUTED boundary fuzz — WABT (wat2wasm / \
             wasm-interp) absent. Every sequence lowered through emit_module \
             on the i32 boundary ABI; a box with WABT + python3 runs every \
             observable (REAL callee-side and caller-side relocations, \
             aliased params, method factories included) and value-matches \
             live CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1313: skipping fuzz value-diff — python3 (the oracle) absent.");
        return;
    }
    let oracle = match python_oracle(&seqs) {
        Some(o) => o,
        None => {
            eprintln!("PMAT-1313: python3 oracle unavailable — skipping value diff.");
            return;
        }
    };

    // Model honesty, part 1: the Rust replay's acc fold must MATCH CPython
    // for every sequence — the replay engine (helper effects, aliasing,
    // meat defaults under SetBase, rebinds, merges) is pinned end-to-end
    // against the oracle before it is trusted to spell eqm/eqn.
    for seq in &seqs {
        let name = format!("{}_acc", seq.tag);
        let want = *oracle
            .get(&name)
            .unwrap_or_else(|| panic!("oracle missing {name}"));
        assert_eq!(
            seq.replay().acc,
            want,
            "{name}: the Rust replay's acc fold diverged from CPython — the \
             replay model no longer mirrors the boundary semantics"
        );
    }

    // Model honesty, part 2: the replay model that SPELLED eqm/eqn must
    // agree with CPython — every eqm is 1, every eqn is 0, ON THE ORACLE SIDE.
    for (name, want) in &oracle {
        if name.ends_with("_eqm") {
            assert_eq!(
                *want, 1,
                "{name}: the replay model diverged from CPython (oracle says \
                 the receiver != the model literal) — fix the replay, the \
                 observable is vacuous otherwise"
            );
        }
        if name.ends_with("_eqn") {
            assert_eq!(
                *want, 0,
                "{name}: the flipped model literal must NOT equal the \
                 receiver on the oracle side"
            );
        }
    }

    let mut names: Vec<(String, Vec<String>)> = seqs
        .iter()
        .map(|seq| {
            (
                seq.tag.clone(),
                seq.observables().into_iter().map(|(n, _)| n).collect(),
            )
        })
        .collect();
    for name in extra_observables() {
        let tag = name.trim_end_matches("_go").to_string();
        names.push((tag, vec![name]));
    }

    let mut checked = 0usize;
    for ((tag, wat), (tag2, obs_names)) in modules.iter().zip(&names) {
        assert_eq!(tag, tag2, "module/name zip drift");
        let (stdout, ok) = assemble_and_run(tag, wat);
        assert!(ok, "wasm-interp failed for {tag}:\n{stdout}");
        assert!(
            !stdout.contains("unreachable executed"),
            "{tag}: no fuzz observable may trap:\n{stdout}"
        );
        for name in obs_names {
            let got = parse_scalar(&stdout, name);
            let want = *oracle
                .get(name)
                .unwrap_or_else(|| panic!("oracle missing {name}"));
            assert_eq!(
                got, want,
                "DIVERGENCE {name}: WASM = {got}, CPython = {want}\n\
                 (a boundary miscompile — a stale pointer after relocation, \
                 a mutation lost through a param, an aliasing bug, a wrong \
                 eval order at a call site, or a bad re-bind)\n\
                 interp output:\n{stdout}"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 100,
        "fuzz breadth regressed: only {checked} observables checked"
    );
    eprintln!(
        "PMAT-1313: EXECUTED boundary fuzz PASSED — {checked} observables \
         across {} sequences (+{} extras) == live CPython; callee-side and \
         caller-side relocations, aliased params, method factories, re-binds \
         and returned-record merges all covered.",
        seqs.len(),
        extra_modules().len()
    );
}
