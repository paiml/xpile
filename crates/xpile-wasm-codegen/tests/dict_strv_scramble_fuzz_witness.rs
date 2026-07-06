//! PMAT-1308 — the ADVERSARIAL-VERIFY differential fuzz for the str-VALUED
//! dict surface shipped by PMAT-1305 (stores/reads), PMAT-1306 (`pop`/
//! `setdefault` str legs) and PMAT-1307 (the content-comparing `==` twin) —
//! the ~4-slices-since-PMAT-1303 skeptic pass over the newest lane. The full
//! str-value mutation alphabet — `d[k] = "s"` / `d[k] = h + "x"` (HEAP-built
//! store) / `del d[k]` / `s = d.pop(k)` / bare `d.pop(k)` / `d.pop(k, dflt)` /
//! `d.setdefault(k, dflt)` (bound AND bare) / `d.update(o)` / `d |= o` /
//! `d.clear()` — is interleaved by a fixed-seed LCG over BOTH key kinds and
//! value-matched against LIVE CPython (`python3`) on the IDENTICAL source.
//!
//! ## What the skeptic pass targets (and what it found)
//!
//! 1. **eq-after-scramble was never pinned.** PMAT-1307's probes are
//!    hand-curated: literal dicts, one del, one convergence walk. Nothing
//!    compares a receiver whose STORAGE was scrambled by an interleaved
//!    pop/del/setdefault/update/clear history — swap-into-hole reorder,
//!    relocated regions, heap-built value slots — against a fresh all-literal
//!    spelling of the same CONTENT. Every fuzz sequence here carries an `eqm`
//!    observable (scrambled receiver `==` literal model, expect 1) and an
//!    `eqn` twin (one value's content flipped, expect 0), so the sv twin's
//!    size gate + walk-p + membership probe are exercised over every storage
//!    shape the alphabet can reach.
//! 2. **The setdefault-miss GROW path shipped relocation-unwitnessed** — the
//!    exact hollow-grow class PMAT-1303 caught in `update`: PMAT-1306's
//!    witness inserts a handful of keys, never outrunning the 16-slot literal
//!    slack, so `$__wasm_dict_set_<k>`'s 2x-grow + write-back under a
//!    SETDEFAULT (str default, bound result) was never executed. The `sg`
//!    sequence drives 20 setdefault-misses through a size-1 receiver (one
//!    real relocation) and reads pre-grow + post-grow values back; `gm`
//!    forces a DOUBLE relocation (17→34→68) with str values through one
//!    `update`; `corpus_forces_real_relocations` pins the capacity arithmetic
//!    against the mirrored slack so the corpus keeps outrunning it.
//! 3. **The accepted mutation-inside-store shape `d[k] = d.pop(j)`.** The
//!    side-effecting-expr belts (PMAT-1306) refuse a pop inside a concat /
//!    f-string / lazily-lowered default — but a pop as the VALUE of a
//!    subscript store is ACCEPTED, and no witness executed it: the pop
//!    mutates the receiver (swap-into-hole) while the store's base-pointer /
//!    key / value operands are being evaluated. `x_setpop_*` pin the fresh-key,
//!    same-key (delete-then-reinsert semantics: CPython evaluates the RHS
//!    first) and content-preserving variants.
//! 4. **Bound reads through moved slots.** `p = d.pop(k)` / `p =
//!    d.setdefault(k, dflt)` results are folded (position-weighted) into every
//!    sequence's `pl` observable and content-checked (`pc`), so a pop that
//!    returns the WRONG slot after a swap/relocation — or a setdefault that
//!    returns the default where CPython returns the present value — diverges.
//!
//! ## The pointer-identity trap, carried through the fuzz
//!
//! The static literal region dedupes by CONTENT, so all-literal corpora hide
//! address-compare miscompiles (the PMAT-1305/1306/1307 standing lesson). The
//! alphabet therefore includes `SetHeap` — a concat-BUILT value store (`h0:
//! str = "v"` … `d[k] = h0 + "v"`): equal bytes at a FRESH address. Curated
//! sequences (`hs`, `ss1`) put heap-built values on the receiver side of
//! `eqm`/`vc`/`pc`, and the extras pin `d.pop(i) == e.pop(j)` /
//! `d.pop(i) < e.pop(j)` with a heap-built side (the ordering pin stores the
//! LEXICOGRAPHICALLY-SMALLER content at the LARGER address, so an address
//! compare inverts it).
//!
//! ## Mutation-verified teeth
//!
//! Both seeded miscompiles are KILLED by the executed differential:
//! * routing str-valued dict `==` back to the int-lane pointer-eq helper
//!   (the PMAT-1307 mutation) → `ps_eqm` diverges (WASM 0, CPython 1);
//! * dropping the setdefault-miss write-back (`local.set` → `drop`) →
//!   the `sg` sequence TRAPS reading through the stale pre-relocation base —
//!   and ONLY a relocating corpus can see this, which is finding #2's point.
//!
//! ## What was probed and did NOT refute (the honest-refusal belt holds)
//!
//! Every memory-flagged hazard of the str lane refuses through the FULL
//! pipeline with a precise diagnostic, pinned in
//! `sideeffect_and_multi_eval_shapes_refuse`:
//! * a side-effecting `d.pop(...)` inside the LAZILY-lowered default of
//!   `d.get(k, dflt)` / `d.pop(k, dflt)` / `d.setdefault(k, dflt)` (CPython
//!   evaluates arguments EAGERLY — the PMAT-1306 int-lane divergence class);
//! * a pop inside a CONCAT / f-string (the length+copy passes re-evaluate
//!   operands — the multi-eval class);
//! * MIXED value-kind `==` and `update` (`dict[K, int]` vs `dict[K, str]`);
//! * `.values()` / `.items()` iteration over a str-valued dict (int-only).
//!
//! Each observable is a standalone `def NAME() -> int` (valid plain `python3`
//! AND wasm-frontend-lowerable); one sequence = ONE module (its own fresh
//! single-page bump heap); the IDENTICAL text feeds both lanes, so the oracle
//! has ZERO reimplementation risk. Deterministic fixed-seed LCG — no `rand`,
//! no time, byte-stable corpus. The value alphabet reuses 8 short contents
//! (the 512-byte literal region dedupes by content — distinct CONTENTS are
//! the scarce resource, not uses).
//!
//! ## Gating
//!
//! The executed diff needs WABT (`wat2wasm` / `wasm-interp`) AND `python3`;
//! without either it skips cleanly after asserting the EMIT path + helper
//! carriage for every sequence. Refusal pins run on the emit path alone.
//! CITES `C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP` (test-only; no new
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
    /// `count` DISTINCT values drawn from `pool`.
    fn sample(&mut self, pool: &[i64], count: usize) -> Vec<i64> {
        let mut avail: Vec<i64> = pool.to_vec();
        let mut out = Vec::with_capacity(count);
        for _ in 0..count.min(avail.len()) {
            let i = self.below(avail.len());
            out.push(avail.remove(i));
        }
        out
    }
}

// ---- the value-content alphabet ----------------------------------------------

/// 8 distinct short contents — lengths 1..=6 make `len(d[k])` a discriminating
/// observable; `"ab"`/`"ba"` catch equal-length content swaps. Small on
/// purpose: the static literal region (512 bytes) dedupes by content.
const CONTENTS: &[&str] = &["u", "vv", "www", "xxxx", "yyyyy", "zzzzzz", "ab", "ba"];

/// Contents eligible for a HEAP-built (`SetHeap`) store: `first + "rest"`
/// needs `len >= 2`.
const HEAP_CIDS: &[usize] = &[1, 2, 3, 4, 5, 6, 7];

// ---- the mutation-op alphabet ------------------------------------------------

/// One receiver mutation over `dict[K, str]`. Content ids index [`CONTENTS`].
/// `Update`/`PipeEq` name a source dict by index into `Seq::srcs` — both lower
/// to `Stmt::DictUpdate`.
#[derive(Clone)]
enum Op {
    /// `d[k] = "<content>"` — a literal (region-deduped) value store.
    Set(i64, usize),
    /// `h<i>: str = "<first>"` then `d[k] = h<i> + "<rest>"` — the SAME
    /// content at a FRESH heap address (defeats literal dedup; an address
    /// compare downstream diverges).
    SetHeap(i64, usize),
    Del(i64),
    /// `p<j>: str = d.pop(k)` — the bound result is folded into `pl`/`pc`.
    PopBind(i64),
    /// bare `d.pop(k)` — the result is discarded (statement position).
    PopStmt(i64),
    /// `p<j>: str = d.pop(k, "<content>")` — key may be live OR dead.
    PopDefault(i64, usize),
    /// `p<j>: str = d.setdefault(k, "<content>")` — present returns the
    /// stored value, absent inserts (the miss path may GROW + relocate).
    SetDefault(i64, usize),
    /// bare `d.setdefault(k, "<content>")`.
    SetDefaultStmt(i64, usize),
    Update(usize),
    PipeEq(usize),
    Clear,
}

/// A literal source dict bound before the ops (`o<idx>`), never itself
/// mutated. Pairs are `(key, content-id)`.
#[derive(Clone)]
struct SrcDict {
    pairs: Vec<(i64, usize)>,
}

/// One fuzz sequence: a receiver literal + bound sources + an interleaved op
/// list, observed through every applicable reduction / read-back / eq probe.
struct Seq {
    tag: String,
    str_keyed: bool,
    init: Vec<(i64, usize)>,
    srcs: Vec<SrcDict>,
    ops: Vec<Op>,
    /// Keys read back via `len(d[k])` + a content check — must be live at
    /// sequence end (checked by `realize()`).
    gets: Vec<i64>,
}

/// The replayed sequence: emitted source lines, bound-result expectations,
/// and the final content model.
struct Realized {
    lines: Vec<String>,
    /// `(local name, expected content)` for every `p<j>` binding, in order.
    bound: Vec<(String, String)>,
    model: BTreeMap<i64, String>,
}

impl Seq {
    /// Key spelling: an int key literal, or the bijective `'k<n>'` for the
    /// str-keyed lane.
    fn key_txt(&self, k: i64) -> String {
        if self.str_keyed {
            format!("'k{k}'")
        } else {
            k.to_string()
        }
    }

    fn dict_ann(&self) -> &'static str {
        if self.str_keyed {
            "dict[str, str]"
        } else {
            "dict[int, str]"
        }
    }

    fn lit(&self, pairs: &[(i64, usize)]) -> String {
        let entries: Vec<String> = pairs
            .iter()
            .map(|(k, c)| format!("{}: \"{}\"", self.key_txt(*k), CONTENTS[*c]))
            .collect();
        format!("{{{}}}", entries.join(", "))
    }

    /// Replay the ops: emit the shared build-and-mutate prefix AND the
    /// content model in one pass, asserting the corpus is trap-free (dels /
    /// 1-arg pops / gets hit live keys).
    fn realize(&self) -> Realized {
        let mut lines = Vec::new();
        let mut bound: Vec<(String, String)> = Vec::new();
        let mut model: BTreeMap<i64, String> = self
            .init
            .iter()
            .map(|(k, c)| (*k, CONTENTS[*c].to_string()))
            .collect();
        lines.push(format!("d: {} = {}", self.dict_ann(), self.lit(&self.init)));
        for (i, s) in self.srcs.iter().enumerate() {
            lines.push(format!(
                "o{i}: {} = {}",
                self.dict_ann(),
                self.lit(&s.pairs)
            ));
        }
        for (oi, op) in self.ops.iter().enumerate() {
            match op {
                Op::Set(k, c) => {
                    lines.push(format!("d[{}] = \"{}\"", self.key_txt(*k), CONTENTS[*c]));
                    model.insert(*k, CONTENTS[*c].to_string());
                }
                Op::SetHeap(k, c) => {
                    let content = CONTENTS[*c];
                    assert!(content.len() >= 2, "{}: SetHeap needs len >= 2", self.tag);
                    let (first, rest) = content.split_at(1);
                    lines.push(format!("h{oi}: str = \"{first}\""));
                    lines.push(format!("d[{}] = h{oi} + \"{rest}\"", self.key_txt(*k)));
                    model.insert(*k, content.to_string());
                }
                Op::Del(k) => {
                    assert!(
                        model.remove(k).is_some(),
                        "{}: del of a dead key {k} would trap",
                        self.tag
                    );
                    lines.push(format!("del d[{}]", self.key_txt(*k)));
                }
                Op::PopBind(k) => {
                    let name = format!("p{}", bound.len());
                    let content = model.remove(k).unwrap_or_else(|| {
                        panic!("{}: 1-arg pop of a dead key {k} would trap", self.tag)
                    });
                    lines.push(format!("{name}: str = d.pop({})", self.key_txt(*k)));
                    bound.push((name, content));
                }
                Op::PopStmt(k) => {
                    assert!(
                        model.remove(k).is_some(),
                        "{}: bare pop of a dead key {k} would trap",
                        self.tag
                    );
                    lines.push(format!("d.pop({})", self.key_txt(*k)));
                }
                Op::PopDefault(k, c) => {
                    let name = format!("p{}", bound.len());
                    let content = model.remove(k).unwrap_or_else(|| CONTENTS[*c].to_string());
                    lines.push(format!(
                        "{name}: str = d.pop({}, \"{}\")",
                        self.key_txt(*k),
                        CONTENTS[*c]
                    ));
                    bound.push((name, content));
                }
                Op::SetDefault(k, c) => {
                    let name = format!("p{}", bound.len());
                    let content = model
                        .entry(*k)
                        .or_insert_with(|| CONTENTS[*c].to_string())
                        .clone();
                    lines.push(format!(
                        "{name}: str = d.setdefault({}, \"{}\")",
                        self.key_txt(*k),
                        CONTENTS[*c]
                    ));
                    bound.push((name, content));
                }
                Op::SetDefaultStmt(k, c) => {
                    model.entry(*k).or_insert_with(|| CONTENTS[*c].to_string());
                    lines.push(format!(
                        "d.setdefault({}, \"{}\")",
                        self.key_txt(*k),
                        CONTENTS[*c]
                    ));
                }
                Op::Update(i) => {
                    for (k, c) in &self.srcs[*i].pairs {
                        model.insert(*k, CONTENTS[*c].to_string());
                    }
                    lines.push(format!("d.update(o{i})"));
                }
                Op::PipeEq(i) => {
                    for (k, c) in &self.srcs[*i].pairs {
                        model.insert(*k, CONTENTS[*c].to_string());
                    }
                    lines.push(format!("d |= o{i}"));
                }
                Op::Clear => {
                    model.clear();
                    lines.push("d.clear()".to_string());
                }
            }
        }
        for k in &self.gets {
            assert!(model.contains_key(k), "{}: get of a dead key {k}", self.tag);
        }
        Realized {
            lines,
            bound,
            model,
        }
    }

    /// Every observation over the mutated receiver (each a standalone def):
    /// `len` / key reductions (int-keyed) / per-get value len + CONTENT check
    /// / the bound-pop fold + content check / the eq-after-scramble pair
    /// (`eqm` expect 1, `eqn` expect 0) / source independence.
    fn observables(&self) -> Vec<(String, String)> {
        let realized = self.realize();
        let common = &realized.lines;
        let mut tails: Vec<(String, String)> = vec![("len".into(), "return len(d)".into())];
        if !self.str_keyed {
            tails.push(("sumk".into(), "return sum(d)".into()));
            if !realized.model.is_empty() {
                tails.push(("mn".into(), "return min(d)".into()));
                tails.push(("mx".into(), "return max(d)".into()));
                tails.push(("srt0".into(), "xs = sorted(d)\n    return xs[0]".into()));
            }
        }
        for (j, k) in self.gets.iter().enumerate() {
            let content = &realized.model[k];
            tails.push((
                format!("vl{j}"),
                format!("return len(d[{}])", self.key_txt(*k)),
            ));
            tails.push((
                format!("vc{j}"),
                format!(
                    "if d[{}] == \"{content}\":\n        return 1\n    return 0",
                    self.key_txt(*k)
                ),
            ));
        }
        if !realized.bound.is_empty() {
            // Position-weighted fold: a swap between two equal-length bound
            // results still diverges. len <= 6 and 7^j keep the fold in i64.
            let fold: Vec<String> = realized
                .bound
                .iter()
                .enumerate()
                .map(|(j, (name, _))| format!("len({name}) * {}", 7i64.pow(j as u32)))
                .collect();
            tails.push(("pl".into(), format!("return {}", fold.join(" + "))));
            let (name, content) = &realized.bound[0];
            tails.push((
                "pc".into(),
                format!("if {name} == \"{content}\":\n        return 1\n    return 0"),
            ));
        }
        // eq-after-scramble: the receiver (scrambled storage, heap-built value
        // slots) vs a FRESH all-literal spelling of the model. Order-spelled
        // sorted-by-key — insertion order differs from storage order by
        // construction.
        let model_pairs: Vec<(i64, usize)> = realized
            .model
            .iter()
            .map(|(k, content)| {
                let cid = CONTENTS
                    .iter()
                    .position(|c| c == content)
                    .expect("model contents come from the alphabet");
                (*k, cid)
            })
            .collect();
        tails.push((
            "eqm".into(),
            format!(
                "m: {} = {}\n    if d == m:\n        return 1\n    return 0",
                self.dict_ann(),
                self.lit(&model_pairs)
            ),
        ));
        if let Some((flip_key, flip_cid)) = model_pairs.first().copied() {
            // One value's content flipped (append "!") — expect 0.
            let entries: Vec<String> = model_pairs
                .iter()
                .map(|(k, c)| {
                    let content = if *k == flip_key {
                        format!("{}!", CONTENTS[flip_cid])
                    } else {
                        CONTENTS[*c].to_string()
                    };
                    format!("{}: \"{content}\"", self.key_txt(*k))
                })
                .collect();
            tails.push((
                "eqn".into(),
                format!(
                    "m: {} = {{{}}}\n    if d == m:\n        return 1\n    return 0",
                    self.dict_ann(),
                    entries.join(", ")
                ),
            ));
        }
        if !self.srcs.is_empty() {
            // The LAST-used source must be unmutated by any merge (update
            // copies entries; it must not share storage).
            let last = self.srcs.len() - 1;
            let (k, _) = self.srcs[last].pairs[0];
            tails.push((
                "srcv".into(),
                format!(
                    "return len(o{last}[{}]) * 10 + len(o{last})",
                    self.key_txt(k)
                ),
            ));
        }
        let mut out = Vec::new();
        for (suffix, tail) in tails {
            let name = format!("{}_{suffix}", self.tag);
            let mut src = format!("def {name}() -> int:\n");
            for line in common {
                src.push_str(&format!("    {line}\n"));
            }
            src.push_str(&format!("    {tail}\n"));
            out.push((name, src));
        }
        out
    }

    fn wasm_source(&self) -> String {
        let mut src = String::new();
        for (_, def) in self.observables() {
            src.push_str(&def);
            src.push('\n');
        }
        src
    }

    fn has_pop(&self) -> bool {
        self.ops
            .iter()
            .any(|o| matches!(o, Op::PopBind(_) | Op::PopStmt(_) | Op::PopDefault(..)))
    }
}

// ---- capacity model (the PMAT-1303 hollow-grow lesson, kept executable) -------

/// The dict growth slack (`DICT_GROWTH_SLACK` in the codegen): a literal's
/// capacity is `count + 16`. Mirrored here (the constant is private) so the
/// corpus can PROVE its curated sequences outrun the slack; if the slack ever
/// changes, `corpus_forces_real_relocations` fails loudly instead of the grow
/// pins silently degrading into plain appends.
const MIRRORED_GROWTH_SLACK: usize = 16;

/// Replay a sequence against the capacity model, counting RELOCATIONS (the
/// `count >= capacity` doublings inside `$__wasm_dict_set_<k>`). `clear`
/// zeroes the count but keeps the capacity (a bare header write).
fn relocations(seq: &Seq) -> usize {
    let mut live: BTreeMap<i64, usize> = seq.init.iter().copied().collect();
    let mut count = live.len();
    let mut cap = live.len() + MIRRORED_GROWTH_SLACK;
    let mut relocs = 0;
    let mut insert = |live: &mut BTreeMap<i64, usize>, count: &mut usize, k: i64, c: usize| {
        if live.insert(k, c).is_none() {
            if *count >= cap {
                cap *= 2;
                relocs += 1;
            }
            *count += 1;
        }
    };
    for op in &seq.ops {
        match op {
            Op::Set(k, c) | Op::SetHeap(k, c) => insert(&mut live, &mut count, *k, *c),
            Op::Del(k) | Op::PopBind(k) | Op::PopStmt(k) => {
                live.remove(k);
                count -= 1;
            }
            Op::PopDefault(k, _) => {
                if live.remove(k).is_some() {
                    count -= 1;
                }
            }
            Op::SetDefault(k, c) | Op::SetDefaultStmt(k, c) => {
                if !live.contains_key(k) {
                    insert(&mut live, &mut count, *k, *c);
                }
            }
            Op::Update(i) | Op::PipeEq(i) => {
                for (k, c) in &seq.srcs[*i].pairs {
                    insert(&mut live, &mut count, *k, *c);
                }
            }
            Op::Clear => {
                live.clear();
                count = 0;
            }
        }
    }
    relocs
}

// ---- corpus -------------------------------------------------------------------

/// `n` fresh `(key, content-id)` pairs starting at `start`.
fn fresh_pairs(start: i64, n: i64) -> Vec<(i64, usize)> {
    (0..n)
        .map(|i| (start + i, ((start + i) % CONTENTS.len() as i64) as usize))
        .collect()
}

fn corpus() -> Vec<Seq> {
    let mut seqs: Vec<Seq> = vec![
        // sg: the SETDEFAULT-miss grow path under a REAL relocation — 20
        // misses through a size-1 receiver (cap 17 → one doubling), str
        // defaults, then pre-grow + post-grow value reads and the eq pair.
        Seq {
            tag: "sg".into(),
            str_keyed: false,
            init: vec![(1, 0)],
            srcs: vec![],
            ops: (0..20)
                .map(|i| Op::SetDefaultStmt(100 + i, (i % CONTENTS.len() as i64) as usize))
                .collect(),
            gets: vec![1, 100, 119],
        },
        // gm: DOUBLE relocation (17→34→68) with str values through ONE update.
        Seq {
            tag: "gm".into(),
            str_keyed: false,
            init: vec![(1, 0)],
            srcs: vec![SrcDict {
                pairs: fresh_pairs(100, 40),
            }],
            ops: vec![Op::Update(0)],
            gets: vec![1, 116, 139],
        },
        // gp: the PEP 584 spelling forces the same relocation, then a bound
        // pop reads a value slot MOVED by the relocation.
        Seq {
            tag: "gp".into(),
            str_keyed: false,
            init: vec![(1, 1)],
            srcs: vec![SrcDict {
                pairs: fresh_pairs(100, 20),
            }],
            ops: vec![Op::PipeEq(0), Op::PopBind(110)],
            gets: vec![1, 119],
        },
        // ps: pop-scramble — the full pop family interleaved with stores over
        // an 8-key receiver; swap-into-hole moves str value slots around, a
        // present-key setdefault must return the HEAP-built stored value.
        Seq {
            tag: "ps".into(),
            str_keyed: false,
            init: (1..=8).map(|k| (k, (k % 8) as usize)).collect(),
            srcs: vec![],
            ops: vec![
                Op::PopBind(3),
                Op::Set(9, 4),
                Op::PopStmt(1),
                Op::Del(8),
                Op::SetHeap(2, 6),
                Op::PopDefault(5, 2),
                Op::PopDefault(77, 3),
                Op::SetDefault(10, 5),
                Op::SetDefault(2, 0),
                Op::PopBind(7),
            ],
            gets: vec![2, 9, 10],
        },
        // hs: heap-heavy — every live value slot ends up concat-BUILT, so the
        // eqm compare (vs an all-literal model) is content-critical in every
        // slot, and a bound pop returns a heap-built value.
        Seq {
            tag: "hs".into(),
            str_keyed: false,
            init: vec![(1, 0)],
            srcs: vec![],
            ops: vec![
                Op::SetHeap(2, 1),
                Op::SetHeap(3, 6),
                Op::SetHeap(4, 3),
                Op::PopBind(3),
                Op::Set(5, 7),
                Op::SetHeap(1, 4),
            ],
            gets: vec![1, 2, 4, 5],
        },
        // cl: clear-rebuild — count drops to 0 (capacity kept), then the
        // receiver is rebuilt via setdefault + update; eqm walks the rebuilt
        // region, a dead-key pop-with-default must NOT mutate.
        Seq {
            tag: "cl".into(),
            str_keyed: false,
            init: vec![(1, 0), (2, 1), (3, 2)],
            srcs: vec![SrcDict {
                pairs: vec![(2, 5), (6, 0)],
            }],
            ops: vec![
                Op::Set(4, 3),
                Op::Clear,
                Op::SetDefaultStmt(5, 4),
                Op::SetDefault(1, 6),
                Op::Update(0),
                Op::PopDefault(9, 7),
            ],
            gets: vec![1, 2, 5, 6],
        },
        // ow: overwrite churn on ONE key across literal / heap-built / literal
        // stores, a present-key setdefault no-op, then pop reads the LAST
        // store and a re-insert follows.
        Seq {
            tag: "ow".into(),
            str_keyed: false,
            init: vec![(1, 0), (2, 1)],
            srcs: vec![],
            ops: vec![
                Op::Set(1, 2),
                Op::SetHeap(1, 6),
                Op::Set(1, 3),
                Op::SetDefaultStmt(1, 4),
                Op::PopBind(1),
                Op::Set(1, 5),
            ],
            gets: vec![1, 2],
        },
        // ss0: the str-KEYED lane — pops / setdefault / update / del
        // interleaved; str keys exercise $__wasm_str_eq key probes over
        // scrambled storage.
        Seq {
            tag: "ss0".into(),
            str_keyed: true,
            init: vec![(0, 0), (1, 1), (2, 2)],
            srcs: vec![SrcDict {
                pairs: vec![(2, 6), (4, 5)],
            }],
            ops: vec![
                Op::PopBind(1),
                Op::SetDefaultStmt(3, 3),
                Op::SetDefault(0, 4),
                Op::Update(0),
                Op::PopDefault(9, 7),
                Op::Del(0),
            ],
            gets: vec![2, 3, 4],
        },
        // ss1: str-keyed + heap-built values — the eqm/vc content pins where
        // BOTH the key probe and the value compare must be content-based.
        Seq {
            tag: "ss1".into(),
            str_keyed: true,
            init: vec![(0, 1)],
            srcs: vec![],
            ops: vec![
                Op::SetHeap(1, 6),
                Op::SetHeap(0, 3),
                Op::Set(2, 0),
                Op::PopStmt(2),
            ],
            gets: vec![0, 1],
        },
    ];

    // --- LCG random walks over the FULL alphabet ------------------------------
    let mut rng = Lcg(0x1308_5CA1_FEED_CAFE); // fixed seed → byte-stable corpus
    for i in 0..11 {
        let str_keyed = i >= 8; // ru0..ru7 int-keyed, rs0..rs2 str-keyed
        let pool: Vec<i64> = if str_keyed {
            (0..=9).collect()
        } else {
            (1..=40).collect()
        };
        let n = 2 + rng.below(3); // 2..=4 initial keys
        let keys = rng.sample(&pool, n);
        let init: Vec<(i64, usize)> = keys
            .iter()
            .map(|&k| (k, rng.below(CONTENTS.len())))
            .collect();
        let mut live: BTreeMap<i64, usize> = init.iter().copied().collect();
        let mut srcs: Vec<SrcDict> = Vec::new();
        let mut ops: Vec<Op> = Vec::new();
        let mut cleared = false;
        // per-walk fresh-key counter (str-keyed keys stay short: 'k10'..)
        let mut fresh = if str_keyed {
            10 + 30 * i as i64
        } else {
            100 + 100 * i as i64
        };
        let n_ops = 4 + rng.below(5); // 4..=8 ops
        for _ in 0..n_ops {
            let pick_key = |rng: &mut Lcg, live: &BTreeMap<i64, usize>, fresh: &mut i64| {
                if !live.is_empty() && rng.below(2) == 0 {
                    let ks: Vec<i64> = live.keys().copied().collect();
                    ks[rng.below(ks.len())]
                } else {
                    *fresh += 1;
                    *fresh
                }
            };
            match rng.below(13) {
                0 | 1 => {
                    let k = pick_key(&mut rng, &live, &mut fresh);
                    let c = rng.below(CONTENTS.len());
                    live.insert(k, c);
                    ops.push(Op::Set(k, c));
                }
                2 => {
                    let k = pick_key(&mut rng, &live, &mut fresh);
                    let c = HEAP_CIDS[rng.below(HEAP_CIDS.len())];
                    live.insert(k, c);
                    ops.push(Op::SetHeap(k, c));
                }
                3 => {
                    if live.len() >= 2 {
                        let ks: Vec<i64> = live.keys().copied().collect();
                        let k = ks[rng.below(ks.len())];
                        live.remove(&k);
                        ops.push(Op::Del(k));
                    }
                }
                4 | 5 => {
                    if live.len() >= 2 {
                        let ks: Vec<i64> = live.keys().copied().collect();
                        let k = ks[rng.below(ks.len())];
                        live.remove(&k);
                        ops.push(if rng.below(2) == 0 {
                            Op::PopBind(k)
                        } else {
                            Op::PopStmt(k)
                        });
                    }
                }
                6 => {
                    // pop-with-default: 50/50 a live key (hit) / a fresh one
                    // (miss — must NOT mutate)
                    let k = pick_key(&mut rng, &live, &mut fresh);
                    live.remove(&k);
                    ops.push(Op::PopDefault(k, rng.below(CONTENTS.len())));
                }
                7 | 8 => {
                    let k = pick_key(&mut rng, &live, &mut fresh);
                    let c = rng.below(CONTENTS.len());
                    live.entry(k).or_insert(c);
                    ops.push(Op::SetDefault(k, c));
                }
                9 => {
                    let k = pick_key(&mut rng, &live, &mut fresh);
                    let c = rng.below(CONTENTS.len());
                    live.entry(k).or_insert(c);
                    ops.push(Op::SetDefaultStmt(k, c));
                }
                10 | 11 => {
                    let n_pairs = 1 + rng.below(3);
                    let mut pairs: Vec<(i64, usize)> = Vec::new();
                    for _ in 0..n_pairs {
                        let k = pick_key(&mut rng, &live, &mut fresh);
                        if pairs.iter().any(|(pk, _)| *pk == k) {
                            continue; // keys within one literal stay distinct
                        }
                        pairs.push((k, rng.below(CONTENTS.len())));
                    }
                    if pairs.is_empty() {
                        continue;
                    }
                    for (k, c) in &pairs {
                        live.insert(*k, *c);
                    }
                    let idx = srcs.len();
                    srcs.push(SrcDict { pairs });
                    ops.push(if rng.below(2) == 0 {
                        Op::Update(idx)
                    } else {
                        Op::PipeEq(idx)
                    });
                }
                _ => {
                    if !cleared && !live.is_empty() {
                        live.clear();
                        cleared = true;
                        ops.push(Op::Clear);
                    }
                }
            }
        }
        let gets = if live.is_empty() {
            vec![]
        } else {
            let ks: Vec<i64> = live.keys().copied().collect();
            vec![ks[rng.below(ks.len())]]
        };
        let tag = if str_keyed {
            format!("rs{}", i - 8)
        } else {
            format!("ru{i}")
        };
        seqs.push(Seq {
            tag,
            str_keyed,
            init,
            srcs,
            ops,
            gets,
        });
    }
    seqs
}

// ---- hand-written interaction extras (shapes the Seq machine can't spell) ----

/// (name, def) probes: the ACCEPTED mutation-inside-store `d[k] = d.pop(j)`
/// (finding #3 — CPython evaluates the RHS first), pop-vs-pop content compares
/// with a heap-built side, merge CONSTRUCTION from a scrambled receiver,
/// self-update, source/receiver independence with str values, and the
/// empty-after-clear eq.
fn extra_defs() -> Vec<(String, String)> {
    let defs: &[(&str, &str)] = &[
        // d[fresh] = d.pop(live): pop first (5 entries -> 4), then insert.
        (
            "x_setpop_fresh",
            "    d: dict[int, str] = {1: \"abc\", 2: \"w\"}\n    d[3] = d.pop(1)\n    return len(d[3]) * 10 + len(d)",
        ),
        // d[k] = d.pop(k): delete-then-reinsert of the SAME key.
        (
            "x_setpop_same",
            "    d: dict[int, str] = {1: \"abc\", 2: \"w\"}\n    d[1] = d.pop(1)\n    return len(d[1]) * 10 + len(d)",
        ),
        // the moved value is HEAP-built; content must survive the pop+store.
        (
            "x_setpop_content",
            "    h: str = \"v\"\n    d: dict[int, str] = {1: \"x\"}\n    d[1] = h + \"v\"\n    d[2] = d.pop(1)\n    if d[2] == \"vv\":\n        return 1\n    return 0",
        ),
        // pop == pop across dicts: equal bytes, DISTINCT allocations (one side
        // heap-built at store time) — address compare answers 0.
        (
            "x_popeq_content",
            "    q: str = \"a\"\n    d: dict[int, str] = {1: \"ab\"}\n    e: dict[int, str] = {2: \"x\"}\n    e[2] = q + \"b\"\n    if d.pop(1) == e.pop(2):\n        return 1\n    return 0",
        ),
        // pop < pop ordering: the lexicographically-SMALLER content sits at
        // the LARGER (heap) address — an address compare INVERTS the answer.
        (
            "x_poplt_addr",
            "    q: str = \"a\"\n    d: dict[int, str] = {1: \"b\"}\n    e: dict[int, str] = {2: \"x\"}\n    e[2] = q + \"a\"\n    if d.pop(1) < e.pop(2):\n        return 1\n    return 0",
        ),
        // bare pop + bare setdefault discard their results but must mutate.
        (
            "x_bare_discard",
            "    d: dict[int, str] = {1: \"abc\", 2: \"w\"}\n    d.pop(1)\n    d.setdefault(3, \"vv\")\n    d.setdefault(2, \"zzzzzz\")\n    return len(d) * 10 + len(d[3])",
        ),
        // merge CONSTRUCTION from a pop-scrambled receiver (PMAT-1304 surface
        // composed with the PMAT-1306 mutations).
        (
            "x_merge_scramble",
            "    d: dict[int, str] = {1: \"u\", 2: \"vv\", 3: \"www\"}\n    o: dict[int, str] = {2: \"xxxx\", 4: \"yyyyy\"}\n    d.pop(1)\n    del d[3]\n    c: dict[int, str] = {**d, **o}\n    return len(c) * 100 + len(c[2]) * 10 + len(d)",
        ),
        (
            "x_pipe_scramble",
            "    d: dict[int, str] = {1: \"u\", 2: \"vv\", 3: \"www\"}\n    o: dict[int, str] = {2: \"xxxx\", 4: \"yyyyy\"}\n    d.pop(3)\n    c: dict[int, str] = d | o\n    return len(c) * 100 + len(c[2]) * 10 + len(c[1])",
        ),
        // self-update over a delete-scrambled str-valued receiver is a no-op.
        (
            "x_selfupdate",
            "    d: dict[int, str] = {1: \"u\", 2: \"vv\", 3: \"www\"}\n    del d[2]\n    d.update(d)\n    return len(d) * 10 + len(d[3])",
        ),
        // merge copies entries: mutating the source after update must not
        // show through the receiver's CONTENT...
        (
            "x_srcmut_content",
            "    d: dict[int, str] = {1: \"u\"}\n    o: dict[int, str] = {2: \"vv\"}\n    d.update(o)\n    o[2] = \"zzzzzz\"\n    return len(d[2])",
        ),
        // ...and vice versa.
        (
            "x_recvmut_src",
            "    d: dict[int, str] = {1: \"u\"}\n    o: dict[int, str] = {2: \"vv\"}\n    d.update(o)\n    d[2] = \"zzzzzz\"\n    return len(o[2])",
        ),
        // {} == {} over ANNOTATED str-valued dicts (the same-kind empty case
        // IS in-lane; only MIXED kinds refuse).
        (
            "x_eq_clear_empty",
            "    d: dict[int, str] = {1: \"u\"}\n    d.clear()\n    m: dict[int, str] = {}\n    if d == m:\n        return 1\n    return 0",
        ),
        // heap-built vs literal value slots, equal content → eq must say 1.
        (
            "x_eq_heap_vs_lit",
            "    h: str = \"v\"\n    a: dict[int, str] = {1: \"x\"}\n    a[1] = h + \"v\"\n    b: dict[int, str] = {1: \"vv\"}\n    if a == b:\n        return 1\n    return 0",
        ),
        // same content reached through DIFFERENT mutation histories → 1.
        (
            "x_eq_order_scramble",
            "    a: dict[int, str] = {1: \"u\", 2: \"vv\", 3: \"www\"}\n    del a[1]\n    a[1] = \"u\"\n    b: dict[int, str] = {3: \"www\", 1: \"u\", 2: \"ba\"}\n    b[2] = \"vv\"\n    if a == b:\n        return 1\n    return 0",
        ),
    ];
    defs.iter()
        .map(|(name, body)| {
            (
                (*name).to_string(),
                format!("def {name}() -> int:\n{body}\n"),
            )
        })
        .collect()
}

fn extras_source() -> String {
    let mut src = String::new();
    for (_, def) in extra_defs() {
        src.push_str(&def);
        src.push('\n');
    }
    src
}

// ---- CPython oracle -----------------------------------------------------------

/// `{observable → expected}` from `python3` running the IDENTICAL defs.
fn python_oracle(seqs: &[Seq]) -> Option<BTreeMap<String, i64>> {
    let mut prog = String::from("v={}\n");
    for seq in seqs {
        for (name, def) in seq.observables() {
            prog.push_str(&def);
            prog.push_str(&format!("v['{name}']={name}()\n"));
        }
    }
    for (name, def) in extra_defs() {
        prog.push_str(&def);
        prog.push_str(&format!("v['{name}']={name}()\n"));
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
            "PMAT-1308: python3 oracle failed:\n{}",
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
        std::env::temp_dir().join(format!("xpile-wasm-svfuzz-{}-{}", std::process::id(), tag));
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
        assert_eq!(x.init, y.init, "{}: init unstable", x.tag);
        assert_eq!(x.gets, y.gets, "{}: gets unstable", x.tag);
        assert_eq!(
            x.wasm_source(),
            y.wasm_source(),
            "{}: source unstable",
            x.tag
        );
    }
}

/// Findings #1/#2 pinned: the curated sequences must EXERCISE the relocation
/// paths (setdefault-miss grow; double-grow update with str values), and the
/// corpus must keep covering the FULL op alphabet, both key lanes, and the
/// content-critical heap-built stores.
#[test]
fn corpus_forces_real_relocations_and_covers_the_alphabet() {
    let seqs = corpus();
    let by_tag = |t: &str| seqs.iter().find(|s| s.tag == t).expect("curated tag");

    assert_eq!(
        relocations(by_tag("sg")),
        1,
        "sg must relocate exactly once THROUGH THE SETDEFAULT-MISS PATH \
         (20 misses past cap 17); if DICT_GROWTH_SLACK changed, update the \
         mirror and re-derive the corpus sizes"
    );
    assert_eq!(
        relocations(by_tag("gm")),
        2,
        "gm must relocate TWICE (17→34→68) with str values"
    );
    assert_eq!(relocations(by_tag("gp")), 1, "gp: `|=` must relocate too");

    // Full alphabet coverage across the corpus.
    let covered = |pred: &dyn Fn(&Op) -> bool| seqs.iter().any(|s| s.ops.iter().any(pred));
    assert!(covered(&|o| matches!(o, Op::Set(..))), "alphabet lost Set");
    assert!(
        covered(&|o| matches!(o, Op::SetHeap(..))),
        "alphabet lost SetHeap (the content-critical heap-built store)"
    );
    assert!(covered(&|o| matches!(o, Op::Del(_))), "alphabet lost Del");
    assert!(
        covered(&|o| matches!(o, Op::PopBind(_))),
        "alphabet lost the bound pop"
    );
    assert!(
        covered(&|o| matches!(o, Op::PopStmt(_))),
        "alphabet lost the bare-statement pop"
    );
    assert!(
        covered(&|o| matches!(o, Op::PopDefault(..))),
        "alphabet lost pop-with-default"
    );
    assert!(
        covered(&|o| matches!(o, Op::SetDefault(..))),
        "alphabet lost the bound setdefault"
    );
    assert!(
        covered(&|o| matches!(o, Op::SetDefaultStmt(..))),
        "alphabet lost the bare setdefault"
    );
    assert!(
        covered(&|o| matches!(o, Op::Update(_))),
        "alphabet lost update"
    );
    assert!(
        covered(&|o| matches!(o, Op::PipeEq(_))),
        "alphabet lost the PEP 584 spelling"
    );
    assert!(covered(&|o| matches!(o, Op::Clear)), "alphabet lost clear");
    assert!(
        seqs.iter().any(|s| s.str_keyed),
        "corpus lost the str-keyed lane"
    );
    // At least one random walk interleaves the pop family with a merge.
    assert!(
        seqs.iter().any(|s| {
            (s.tag.starts_with("ru") || s.tag.starts_with("rs"))
                && s.ops
                    .iter()
                    .any(|o| matches!(o, Op::PopBind(_) | Op::PopStmt(_) | Op::PopDefault(..)))
                && s.ops
                    .iter()
                    .any(|o| matches!(o, Op::Update(_) | Op::PipeEq(_)))
        }),
        "no random walk interleaves a pop with a merge"
    );
    // Every sequence realizes trap-free (asserts inside) and carries the
    // eq-after-scramble observable.
    for seq in &seqs {
        let _ = seq.realize();
        assert!(
            seq.observables().iter().any(|(n, _)| n.ends_with("_eqm")),
            "{}: lost the eq-after-scramble observable",
            seq.tag
        );
    }
}

// ---- EMIT-path pins (run without WABT) --------------------------------------------

#[test]
fn fuzz_corpus_lowers_and_carries_the_sv_helpers() {
    for seq in &corpus() {
        let wat = emit(&seq.wasm_source())
            .unwrap_or_else(|e| panic!("sequence {} must lower: {e}", seq.tag));
        let suffix = if seq.str_keyed { "s" } else { "i" };
        assert!(
            wat.contains(&format!("call $__wasm_dict_eq_sv_{suffix}")),
            "{}: the eq observable must route through the CONTENT-comparing \
             $__wasm_dict_eq_sv_{suffix} twin (not the int-lane pointer eq)",
            seq.tag
        );
        assert!(
            wat.contains(&format!("call $__wasm_dict_set_{suffix}")),
            "{}: stores must route through $__wasm_dict_set_{suffix}",
            seq.tag
        );
        if seq.has_pop() {
            assert!(
                wat.contains(&format!("call $__wasm_dict_pop_{suffix}")),
                "{}: pops must route through $__wasm_dict_pop_{suffix}",
                seq.tag
            );
        }
    }
    emit(&extras_source()).expect("the extras module must lower");
}

/// The honest-refusal belt of the str-value lane, pinned through the FULL
/// pipeline: the eager-vs-lazy default class (PMAT-1306), the concat/f-string
/// multi-eval class, mixed VALUE kinds, and str-value iteration. If any of
/// these starts lowering, it must arrive WITH an executed witness — not by a
/// belt silently loosening.
#[test]
fn sideeffect_and_multi_eval_shapes_refuse() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "pop inside a get default (eager-vs-lazy)",
            "def f() -> int:\n    d: dict[int, str] = {1: \"u\"}\n    e: dict[int, str] = {2: \"vv\"}\n    s: str = d.get(1, e.pop(2))\n    return len(s)\n",
            "eagerly",
        ),
        (
            "pop inside a pop default (eager-vs-lazy)",
            "def f() -> int:\n    d: dict[int, str] = {1: \"u\"}\n    e: dict[int, str] = {2: \"vv\"}\n    s: str = d.pop(1, e.pop(2))\n    return len(s)\n",
            "eagerly",
        ),
        (
            "pop inside a setdefault default (eager-vs-lazy)",
            "def f() -> int:\n    d: dict[int, str] = {1: \"u\"}\n    e: dict[int, str] = {2: \"vv\"}\n    s: str = d.setdefault(1, e.pop(2))\n    return len(s)\n",
            "eagerly",
        ),
        (
            "self-referential pop default",
            "def f() -> int:\n    d: dict[int, str] = {1: \"u\", 2: \"vv\"}\n    s: str = d.pop(1, d.pop(2))\n    return len(s)\n",
            "eagerly",
        ),
        (
            "pop inside a concat (multi-eval)",
            "def f() -> int:\n    d: dict[int, str] = {1: \"u\"}\n    s: str = d.pop(1) + \"Z\"\n    return len(s)\n",
            "re-evaluates",
        ),
        (
            "pop inside an f-string (multi-eval)",
            "def f() -> int:\n    d: dict[int, str] = {1: \"u\"}\n    s: str = f\"{d.pop(1)}x\"\n    return len(s)\n",
            "re-evaluates",
        ),
        (
            "mixed value-kind eq",
            "def f() -> int:\n    d: dict[int, str] = {1: \"u\"}\n    e: dict[int, int] = {1: 5}\n    if d == e:\n        return 1\n    return 0\n",
            "VALUE kinds",
        ),
        (
            "mixed value-kind update",
            "def f() -> int:\n    d: dict[int, str] = {1: \"u\"}\n    e: dict[int, int] = {2: 5}\n    d.update(e)\n    return len(d)\n",
            "VALUE kinds differ",
        ),
        (
            "str-value .values() iteration",
            "def f() -> int:\n    d: dict[int, str] = {1: \"u\"}\n    t: int = 0\n    for v in d.values():\n        t = t + 1\n    return t\n",
            "str-value iteration",
        ),
        (
            "str-value .items() iteration",
            "def f() -> int:\n    d: dict[int, str] = {1: \"u\"}\n    t: int = 0\n    for k, v in d.items():\n        t = t + k\n    return t\n",
            "str-value iteration",
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
fn dict_strv_fuzz_matches_cpython() {
    let seqs = corpus();

    // EMIT path holds regardless of WABT.
    let mut modules: Vec<(String, String)> = seqs
        .iter()
        .map(|seq| (seq.tag.clone(), emit(&seq.wasm_source()).expect("lowers")))
        .collect();
    modules.push(("extras".into(), emit(&extras_source()).expect("lowers")));

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1308: skipping EXECUTED str-value dict fuzz — WABT (wat2wasm / \
             wasm-interp) absent. Every sequence lowered through emit_module and \
             carries the $__wasm_dict_eq_sv_<k> + pop/set helpers; a box with \
             WABT + python3 runs every observable (REAL setdefault-grow and \
             double-relocation merges included) and value-matches live CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1308: skipping fuzz value-diff — python3 (the oracle) absent.");
        return;
    }
    let oracle = match python_oracle(&seqs) {
        Some(o) => o,
        None => {
            eprintln!("PMAT-1308: python3 oracle unavailable — skipping value diff.");
            return;
        }
    };

    let mut names: Vec<(String, Vec<String>)> = seqs
        .iter()
        .map(|seq| {
            (
                seq.tag.clone(),
                seq.observables().into_iter().map(|(n, _)| n).collect(),
            )
        })
        .collect();
    names.push((
        "extras".into(),
        extra_defs().into_iter().map(|(n, _)| n).collect(),
    ));

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
                 (a str-value miscompile — an address compare, a wrong slot \
                 after swap/relocation, a lazy default, or an eq-gate bug)\n\
                 interp output:\n{stdout}"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 150,
        "fuzz breadth regressed: only {checked} observables checked"
    );
    eprintln!(
        "PMAT-1308: EXECUTED str-value dict fuzz PASSED — {checked} observables \
         across {} sequences (+extras) == live CPython, REAL setdefault-grow \
         relocation and heap-built content compares included.",
        seqs.len()
    );
}
