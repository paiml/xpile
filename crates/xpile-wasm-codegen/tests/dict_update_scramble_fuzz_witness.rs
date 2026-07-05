//! PMAT-1303 — an ADVERSARIAL-VERIFY differential witness for the native-WASM
//! dict MUTATION surface opened by `d.update(other)` (`Stmt::DictUpdate`,
//! PMAT-1302), fuzzed as an OP ALPHABET — `d[k] = v` / `del d[k]` /
//! `d.update(o)` / `d |= o` (PEP 584) interleaved by a fixed-seed LCG — and
//! value-matched against LIVE CPython (`python3`) on the IDENTICAL source.
//!
//! ## The gaps this closes (what the skeptic pass found)
//!
//! 1. **The shipped "grow" pins never grew.** A dict literal's capacity is
//!    `count + DICT_GROWTH_SLACK` (= 16 spare slots), so the PMAT-1302
//!    witness's relocation probes — merging FOUR entries into a size-1 dict
//!    (capacity 17) — never reach `count >= capacity`: the 2x-grow +
//!    relocation + write-back path of `$__wasm_dict_update_<k>` was shipped
//!    UNWITNESSED (the `grow_*` pins exercised plain appends). This file
//!    merges 20-40 FRESH keys through one `update` call, forcing a real
//!    relocation (and a DOUBLE relocation, 17→34→68, in `gm`) and then reads
//!    pre-grow keys, post-grow keys, and whole-dict reductions through the
//!    written-back pointer. `corpus_forces_real_relocations` pins the
//!    capacity arithmetic so the corpus keeps outrunning the slack if it
//!    ever changes — and documents that the OLD witness shape (1 literal
//!    entry + 4 merged) relocates ZERO times.
//! 2. **The shipped witness never left meta-HIR.** Every PMAT-1302 probe
//!    builds `Stmt::DictUpdate` by hand; none proves REAL Python
//!    `d.update(o)` survives the frontend. This fuzz drives the FULL
//!    pipeline (Python source → `PythonFrontend` → `emit_module` →
//!    `wat2wasm` → `wasm-interp`), which also covers the frontend-only
//!    spelling `d |= o` (PEP 584 lowers to the SAME `Stmt::DictUpdate` —
//!    reachable on WASM since PMAT-1302 but pinned nowhere).
//! 3. **`update` was never interleaved with the older mutations.** `del`
//!    swap-into-hole + `update`'s update-or-insert + overwrite-after-merge
//!    compose into storage orders no per-op witness sees; every sequence
//!    here is observed through the (order-independent) reduction +
//!    read-back surface: `len` / `sum(d)` / `sum(d.values())` / `min` /
//!    `max` / `d[k]` / a BOUND `sorted(d)` element.
//!
//! ## What was probed and did NOT refute
//!
//! * **The stale-alias hazard is closed upstream.** `update` relocates the
//!   receiver and writes back ONLY the receiver's local — so an accepted
//!   dict copy (`e = d`) would go stale across a relocating merge and read
//!   garbage. It cannot arise: the frontend's alias analysis refuses
//!   `e = d` + mutation ("aliases `d` and `e`"), and the codegen's
//!   `emit_heap_map_bind` independently refuses a dict-name binding value.
//!   Pinned in `dict_copy_binding_refuses` so neither belt loosens silently.
//! * **The helper snapshots `n = count(o)` BEFORE the walk**, so the
//!   self-merge `d.update(d)` (which can never grow — every key is already
//!   present) stays a no-op even over a delete-scrambled receiver
//!   (`x_selfdel_*`).
//! * **Merging copies entries; it does not share storage.** Mutating the
//!   source AFTER `d.update(o)` must not show through `d` (and vice versa) —
//!   `x_srcmut_*` would catch a shared-region implementation.
//!
//! Each observable is a standalone `def NAME() -> int` (valid plain
//! `python3` AND wasm-frontend-lowerable); one sequence = ONE module (its own
//! fresh single-page bump heap); the IDENTICAL text feeds both lanes, so the
//! oracle has ZERO reimplementation risk. Deterministic fixed-seed LCG — no
//! `rand`, no time, byte-stable corpus. Values are bounded and only
//! sum/min/max/len/get/sorted observables are used (no product), so no
//! observable overflows `i64`.
//!
//! ## Gating
//!
//! The executed diff needs WABT (`wat2wasm` / `wasm-interp`) AND `python3`;
//! without either it skips cleanly after asserting the EMIT path for every
//! sequence. Refusal pins run on the emit path alone. CITES
//! `C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP` (test-only; no new contract).

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
    fn between(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as usize) as i64
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

// ---- the mutation-op alphabet ------------------------------------------------

/// One receiver mutation. `Update`/`PipeEq` name a source dict by index into
/// `Seq::srcs` — both lower to `Stmt::DictUpdate` (PEP 584 `|=` and `.update()`
/// are the same meta-HIR op; fuzzing both pins the frontend spelling too).
#[derive(Clone)]
enum Op {
    Set(i64, i64),
    Del(i64),
    Update(usize),
    PipeEq(usize),
}

/// A literal source dict bound before the ops (`o<idx>`), never itself mutated.
#[derive(Clone)]
struct SrcDict {
    pairs: Vec<(i64, i64)>,
}

/// One fuzz sequence: a receiver literal + bound sources + an interleaved op
/// list, observed through every applicable reduction / read-back.
struct Seq {
    tag: String,
    str_keyed: bool,
    init: Vec<(i64, i64)>,
    srcs: Vec<SrcDict>,
    ops: Vec<Op>,
    /// Keys read back via `d[k]` — must be live at sequence end (checked by
    /// `model()`); curated sequences pick pre-grow AND post-grow keys.
    gets: Vec<i64>,
}

impl Seq {
    /// Key spelling: an int key literal, or the bijective `'k<n>'` for the
    /// str-keyed lane (values stay ints in both).
    fn key_txt(&self, k: i64) -> String {
        if self.str_keyed {
            format!("'k{k}'")
        } else {
            k.to_string()
        }
    }

    fn dict_ann(&self) -> &'static str {
        if self.str_keyed {
            "dict[str, int]"
        } else {
            "dict[int, int]"
        }
    }

    fn lit(&self, pairs: &[(i64, i64)]) -> String {
        let entries: Vec<String> = pairs
            .iter()
            .map(|(k, v)| format!("{}: {v}", self.key_txt(*k)))
            .collect();
        format!("{{{}}}", entries.join(", "))
    }

    /// The shared build-and-mutate prefix of every observable.
    fn common(&self) -> Vec<String> {
        let mut v = Vec::new();
        v.push(format!("d: {} = {}", self.dict_ann(), self.lit(&self.init)));
        for (i, s) in self.srcs.iter().enumerate() {
            v.push(format!(
                "o{i}: {} = {}",
                self.dict_ann(),
                self.lit(&s.pairs)
            ));
        }
        for op in &self.ops {
            match op {
                Op::Set(k, val) => v.push(format!("d[{}] = {val}", self.key_txt(*k))),
                Op::Del(k) => v.push(format!("del d[{}]", self.key_txt(*k))),
                Op::Update(i) => v.push(format!("d.update(o{i})")),
                Op::PipeEq(i) => v.push(format!("d |= o{i}")),
            }
        }
        v
    }

    /// Replay the ops over a `BTreeMap` — the model that keeps the corpus
    /// trap-free (dels/gets hit live keys, min/max/sorted need non-empty).
    fn model(&self) -> BTreeMap<i64, i64> {
        let mut m: BTreeMap<i64, i64> = self.init.iter().copied().collect();
        for op in &self.ops {
            match op {
                Op::Set(k, v) => {
                    m.insert(*k, *v);
                }
                Op::Del(k) => {
                    assert!(
                        m.remove(k).is_some(),
                        "{}: del of a dead key {k} would trap",
                        self.tag
                    );
                }
                Op::Update(i) | Op::PipeEq(i) => {
                    for (k, v) in &self.srcs[*i].pairs {
                        m.insert(*k, *v);
                    }
                }
            }
        }
        for k in &self.gets {
            assert!(m.contains_key(k), "{}: get of a dead key {k}", self.tag);
        }
        m
    }

    /// Every observation over the mutated receiver (each a standalone def).
    /// str keys can't be summed/ordered, so the str lane keeps len / value-sum
    /// / get; the int lane adds key-sum, min, max, and a BOUND `sorted(d)[0]`.
    fn observables(&self) -> Vec<(String, String)> {
        let live = self.model();
        let common = self.common();
        let mut tails: Vec<(&str, String)> = vec![
            ("len", "return len(d)".into()),
            ("sumv", "return sum(d.values())".into()),
        ];
        if !self.str_keyed {
            tails.push(("sumk", "return sum(d)".into()));
            if !live.is_empty() {
                tails.push(("mn", "return min(d)".into()));
                tails.push(("mx", "return max(d)".into()));
                tails.push(("srt0", "xs = sorted(d)\n    return xs[0]".into()));
            }
        }
        if !self.srcs.is_empty() {
            // The LAST-used source must be unmutated by the merge.
            let last = self.srcs.len() - 1;
            tails.push(("srcv", format!("return sum(o{last}.values())")));
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
        for (gi, k) in self.gets.iter().enumerate() {
            let name = format!("{}_get{gi}", self.tag);
            let mut src = format!("def {name}() -> int:\n");
            for line in &common {
                src.push_str(&format!("    {line}\n"));
            }
            src.push_str(&format!("    return d[{}]\n", self.key_txt(*k)));
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
}

// ---- capacity model (pins finding #1) ----------------------------------------

/// The dict growth slack (`DICT_GROWTH_SLACK` in the codegen): a literal's
/// capacity is `count + 16`. Mirrored here (the constant is private) so the
/// corpus can PROVE its merges outrun the slack; if the slack ever changes,
/// `corpus_forces_real_relocations` fails loudly instead of the grow pins
/// silently degrading back into plain appends (the PMAT-1302 witness's bug).
const MIRRORED_GROWTH_SLACK: usize = 16;

/// Replay a sequence against the capacity model, counting RELOCATIONS (the
/// `count >= capacity` doublings inside `$__wasm_dict_set_<k>`).
fn relocations(seq: &Seq) -> usize {
    let mut live: BTreeMap<i64, i64> = seq.init.iter().copied().collect();
    let mut count = live.len();
    let mut cap = live.len() + MIRRORED_GROWTH_SLACK;
    let mut relocs = 0;
    let mut insert = |live: &mut BTreeMap<i64, i64>, count: &mut usize, k: i64, v: i64| {
        if live.insert(k, v).is_none() {
            if *count >= cap {
                cap *= 2;
                relocs += 1;
            }
            *count += 1;
        }
    };
    for op in &seq.ops {
        match op {
            Op::Set(k, v) => insert(&mut live, &mut count, *k, *v),
            Op::Del(k) => {
                live.remove(k);
                count -= 1;
            }
            Op::Update(i) | Op::PipeEq(i) => {
                for (k, v) in &seq.srcs[*i].pairs {
                    insert(&mut live, &mut count, *k, *v);
                }
            }
        }
    }
    relocs
}

// ---- corpus -------------------------------------------------------------------

/// `n` fresh `(key, value)` pairs `100+start..` with small bounded values.
fn fresh_pairs(start: i64, n: i64) -> Vec<(i64, i64)> {
    (0..n).map(|i| (start + i, (start + i) % 97 - 40)).collect()
}

fn corpus() -> Vec<Seq> {
    let mut seqs: Vec<Seq> = vec![
        // g1: ONE real relocation — 20 fresh keys through one update into a
        // size-1 receiver (cap 17); read a pre-grow key, a post-grow key, and
        // every reduction through the written-back pointer.
        Seq {
            tag: "g1".into(),
            str_keyed: false,
            init: vec![(1, 1)],
            srcs: vec![SrcDict {
                pairs: fresh_pairs(100, 20),
            }],
            ops: vec![Op::Update(0)],
            gets: vec![1, 100, 119],
        },
        // gm: DOUBLE relocation in ONE update call (cap 17 → 34 → 68).
        Seq {
            tag: "gm".into(),
            str_keyed: false,
            init: vec![(1, 1)],
            srcs: vec![SrcDict {
                pairs: fresh_pairs(100, 40),
            }],
            ops: vec![Op::Update(0)],
            gets: vec![1, 116, 139],
        },
        // ge: EMPTY receiver (cap 16, the PMAT-1160 annotated-empty literal)
        // grown through update — the shipped witness only merged INTO
        // non-empty and FROM empty.
        Seq {
            tag: "ge".into(),
            str_keyed: false,
            init: vec![],
            srcs: vec![SrcDict {
                pairs: fresh_pairs(100, 20),
            }],
            ops: vec![Op::Update(0)],
            gets: vec![100, 119],
        },
        // gp: the PEP 584 spelling forces the SAME relocation.
        Seq {
            tag: "gp".into(),
            str_keyed: false,
            init: vec![(1, 1)],
            srcs: vec![SrcDict {
                pairs: fresh_pairs(100, 20),
            }],
            ops: vec![Op::PipeEq(0)],
            gets: vec![1, 119],
        },
        // gd: mutate the RELOCATED region — grow, then del (swap-into-hole in
        // the new region), then insert; reductions walk the scramble.
        Seq {
            tag: "gd".into(),
            str_keyed: false,
            init: vec![(1, 1)],
            srcs: vec![SrcDict {
                pairs: fresh_pairs(100, 20),
            }],
            ops: vec![Op::Update(0), Op::Del(105), Op::Del(1), Op::Set(300, 7)],
            gets: vec![104, 300],
        },
        // gc: CHAINED updates — o1 overwrites keys o0 merged plus the
        // receiver's own, then appends fresh.
        Seq {
            tag: "gc".into(),
            str_keyed: false,
            init: vec![(1, 1), (2, 2)],
            srcs: vec![
                SrcDict {
                    pairs: fresh_pairs(100, 10),
                },
                SrcDict {
                    pairs: vec![(2, 99), (100, 55), (400, 8)],
                },
            ],
            ops: vec![Op::Update(0), Op::Update(1)],
            gets: vec![2, 100, 400],
        },
        // gr: update ONTO a fully-emptied receiver (dels leave count 0 with
        // stale entry bytes; the merge re-populates from slot 0).
        Seq {
            tag: "gr".into(),
            str_keyed: false,
            init: vec![(1, 1), (2, 2)],
            srcs: vec![SrcDict {
                pairs: vec![(5, 50), (6, 60)],
            }],
            ops: vec![Op::Del(1), Op::Del(2), Op::Update(0)],
            gets: vec![5, 6],
        },
        // ow: overwrite a MERGED key after the update (a set on the
        // update-written entry, not a fresh append).
        Seq {
            tag: "ow".into(),
            str_keyed: false,
            init: vec![(1, 1)],
            srcs: vec![SrcDict {
                pairs: vec![(2, 20), (3, 30)],
            }],
            ops: vec![Op::Update(0), Op::Set(2, 5)],
            gets: vec![1, 2, 3],
        },
        // st0: str-keyed content-compare merge (shared key overwritten by
        // CONTENT, not pointer) + read-backs.
        Seq {
            tag: "st0".into(),
            str_keyed: true,
            init: vec![(0, 1), (1, 2)],
            srcs: vec![SrcDict {
                pairs: vec![(1, 9), (2, 3)],
            }],
            ops: vec![Op::Update(0)],
            gets: vec![0, 1, 2],
        },
        // st1: str-keyed PEP 584 + del + re-insert around the merge.
        Seq {
            tag: "st1".into(),
            str_keyed: true,
            init: vec![(0, 1)],
            srcs: vec![SrcDict {
                pairs: vec![(1, 2), (2, 3)],
            }],
            ops: vec![Op::PipeEq(0), Op::Del(0), Op::Set(3, 4)],
            gets: vec![1, 3],
        },
    ];

    // --- LCG random walks: update interleaved with the older mutation ops ---
    let key_pool: Vec<i64> = (-20..=40).collect();
    let mut rng = Lcg(0x1303_D1C7_FEED_CAFE); // fixed seed → byte-stable corpus
    for i in 0..8 {
        let n = 2 + rng.below(3); // 2..=4 initial keys
        let keys = rng.sample(&key_pool, n);
        let init: Vec<(i64, i64)> = keys.iter().map(|&k| (k, rng.between(-50, 50))).collect();
        let mut live: BTreeMap<i64, i64> = init.iter().copied().collect();
        let mut srcs: Vec<SrcDict> = Vec::new();
        let mut ops: Vec<Op> = Vec::new();
        let mut fresh = 100 + 100 * i as i64; // per-seq fresh-key counter
        let n_ops = 3 + rng.below(4); // 3..=6 ops
        for _ in 0..n_ops {
            match rng.below(4) {
                0 => {
                    // set: 50/50 overwrite a live key / insert a fresh one
                    let k = if !live.is_empty() && rng.below(2) == 0 {
                        let ks: Vec<i64> = live.keys().copied().collect();
                        ks[rng.below(ks.len())]
                    } else {
                        fresh += 1;
                        fresh
                    };
                    let v = rng.between(-50, 50);
                    live.insert(k, v);
                    ops.push(Op::Set(k, v));
                }
                1 => {
                    // del a live key, keeping at least one survivor
                    if live.len() >= 2 {
                        let ks: Vec<i64> = live.keys().copied().collect();
                        let k = ks[rng.below(ks.len())];
                        live.remove(&k);
                        ops.push(Op::Del(k));
                    }
                }
                which => {
                    // update / |= with a fresh 1..=3-pair source: each pair
                    // 50/50 overlaps a live key (new value) / is fresh
                    let n_pairs = 1 + rng.below(3);
                    let mut pairs: Vec<(i64, i64)> = Vec::new();
                    for _ in 0..n_pairs {
                        let k = if !live.is_empty() && rng.below(2) == 0 {
                            let ks: Vec<i64> = live.keys().copied().collect();
                            ks[rng.below(ks.len())]
                        } else {
                            fresh += 1;
                            fresh
                        };
                        if pairs.iter().any(|(pk, _)| *pk == k) {
                            continue; // keys within one literal stay distinct
                        }
                        pairs.push((k, rng.between(-50, 50)));
                    }
                    if pairs.is_empty() {
                        continue;
                    }
                    for (k, v) in &pairs {
                        live.insert(*k, *v);
                    }
                    let idx = srcs.len();
                    srcs.push(SrcDict { pairs });
                    ops.push(if which == 2 {
                        Op::Update(idx)
                    } else {
                        Op::PipeEq(idx)
                    });
                }
            }
        }
        // one read-back on a random live key (live is never empty: dels keep
        // a survivor and sets/updates only add)
        let ks: Vec<i64> = live.keys().copied().collect();
        let gets = vec![ks[rng.below(ks.len())]];
        seqs.push(Seq {
            tag: format!("ru{i}"),
            str_keyed: false,
            init,
            srcs,
            ops,
            gets,
        });
    }
    seqs
}

// ---- hand-written interaction extras (shapes the Seq machine can't spell) ----

/// (name, def) probes for two-receiver and sharing edges: the MUTUAL merge
/// `a.update(b); b.update(a)`, the delete-scrambled SELF-merge, and the
/// source/receiver INDEPENDENCE checks (a shared-storage implementation of
/// `update` would fail `x_srcmut_val` / `x_recvmut_src`).
fn extra_defs() -> Vec<(String, String)> {
    let defs: &[(&str, &str)] = &[
        (
            "x_mutual_suma",
            "    a: dict[int, int] = {1: 10, 2: 20}\n    b: dict[int, int] = {2: 99, 3: 30}\n    a.update(b)\n    b.update(a)\n    return sum(a.values())",
        ),
        (
            "x_mutual_sumb",
            "    a: dict[int, int] = {1: 10, 2: 20}\n    b: dict[int, int] = {2: 99, 3: 30}\n    a.update(b)\n    b.update(a)\n    return sum(b.values())",
        ),
        (
            "x_mutual_lens",
            "    a: dict[int, int] = {1: 10, 2: 20}\n    b: dict[int, int] = {2: 99, 3: 30}\n    a.update(b)\n    b.update(a)\n    return len(a) * 10 + len(b)",
        ),
        (
            "x_selfdel_sum",
            "    d: dict[int, int] = {1: 10, 2: 20, 3: 30}\n    del d[2]\n    d.update(d)\n    return sum(d.values()) + len(d)",
        ),
        (
            "x_selfdel_get",
            "    d: dict[int, int] = {1: 10, 2: 20, 3: 30}\n    del d[2]\n    d.update(d)\n    return d[3]",
        ),
        (
            "x_srcmut_len",
            "    d: dict[int, int] = {1: 10}\n    o: dict[int, int] = {2: 20}\n    d.update(o)\n    o[99] = 5\n    return len(d) * 10 + len(o)",
        ),
        (
            "x_srcmut_val",
            "    d: dict[int, int] = {1: 10}\n    o: dict[int, int] = {2: 20}\n    d.update(o)\n    o[2] = 77\n    return d[2]",
        ),
        (
            "x_recvmut_src",
            "    d: dict[int, int] = {1: 10}\n    o: dict[int, int] = {2: 20}\n    d.update(o)\n    d[2] = 55\n    return o[2]",
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
            "PMAT-1303: python3 oracle failed:\n{}",
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
        std::env::temp_dir().join(format!("xpile-wasm-updfuzz-{}-{}", std::process::id(), tag));
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

/// FINDING #1 pinned: the corpus must EXERCISE the relocation path the
/// PMAT-1302 witness only claimed to. The old witness shape (a 1-entry
/// literal + 4 merged keys) relocates ZERO times — its capacity is
/// `1 + 16 = 17` and the count only reaches 5.
#[test]
fn corpus_forces_real_relocations() {
    let seqs = corpus();
    let by_tag = |t: &str| seqs.iter().find(|s| s.tag == t).expect("curated tag");

    // The shipped PMAT-1302 "grow" pins, replayed against the capacity model:
    // NO relocation ever happened. (This is the hollow-grow finding, kept
    // executable so the slack mirror stays honest.)
    let old_witness_shape = Seq {
        tag: "pmat1302_grow_pins".into(),
        str_keyed: false,
        init: vec![(1, 1)],
        srcs: vec![SrcDict {
            pairs: vec![(2, 2), (3, 3), (4, 4), (5, 5)],
        }],
        ops: vec![Op::Update(0)],
        gets: vec![],
    };
    assert_eq!(
        relocations(&old_witness_shape),
        0,
        "the PMAT-1302 witness shape now relocates — DICT_GROWTH_SLACK \
         changed; update this file's mirror and re-derive the corpus sizes"
    );

    assert_eq!(
        relocations(by_tag("g1")),
        1,
        "g1 must relocate exactly once"
    );
    assert_eq!(
        relocations(by_tag("gm")),
        2,
        "gm must relocate TWICE (17→34→68)"
    );
    assert_eq!(
        relocations(by_tag("ge")),
        1,
        "ge must grow the empty receiver"
    );
    assert_eq!(relocations(by_tag("gp")), 1, "gp: `|=` must relocate too");
    assert!(
        relocations(by_tag("gd")) >= 1,
        "gd must mutate a relocated region"
    );

    // Both DictUpdate spellings and the str lane are present.
    assert!(
        seqs.iter()
            .any(|s| s.ops.iter().any(|o| matches!(o, Op::Update(_)))),
        "corpus lost the .update() spelling"
    );
    assert!(
        seqs.iter()
            .any(|s| s.ops.iter().any(|o| matches!(o, Op::PipeEq(_)))),
        "corpus lost the PEP 584 `|=` spelling"
    );
    assert!(
        seqs.iter().any(|s| s.str_keyed),
        "corpus lost the str-keyed lane"
    );
    // At least one random walk actually interleaves update with set/del.
    assert!(
        seqs.iter().any(|s| {
            s.tag.starts_with("ru")
                && s.ops
                    .iter()
                    .any(|o| matches!(o, Op::Update(_) | Op::PipeEq(_)))
                && s.ops.iter().any(|o| matches!(o, Op::Set(..) | Op::Del(_)))
        }),
        "no random walk interleaves a merge with set/del"
    );
    // Every sequence's model is trap-free by construction (asserts inside).
    for seq in &seqs {
        let _ = seq.model();
    }
}

// ---- EMIT-path pins (run without WABT) --------------------------------------------

#[test]
fn fuzz_corpus_lowers_and_reaches_the_update_helper() {
    for seq in &corpus() {
        let wat = emit(&seq.wasm_source())
            .unwrap_or_else(|e| panic!("sequence {} must lower: {e}", seq.tag));
        let suffix = if seq.str_keyed { "s" } else { "i" };
        assert!(
            wat.contains(&format!("call $__wasm_dict_update_{suffix}")),
            "{}: the merge must route through $__wasm_dict_update_{suffix} \
             (not some other path):\n{}",
            seq.tag,
            &wat[..wat.len().min(2048)]
        );
    }
    emit(&extras_source()).expect("the extras module must lower");
}

/// The stale-alias defense: `update` writes the (possibly relocated) receiver
/// back to ONE local, so an accepted dict copy would silently read freed
/// memory after a growing merge. The frontend refuses the alias+mutate shape;
/// pin it so the hazard cannot open silently.
#[test]
fn dict_copy_binding_refuses() {
    let err = emit(
        "def f() -> int:\n    d: dict[int, int] = {1: 10}\n    e = d\n    o: dict[int, int] = {2: 20}\n    d.update(o)\n    return e[2]\n",
    )
    .expect_err("a dict copy observed across a mutating merge must refuse");
    assert!(
        err.contains("aliases"),
        "refusal should come from the alias analysis, got: {err}"
    );
}

/// Full-pipeline refusal pins (the shipped witness pinned these at meta-HIR
/// level only; the frontend must not re-shape them into something accepted).
#[test]
fn out_of_lane_update_shapes_refuse_through_the_full_pipeline() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "literal argument",
            "def f() -> int:\n    d: dict[int, int] = {1: 10}\n    d.update({9: 9})\n    return len(d)\n",
            "must be a `dict` NAME",
        ),
        (
            "kwargs form",
            "def f() -> int:\n    d: dict[str, int] = {'a': 1}\n    d.update(x=1)\n    return len(d)\n",
            "must be a `dict` NAME",
        ),
        (
            "set argument",
            "def f() -> int:\n    d: dict[int, int] = {1: 10}\n    s: set[int] = {2, 3}\n    d.update(s)\n    return len(d)\n",
            "not a mapping",
        ),
        (
            "key-kind mismatch",
            "def f() -> int:\n    d: dict[int, int] = {1: 10}\n    o: dict[str, int] = {'x': 2}\n    d.update(o)\n    return len(d)\n",
            "key kinds differ",
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
fn dict_mutation_fuzz_matches_cpython() {
    let seqs = corpus();

    // EMIT path holds regardless of WABT.
    let mut modules: Vec<(String, String)> = seqs
        .iter()
        .map(|seq| (seq.tag.clone(), emit(&seq.wasm_source()).expect("lowers")))
        .collect();
    modules.push(("extras".into(), emit(&extras_source()).expect("lowers")));

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1303: skipping EXECUTED dict-mutation fuzz — WABT (wat2wasm / \
             wasm-interp) absent. Every sequence lowered through emit_module and \
             routes through $__wasm_dict_update_<k>; a box with WABT + python3 \
             runs every observable (REAL relocating grows included) and \
             value-matches live CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1303: skipping fuzz value-diff — python3 (the oracle) absent.");
        return;
    }
    let oracle = match python_oracle(&seqs) {
        Some(o) => o,
        None => {
            eprintln!("PMAT-1303: python3 oracle unavailable — skipping value diff.");
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
                 (a mutation-surface miscompile — a relocation write-back, \
                 merge-dedup, or storage-sharing bug)\ninterp output:\n{stdout}"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 130,
        "fuzz breadth regressed: only {checked} observables checked"
    );
    eprintln!(
        "PMAT-1303: EXECUTED dict-mutation fuzz PASSED — {checked} observables \
         across {} sequences (+extras) == live CPython, REAL single- and \
         double-relocation merges included.",
        seqs.len()
    );
}
