//! PMAT-1296 — an ADVERSARIAL-VERIFY differential witness for the recent native-WASM
//! dict/set REDUCTION family (PMAT-1290..1295) against LIVE CPython (`python3`),
//! fuzzed over randomized value distributions AND observed through the reductions
//! themselves — over containers first MUTATED by a random `del` / `discard`
//! sequence.
//!
//! ## The gap this closes
//!
//! Two prior fuzzes bracket this one but neither reaches it:
//!
//!   * `dict_set_family_fuzz_witness` (PMAT-1238/1240) fuzzes the MUTATION ops
//!     (`d[k]=v`, `setdefault`, `pop`, `del`, `clear`; `s.add`, `remove`,
//!     `discard`, `clear`) and observes them through `d.get(k,-1)` / `len` /
//!     `k in s`. It NEVER observes a container through a REDUCTION.
//!   * The per-op reduction witnesses (`set_reduce_witness` PMAT-1293,
//!     `dict_key_reduce_witness` PMAT-1294, `dict_value_reduce_witness` PMAT-1295)
//!     pin `sum`/`min`/`max`/`sorted` — but each over ONE hand-picked literal with
//!     at most a single `discard`, and the sorted witnesses check a couple of fixed
//!     indices.
//!
//! So the interaction the reductions are MOST at risk in is unwitnessed: a
//! reduction observing a container AFTER a `del`/`discard` sequence
//! (swap-last-into-hole scrambles storage order), over RANDOMIZED value
//! distributions, and — for the order-DEFINING `sorted` — element-by-element order
//! over that scrambled storage. A hash-order divergence (the PMAT-1292 class — an
//! order-DEPENDENT observation reading the bump-heap's arbitrary storage order
//! instead of CPython's) or a swap-into-hole materialiser bug would surface here and
//! nowhere else.
//!
//! ## What this fuzzes
//!
//! A DETERMINISTIC corpus (fixed-seed LCG, no `rand` — `cargo deny` unaffected) of
//! `dict[int, int]` and `set[int]` sequences: an initial distinct-key literal + a
//! random trap-free `del` / `discard` prefix (always leaving ≥ 2 live elements, so
//! `min`/`max` never trap and `sorted` has ≥ 2 indices). Every sequence is observed
//! through EVERY reduction the WASM subset supports:
//!
//!   dict:  `sum(d)`  `min(d)`  `max(d)`  `sum(d.values())`  `min(d.values())`
//!          `max(d.values())`  and `sorted(d)[i]` / `sorted(d.values())[i]` for
//!          every live index `i` (bound to a named `list[int]` — the supported form).
//!   set:   `sum(s)`  `min(s)`  `max(s)`  and `sorted(s)[i]` for every live index.
//!
//! Each observable is a standalone `-> int` function; a whole sequence is ONE module
//! (its own fresh single-page bump heap, like the PMAT-1238 fuzz). The identical
//! Python — annotations and all (valid plain `python3`) — is the sole oracle, so
//! there is ZERO reimplementation risk. Value ranges are bounded so no reduction
//! sum overflows `i64` (the documented int-is-i64 model — not a bug — is kept out of
//! the differential).
//!
//! ## Refusal guards (the PMAT-1292 defense)
//!
//! The reductions are CPython-exact ONLY because they are order-BLIND (`sum`/`min`/
//! `max`) or order-DEFINING (`sorted`). An ORDER-DEPENDENT observation of a
//! bump-heap container — `for k in d` / `for k in d.keys()` / `for v in
//! d.values()` with a positional body (`r = r*10 + k`), `list(d)`,
//! `list(d.values())`, `list(s)` — would read the arbitrary storage order and
//! silently diverge from CPython's insertion / hash order. Those MUST refuse at
//! compile time. (PMAT-1297 later opened `for k in d` for order-INDEPENDENT
//! commutative bodies, which stay CPython-exact; the cases pinned here all use an
//! order-DEPENDENT `r = r*10 + k` body or a bare `list(...)`, so they still
//! refuse.) This witness pins those refusals so a future slice can never quietly
//! accept an order-DEPENDENT one without an order-safe model (the exact class
//! PMAT-1292 caught mid-stream). These run on the EMIT path alone — no WABT
//! required.
//!
//! ## Gating
//!
//! The executed diff runs only when BOTH WABT (`wat2wasm` / `wasm-interp`) AND
//! `python3` are present; without WABT it skips cleanly after still exercising the
//! EMIT path for every sequence. CITES `C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`.

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
    /// A signed int in `[lo, hi]`.
    fn between(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as usize) as i64
    }
    /// `count` DISTINCT values drawn from `pool`.
    fn sample(&mut self, pool: &[i64], count: usize) -> Vec<i64> {
        let mut avail: Vec<i64> = pool.to_vec();
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let i = self.below(avail.len());
            out.push(avail.remove(i));
        }
        out
    }
}

// ---- one observable ---------------------------------------------------------

/// A single `-> int` reduction observation: its unique export name, the bare Python
/// statements that (re)build + mutate the container (and optionally bind a sorted
/// list), and the `int` return expression. The SAME `build`/`ret` feed both the
/// WASM module and the CPython oracle, so they are identical by construction.
struct Obs {
    name: String,
    build: Vec<String>,
    ret: String,
}

// ---- sequence → observables -------------------------------------------------

/// A container sequence: an initial distinct-key literal + a trap-free delete
/// prefix, always leaving ≥ 2 live elements.
struct Seq {
    tag: String,
    is_dict: bool,
    keys: Vec<i64>,
    vals: Vec<i64>, // parallel to `keys`; ignored for a set
    dels: Vec<i64>, // a subset of `keys`, deleted in order
}

impl Seq {
    /// Live element count after the deletes (deletes target distinct present keys).
    fn live(&self) -> usize {
        self.keys.len() - self.dels.len()
    }

    /// The container-build + delete lines shared by every observable.
    fn common(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.is_dict {
            let entries: Vec<String> = self
                .keys
                .iter()
                .zip(&self.vals)
                .map(|(k, val)| format!("{k}: {val}"))
                .collect();
            v.push(format!("d: dict[int, int] = {{{}}}", entries.join(", ")));
            for k in &self.dels {
                v.push(format!("del d[{k}]"));
            }
        } else {
            let elems: Vec<String> = self.keys.iter().map(|k| k.to_string()).collect();
            v.push(format!("s: set[int] = {{{}}}", elems.join(", ")));
            for k in &self.dels {
                // `discard` never traps; deletes here are on present keys anyway.
                v.push(format!("s.discard({k})"));
            }
        }
        v
    }

    /// Every reduction observation over this sequence.
    fn observables(&self) -> Vec<Obs> {
        let mut out = Vec::new();
        let common = self.common();
        let l = self.live();
        let mut push = |suffix: &str, mut build: Vec<String>, ret: String| {
            let name = format!("{}_{suffix}", self.tag);
            let mut lines = common.clone();
            lines.append(&mut build);
            out.push(Obs {
                name,
                build: lines,
                ret,
            });
        };
        if self.is_dict {
            push("sum", vec![], "sum(d)".into());
            push("min", vec![], "min(d)".into());
            push("max", vec![], "max(d)".into());
            push("vsum", vec![], "sum(d.values())".into());
            push("vmin", vec![], "min(d.values())".into());
            push("vmax", vec![], "max(d.values())".into());
            for i in 0..l {
                push(
                    &format!("k{i}"),
                    vec!["xs: list[int] = sorted(d)".into()],
                    format!("xs[{i}]"),
                );
            }
            for i in 0..l {
                push(
                    &format!("v{i}"),
                    vec!["ys: list[int] = sorted(d.values())".into()],
                    format!("ys[{i}]"),
                );
            }
        } else {
            push("sum", vec![], "sum(s)".into());
            push("min", vec![], "min(s)".into());
            push("max", vec![], "max(s)".into());
            for i in 0..l {
                push(
                    &format!("e{i}"),
                    vec!["xs: list[int] = sorted(s)".into()],
                    format!("xs[{i}]"),
                );
            }
        }
        out
    }

    /// The Python module: one `def NAME() -> int:` per observable, `emit`-ready.
    fn wasm_source(&self) -> String {
        let mut src = String::new();
        for obs in self.observables() {
            src.push_str(&format!("def {}() -> int:\n", obs.name));
            for line in &obs.build {
                src.push_str(&format!("    {line}\n"));
            }
            src.push_str(&format!("    return {}\n\n", obs.ret));
        }
        src
    }
}

// ---- corpus -----------------------------------------------------------------

/// Curated interaction edges + a fixed-seed LCG walk over `dict[int,int]` and
/// `set[int]` sequences.
fn corpus() -> Vec<Seq> {
    // --- curated edges (the paths a random walk may under-sample) ---
    let mut seqs: Vec<Seq> = vec![
        // dict: delete a MIDDLE key (swap-last-into-hole) then reduce over survivors.
        Seq {
            tag: "cd_delmid".into(),
            is_dict: true,
            keys: vec![10, 3, 27, 8, 19],
            vals: vec![1, 2, 3, 4, 5],
            dels: vec![27],
        },
        // dict: delete down to a SINGLE live entry (min==max==the-only-key).
        Seq {
            tag: "cd_leaveone".into(),
            is_dict: true,
            keys: vec![5, 1, 9],
            vals: vec![7, 2, 4],
            dels: vec![5, 9],
        },
        // dict: a near-`i64::MIN` key/value — the PMAT-1289 deep-negative-literal
        // class through the `del` key path. Live sum stays in `i64` range.
        Seq {
            tag: "cd_deepneg".into(),
            is_dict: true,
            keys: vec![-9223372036854775807, 5, 1],
            vals: vec![4, 9, 2],
            dels: vec![5],
        },
        // set: delete a MIDDLE element then reduce over survivors.
        Seq {
            tag: "cs_delmid".into(),
            is_dict: false,
            keys: vec![50, 3, 27, 8],
            vals: vec![],
            dels: vec![27],
        },
        // set: delete down to a single live element.
        Seq {
            tag: "cs_leaveone".into(),
            is_dict: false,
            keys: vec![9, 2, 40, 7],
            vals: vec![],
            dels: vec![9, 40, 7],
        },
        // set: near-`i64::MIN` element through the `discard` path.
        Seq {
            tag: "cs_deepneg".into(),
            is_dict: false,
            keys: vec![-9223372036854775807, 100, 2],
            vals: vec![],
            dels: vec![100],
        },
    ];

    // --- random walk: bounded ranges keep every reduction sum inside i64 ---
    let key_pool: Vec<i64> = (-20..=40).collect();
    let mut rng = Lcg(0x1296_C0FF_EED1_C7A9); // fixed seed → byte-stable corpus
    for i in 0..8 {
        let n = 3 + rng.below(4); // 3..=6 distinct keys
        let keys = rng.sample(&key_pool, n);
        let vals: Vec<i64> = (0..n).map(|_| rng.between(-30, 60)).collect();
        let n_del = rng.below(n - 1); // 0..=n-2 → live ≥ 2
        let dels = rng.sample(&keys, n_del);
        seqs.push(Seq {
            tag: format!("rd{i}"),
            is_dict: true,
            keys,
            vals,
            dels,
        });
    }
    for i in 0..8 {
        let n = 3 + rng.below(4);
        let keys = rng.sample(&key_pool, n);
        let n_del = rng.below(n - 1);
        let dels = rng.sample(&keys, n_del);
        seqs.push(Seq {
            tag: format!("rs{i}"),
            is_dict: false,
            keys,
            vals: vec![],
            dels,
        });
    }
    seqs
}

// ---- CPython oracle ---------------------------------------------------------

/// `{observable_name → expected int}` from `python3` running the IDENTICAL
/// per-observable build + return. The annotations are valid plain Python, so the
/// oracle and the WASM module share one source of truth.
fn python_oracle(seqs: &[Seq]) -> Option<BTreeMap<String, i64>> {
    let mut prog = String::from("v={}\n");
    for seq in seqs {
        for obs in seq.observables() {
            prog.push_str(&format!("def {}():\n", obs.name));
            for line in &obs.build {
                prog.push_str(&format!("\t{line}\n"));
            }
            prog.push_str(&format!("\treturn {}\n", obs.ret));
            prog.push_str(&format!("v['{}']={}()\n", obs.name, obs.name));
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
            "PMAT-1296: python3 oracle failed:\n{}",
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

fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-redfuzz-{}-{}", std::process::id(), tag));
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

/// Parse a `name() => i64:<value>` line. `wasm-interp` prints i64 UNSIGNED, so a
/// negative renders as its two's-complement `u64` — parse as `u64`, reinterpret.
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

// ---- tests ------------------------------------------------------------------

#[test]
fn reduction_fuzz_lowers() {
    // The EMIT path must lower for every sequence regardless of WABT (holds on
    // free CI) — a smoke over the whole reduction corpus.
    for seq in &corpus() {
        emit(&seq.wasm_source())
            .unwrap_or_else(|e| panic!("reduction sequence {} must lower: {e}", seq.tag));
    }
}

#[test]
fn corpus_is_deterministic_and_exercises_deletes_and_sorted() {
    let a = corpus();
    let b = corpus();
    assert_eq!(a.len(), b.len(), "corpus size unstable");
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.tag, y.tag, "tag order unstable");
        assert_eq!(x.keys, y.keys, "{}: keys unstable", x.tag);
        assert_eq!(x.dels, y.dels, "{}: dels unstable", x.tag);
    }
    // Every sequence leaves ≥ 2 live elements (min/max never trap; ≥ 2 sorted
    // indices).
    for seq in &a {
        assert!(
            seq.live() >= 2 || seq.tag.contains("leaveone"),
            "{}: fewer than 2 live elements without being a leave-one edge",
            seq.tag
        );
        assert!(
            seq.live() >= 1,
            "{}: no live elements (would trap)",
            seq.tag
        );
    }
    // The corpus genuinely deletes AND observes through sorted: at least one dict
    // and one set sequence carry a delete, and every sequence emits ≥ 1 sorted
    // observable.
    assert!(
        a.iter().any(|s| s.is_dict && !s.dels.is_empty()),
        "no dict sequence exercises a delete before reduction"
    );
    assert!(
        a.iter().any(|s| !s.is_dict && !s.dels.is_empty()),
        "no set sequence exercises a discard before reduction"
    );
    for seq in &a {
        let obs = seq.observables();
        assert!(
            obs.iter()
                .any(|o| o.ret.starts_with("xs[") || o.ret.starts_with("ys[")),
            "{}: no sorted-index observable",
            seq.tag
        );
    }
}

#[test]
fn reductions_match_cpython_over_random_sequences() {
    let seqs = corpus();

    // EMIT path holds regardless of WABT (also asserted in `reduction_fuzz_lowers`).
    for seq in &seqs {
        emit(&seq.wasm_source()).expect("reduction sequence lowers");
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1296: skipping EXECUTED reduction fuzz — WABT (wat2wasm / wasm-interp) \
             absent. Every sequence lowered through emit_module; a box with WABT + python3 \
             runs every observable and value-matches live CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1296: skipping reduction fuzz value-diff — python3 (the oracle) absent.");
        return;
    }

    let oracle = match python_oracle(&seqs) {
        Some(o) => o,
        None => {
            eprintln!("PMAT-1296: python3 oracle unavailable — skipping value diff.");
            return;
        }
    };

    let mut mismatches: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for seq in &seqs {
        let wat = emit(&seq.wasm_source()).expect("sequence lowers");
        let (stdout, ok) = assemble_and_run(&seq.tag, &wat);
        assert!(ok, "wasm-interp run failed for {}:\n{stdout}", seq.tag);
        assert!(
            !stdout.contains("unreachable executed"),
            "{} trapped — a reduction hit an empty container:\n{stdout}",
            seq.tag
        );
        for obs in seq.observables() {
            let expected = *oracle
                .get(&obs.name)
                .unwrap_or_else(|| panic!("CPython oracle missing observable {}", obs.name));
            let got = parse_scalar(&stdout, &obs.name);
            if got == expected {
                checked += 1;
            } else {
                mismatches.push(format!(
                    "{}: WASM={got} CPython={expected}  [{} → {}]",
                    obs.name,
                    obs.build.join("; "),
                    obs.ret
                ));
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "PMAT-1296: {} WASM/CPython divergence(s) over the reduction corpus:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert_eq!(
        checked,
        oracle.len(),
        "every CPython observable must be matched by a WASM export"
    );

    eprintln!(
        "PMAT-1296: reduction fuzz PASSED — {checked} reduction observables across {} \
         dict/set sequences executed in WABT and matched live python3. No divergence in \
         sum/min/max over keys or values, and sorted(d)/sorted(d.values())/sorted(s) \
         element order held CPython-exact after random del/discard swap-into-hole \
         scrambling.",
        seqs.len()
    );
}

/// The PMAT-1292 defense: an order-DEPENDENT observation of a bump-heap container
/// reads its arbitrary storage order and would silently diverge from CPython's
/// insertion / hash order. Every such form MUST refuse at compile time. These pin
/// the refusals so a future slice cannot quietly accept one without an order-safe
/// model. EMIT path only — no WABT required.
///
/// PMAT-1365 supplied exactly that order-safe model for `list(d)` / `list(d.keys())`
/// / `list(d.values())`, so those three moved OFF this roster — but only under a
/// module-wide gate, and the gated shapes are re-pinned below. A dict's storage
/// order IS Python's insertion order (`$__wasm_dict_set_<k>` is
/// update-in-place-else-append-at-count) UNTIL a removal permutes it
/// (swap-last-into-hole) or a `set` seeds it (xpile-insertion vs CPython-hash
/// iteration), which is why the cases here now carry a hazard. A dict view in a
/// hazard-FREE module is CPython-exact and executed against live python3 by
/// `dict_view_list_witness.rs`. `list(s)` over a set stays refused unconditionally
/// — a set has no hazard-free module, its order is unfaithful by construction.
#[test]
fn order_dependent_iteration_refuses() {
    let cases: &[(&str, &str)] = &[
        (
            "for-k-in-dict",
            "def go() -> int:\n    d: dict[int, int] = {50: 1, 3: 1, 27: 1}\n    r: int = 0\n    for k in d:\n        r = r * 10 + k\n    return r\n",
        ),
        (
            "for-v-in-values",
            "def go() -> int:\n    d: dict[int, int] = {50: 7, 3: 2, 27: 9}\n    r: int = 0\n    for v in d.values():\n        r = r * 10 + v\n    return r\n",
        ),
        (
            "for-k-in-keys",
            "def go() -> int:\n    d: dict[int, int] = {50: 1, 3: 1, 27: 1}\n    r: int = 0\n    for k in d.keys():\n        r = r * 10 + k\n    return r\n",
        ),
        (
            // PMAT-1365: the bare form now EMITS; what still refuses is the form
            // whose module perturbs the insertion order — here a `del`, which
            // swaps the LAST entry into the hole and would walk [27, 3] where
            // CPython walks [3, 27].
            "list-of-dict-after-del",
            "def go() -> int:\n    d: dict[int, int] = {50: 1, 3: 1, 27: 1}\n    del d[50]\n    xs: list[int] = list(d)\n    return xs[0]\n",
        ),
        (
            // Same gate through the VALUE materialiser, and via `d.pop(k)` — a
            // removal that hides in EXPRESSION position rather than statement
            // position.
            "list-of-values-after-pop",
            "def go() -> int:\n    d: dict[int, int] = {50: 1, 3: 1, 27: 1}\n    p: int = d.pop(50)\n    xs: list[int] = list(d.values())\n    return xs[0] + p\n",
        ),
        (
            // A `set` ANYWHERE in the module poisons the dict view too: a dict
            // filled while iterating a set (`for x in s: d[x] = …`, accepted
            // since PMAT-1314) inherits xpile's insertion order where CPython
            // used hash order.
            "list-of-dict-with-a-set-in-scope",
            "def go() -> int:\n    s: set[int] = {9, 4}\n    d: dict[int, int] = {50: 1, 3: 1, 27: 1}\n    xs: list[int] = list(d)\n    return xs[0] + len(s)\n",
        ),
        (
            "list-of-set",
            "def go() -> int:\n    s: set[int] = {50, 3, 27}\n    xs: list[int] = list(s)\n    return xs[0]\n",
        ),
        (
            // Indexing a NON-name `sorted(...)` temporary is also unsupported —
            // the supported form binds it to a named list first.
            "index-sorted-temporary",
            "def go() -> int:\n    d: dict[int, int] = {50: 1, 3: 1, 27: 1}\n    return sorted(d)[0]\n",
        ),
    ];
    for (name, src) in cases {
        let err = emit(src).err().unwrap_or_else(|| {
            panic!(
                "PMAT-1296: `{name}` is an order-DEPENDENT (or unsupported-temporary) \
                 observation and MUST refuse — a silent accept is the PMAT-1292 miscompile \
                 class"
            )
        });
        assert!(
            err.contains("unsupported construct") || err.contains("WASM"),
            "`{name}` refusal should name the unsupported construct, got: {err}"
        );
    }
}
