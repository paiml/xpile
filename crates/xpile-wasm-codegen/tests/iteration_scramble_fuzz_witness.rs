//! PMAT-1300 — an ADVERSARIAL-VERIFY differential witness for the native-WASM
//! hash-container ITERATION surface (`for x in s` PMAT-1290, `for v in
//! d.values()` PMAT-1298, `for k in d.keys()` PMAT-1299), fuzzed with
//! order-INDEPENDENT commutative bodies over containers first SCRAMBLED by a
//! random `del` / `discard` / `add` / insert sequence, value-matched against LIVE
//! CPython (`python3`).
//!
//! ## The gap this closes
//!
//! The reduction fuzz `dict_set_reduction_fuzz_witness` (PMAT-1296) fuzzes
//! `sum`/`min`/`max`/`sorted` over delete-scrambled containers, but — by its own
//! docstring — it "NEVER observes a container through ITERATION". The iteration
//! forms opened AFTER it (PMAT-1297/1298/1299) each ship a per-form witness
//! (`dict_key_iteration_witness`, `dict_value_iteration_witness`,
//! `dict_keys_view_iteration_witness`) — but each pins a FIXED small literal with
//! at most a single `discard`, and NONE scrambles the container with a delete/add
//! sequence before iterating.
//!
//! So the interaction the iteration forms are MOST at risk in is unwitnessed: an
//! order-INDEPENDENT loop body (`acc = acc + e`, `acc = acc * e`, `acc = acc ^ e`,
//! `acc = acc + 1`, the `if e > acc: acc = e` min/max idiom) observing a container
//! AFTER a `del`/`discard`/`add` sequence — a `del`/`discard` swaps the last entry
//! into the hole and an `add` re-fills it, so bump-heap STORAGE order genuinely
//! diverges from CPython's INSERTION order. The order-safety claim
//! (`set_iteration_body_order_safe`: only a commutative fold is accepted, so the
//! result is storage-order-INVARIANT and therefore CPython-exact) is exactly what
//! a delete-scramble stresses. The PMAT-1292 class — an order-DEPENDENT body
//! silently reading the arbitrary storage order — would surface here and nowhere
//! else.
//!
//! ## What this fuzzes
//!
//! A DETERMINISTIC corpus (fixed-seed LCG, no `rand` — `cargo deny` unaffected) of
//! `dict[int, int]` and `set[int]` sequences: an initial distinct-key literal + a
//! trap-free `del`/`discard` prefix (targets present keys) + a fresh-key
//! `add`/insert suffix (disjoint pool, so no re-add ambiguity). Every sequence is
//! observed through every APPLICABLE iteration form with every commutative body:
//!
//!   set  (`for x in s`):        sum / product / xor / count / max-idiom / min-idiom
//!   dict (`for v in d.values()`,
//!         `for k in d.keys()`):  the same six bodies over EACH view.
//!
//! (Bare `for k in d` over a scrambled dict deliberately refuses — a
//! function-level dict mutation routes it to the keys-snapshot + size-change-guard
//! form the WASM subset does not model; pinned in `bare_for_k_over_mutated_dict_refuses`.)
//!
//! Each observable is a standalone `-> int` function; a whole sequence is ONE
//! module (its own fresh single-page bump heap). Value/key ranges are bounded so no
//! product overflows `i64` (the documented int-is-i64 model — not a bug — is kept
//! out of the differential); the deep-negative-literal edge (the PMAT-1289 class)
//! is left as the SOLE survivor so its product is itself. The identical Python —
//! annotations and all (valid plain `python3`) — is the sole oracle, so there is
//! ZERO reimplementation risk.
//!
//! ## Refusal guards (the PMAT-1292 defense)
//!
//! An ORDER-DEPENDENT body (`r = r * 10 + e`, a positional fold) over any of these
//! forms reads the arbitrary storage order and would silently diverge from
//! CPython's insertion/hash order; it MUST refuse. A str-VALUED `.values()`
//! iteration is not in the int-only value lane and MUST refuse. A bare `for k in d`
//! over a mutated dict MUST refuse. These pins keep a future slice from quietly
//! accepting an order-DEPENDENT (or out-of-lane) iteration without an order-safe
//! model. They run on the EMIT path alone — no WABT required.
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
    /// A signed int in `[lo, hi]`.
    fn between(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as usize) as i64
    }
}

// ---- commutative loop bodies (all order-INDEPENDENT) ------------------------

/// The order-independent body shapes accepted by `set_iteration_body_order_safe`.
/// Each is CPython-exact regardless of iteration order, so it stays correct after
/// the container's storage order is scrambled by a delete/add.
#[derive(Clone, Copy)]
enum BodyKind {
    Sum,
    Product,
    Xor,
    Count,
    MaxIdiom,
    MinIdiom,
}

impl BodyKind {
    const ALL: [BodyKind; 6] = [
        BodyKind::Sum,
        BodyKind::Product,
        BodyKind::Xor,
        BodyKind::Count,
        BodyKind::MaxIdiom,
        BodyKind::MinIdiom,
    ];

    fn suffix(self) -> &'static str {
        match self {
            BodyKind::Sum => "sum",
            BodyKind::Product => "prod",
            BodyKind::Xor => "xor",
            BodyKind::Count => "cnt",
            BodyKind::MaxIdiom => "mx",
            BodyKind::MinIdiom => "mn",
        }
    }

    /// The accumulator initialiser. `-1000`/`1000` sit strictly outside the bounded
    /// value pool, so the min/max idiom finds the true extremum for non-deep-neg
    /// sequences and — because BOTH lanes run the identical idiom — matches CPython
    /// exactly even for the deep-negative survivor.
    fn init(self) -> &'static str {
        match self {
            BodyKind::Sum | BodyKind::Xor | BodyKind::Count => "acc: int = 0",
            BodyKind::Product => "acc: int = 1",
            BodyKind::MaxIdiom => "acc: int = -1000",
            BodyKind::MinIdiom => "acc: int = 1000",
        }
    }

    /// The loop-body lines (relative to the loop indent) for loop variable `var`.
    fn loop_lines(self, var: &str) -> Vec<String> {
        match self {
            BodyKind::Sum => vec![format!("acc = acc + {var}")],
            BodyKind::Product => vec![format!("acc = acc * {var}")],
            BodyKind::Xor => vec![format!("acc = acc ^ {var}")],
            BodyKind::Count => vec!["acc = acc + 1".to_string()],
            BodyKind::MaxIdiom => vec![format!("if {var} > acc:"), format!("    acc = {var}")],
            BodyKind::MinIdiom => vec![format!("if {var} < acc:"), format!("    acc = {var}")],
        }
    }
}

// ---- iteration forms --------------------------------------------------------

/// One iteration form: its name suffix, the iterable expression, and the loop
/// variable it binds.
struct IterForm {
    suffix: &'static str,
    iterable: &'static str,
    var: &'static str,
}

/// The set has one iteration form; a dict is observed through BOTH read-only views.
const SET_FORMS: &[IterForm] = &[IterForm {
    suffix: "set",
    iterable: "s",
    var: "x",
}];
const DICT_FORMS: &[IterForm] = &[
    IterForm {
        suffix: "val",
        iterable: "d.values()",
        var: "v",
    },
    IterForm {
        suffix: "key",
        iterable: "d.keys()",
        var: "k",
    },
];

// ---- one observable ---------------------------------------------------------

/// A single `-> int` iteration observation: its unique export name and the FULL
/// `def NAME() -> int:` source (valid plain `python3` AND lowerable by the WASM
/// frontend), so the WASM module and the CPython oracle share one text.
struct Obs {
    name: String,
    def_text: String,
}

// ---- sequence → observables -------------------------------------------------

/// A container sequence: an initial distinct-key literal + a trap-free delete
/// prefix (present keys) + a fresh-key add suffix (disjoint pool).
struct Seq {
    tag: String,
    is_dict: bool,
    keys: Vec<i64>,
    vals: Vec<i64>,        // parallel to `keys`; ignored for a set
    dels: Vec<i64>,        // a subset of `keys`, deleted in order
    adds: Vec<(i64, i64)>, // fresh (key, value); for a set only the key is used
}

impl Seq {
    /// The container-build + delete + add lines shared by every observable.
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
            for (k, val) in &self.adds {
                v.push(format!("d[{k}] = {val}"));
            }
        } else {
            let elems: Vec<String> = self.keys.iter().map(|k| k.to_string()).collect();
            v.push(format!("s: set[int] = {{{}}}", elems.join(", ")));
            for k in &self.dels {
                // `discard` never traps; deletes here are on present keys anyway.
                v.push(format!("s.discard({k})"));
            }
            for (k, _) in &self.adds {
                v.push(format!("s.add({k})"));
            }
        }
        v
    }

    /// The applicable iteration forms for this container kind.
    fn forms(&self) -> &'static [IterForm] {
        if self.is_dict {
            DICT_FORMS
        } else {
            SET_FORMS
        }
    }

    /// Every iteration observation over this sequence.
    fn observables(&self) -> Vec<Obs> {
        let mut out = Vec::new();
        let common = self.common();
        for form in self.forms() {
            for body in BodyKind::ALL {
                let name = format!("{}_{}_{}", self.tag, form.suffix, body.suffix());
                let mut src = format!("def {name}() -> int:\n");
                for line in &common {
                    src.push_str(&format!("    {line}\n"));
                }
                src.push_str(&format!("    {}\n", body.init()));
                src.push_str(&format!("    for {} in {}:\n", form.var, form.iterable));
                for line in body.loop_lines(form.var) {
                    src.push_str(&format!("        {line}\n"));
                }
                src.push_str("    return acc\n");
                out.push(Obs {
                    name,
                    def_text: src,
                });
            }
        }
        out
    }

    /// The Python module: one `def NAME() -> int:` per observable, `emit`-ready.
    fn wasm_source(&self) -> String {
        let mut src = String::new();
        for obs in self.observables() {
            src.push_str(&obs.def_text);
            src.push('\n');
        }
        src
    }
}

// ---- corpus -----------------------------------------------------------------

/// Curated interaction edges + a fixed-seed LCG walk over `dict[int,int]` and
/// `set[int]` sequences. Every value/key stays within a bounded pool so no product
/// over the (≤ ~8 live) container overflows `i64`; the deep-negative edges leave
/// the deep-neg element as the SOLE survivor so ITS product is itself.
fn corpus() -> Vec<Seq> {
    let mut seqs: Vec<Seq> = vec![
        // dict: delete a MIDDLE key (swap-last-into-hole) then iterate survivors.
        Seq {
            tag: "cd_delmid".into(),
            is_dict: true,
            keys: vec![10, 3, 27, 8, 19],
            vals: vec![1, 2, 3, 4, 5],
            dels: vec![27],
            adds: vec![],
        },
        // dict: delete a middle key THEN re-fill the hole with a fresh insert —
        // storage order diverges from insertion order both ways.
        Seq {
            tag: "cd_delfill".into(),
            is_dict: true,
            keys: vec![10, 3, 27, 8],
            vals: vec![2, 4, 6, 8],
            dels: vec![27],
            adds: vec![(55, 9)],
        },
        // dict: delete down to a SINGLE live entry.
        Seq {
            tag: "cd_leaveone".into(),
            is_dict: true,
            keys: vec![5, 1, 9],
            vals: vec![7, 2, 4],
            dels: vec![5, 9],
            adds: vec![],
        },
        // dict: delete EVERY entry — iteration runs zero times (accumulator survives).
        Seq {
            tag: "cd_empty".into(),
            is_dict: true,
            keys: vec![4, 2],
            vals: vec![3, 7],
            dels: vec![4, 2],
            adds: vec![],
        },
        // dict: a near-`i64::MIN` key AND value (the PMAT-1289 deep-neg-literal
        // class) left as the SOLE survivor, so product = itself (no overflow).
        Seq {
            tag: "cd_deepneg".into(),
            is_dict: true,
            keys: vec![-9223372036854775807, 5],
            vals: vec![-9223372036854775807, 9],
            dels: vec![5],
            adds: vec![],
        },
        // set: delete a MIDDLE element then re-fill with an add.
        Seq {
            tag: "cs_delfill".into(),
            is_dict: false,
            keys: vec![50, 3, 27, 8],
            vals: vec![],
            dels: vec![27],
            adds: vec![(60, 0)],
        },
        // set: delete down to a single live element.
        Seq {
            tag: "cs_leaveone".into(),
            is_dict: false,
            keys: vec![9, 2, 40, 7],
            vals: vec![],
            dels: vec![9, 40, 7],
            adds: vec![],
        },
        // set: delete every element — zero iterations.
        Seq {
            tag: "cs_empty".into(),
            is_dict: false,
            keys: vec![6, 1],
            vals: vec![],
            dels: vec![6, 1],
            adds: vec![],
        },
        // set: near-`i64::MIN` element as the SOLE survivor.
        Seq {
            tag: "cs_deepneg".into(),
            is_dict: false,
            keys: vec![-9223372036854775807, 100],
            vals: vec![],
            dels: vec![100],
            adds: vec![],
        },
    ];

    // --- random walk: bounded ranges keep every product inside i64 ---
    let key_pool: Vec<i64> = (-20..=40).collect();
    let add_pool: Vec<i64> = (50..=70).collect(); // disjoint from key_pool
    let mut rng = Lcg(0x1300_C0FF_EED0_17E5); // fixed seed → byte-stable corpus
    for i in 0..6 {
        let n = 3 + rng.below(3); // 3..=5 distinct keys
        let keys = rng.sample(&key_pool, n);
        let vals: Vec<i64> = (0..keys.len()).map(|_| rng.between(-20, 40)).collect();
        let n_del = rng.below(keys.len()); // 0..=n-1 → ≥ 1 initial survivor
        let dels = rng.sample(&keys, n_del);
        let n_add = rng.below(3); // 0..=2 fresh inserts
        let add_keys = rng.sample(&add_pool, n_add);
        let adds: Vec<(i64, i64)> = add_keys
            .into_iter()
            .map(|k| (k, rng.between(-20, 40)))
            .collect();
        seqs.push(Seq {
            tag: format!("rd{i}"),
            is_dict: true,
            keys,
            vals,
            dels,
            adds,
        });
    }
    for i in 0..6 {
        let n = 3 + rng.below(3);
        let keys = rng.sample(&key_pool, n);
        let n_del = rng.below(keys.len());
        let dels = rng.sample(&keys, n_del);
        let n_add = rng.below(3);
        let add_keys = rng.sample(&add_pool, n_add);
        let adds: Vec<(i64, i64)> = add_keys.into_iter().map(|k| (k, 0)).collect();
        seqs.push(Seq {
            tag: format!("rs{i}"),
            is_dict: false,
            keys,
            vals: vec![],
            dels,
            adds,
        });
    }
    seqs
}

// ---- CPython oracle ---------------------------------------------------------

/// `{observable_name → expected int}` from `python3` running the IDENTICAL
/// per-observable `def`. The annotations are valid plain Python, so the oracle and
/// the WASM module share one source of truth.
fn python_oracle(seqs: &[Seq]) -> Option<BTreeMap<String, i64>> {
    let mut prog = String::from("v={}\n");
    for seq in seqs {
        for obs in seq.observables() {
            prog.push_str(&obs.def_text);
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
            "PMAT-1300: python3 oracle failed:\n{}",
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
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-iterfuzz-{}-{}",
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
fn iteration_fuzz_lowers() {
    // The EMIT path must lower for every sequence regardless of WABT (holds on
    // free CI) — a smoke over the whole iteration corpus.
    for seq in &corpus() {
        emit(&seq.wasm_source())
            .unwrap_or_else(|e| panic!("iteration sequence {} must lower: {e}", seq.tag));
    }
}

#[test]
fn corpus_is_deterministic_and_scrambles_before_iterating() {
    let a = corpus();
    let b = corpus();
    assert_eq!(a.len(), b.len(), "corpus size unstable");
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.tag, y.tag, "tag order unstable");
        assert_eq!(x.keys, y.keys, "{}: keys unstable", x.tag);
        assert_eq!(x.dels, y.dels, "{}: dels unstable", x.tag);
        assert_eq!(x.adds, y.adds, "{}: adds unstable", x.tag);
    }
    // The corpus genuinely scrambles: at least one dict and one set carry a delete,
    // and at least one of each carries an add that re-fills a hole.
    assert!(
        a.iter().any(|s| s.is_dict && !s.dels.is_empty()),
        "no dict sequence deletes before iterating"
    );
    assert!(
        a.iter().any(|s| !s.is_dict && !s.dels.is_empty()),
        "no set sequence discards before iterating"
    );
    assert!(
        a.iter().any(|s| s.is_dict && !s.adds.is_empty()),
        "no dict sequence re-fills a hole with an add"
    );
    assert!(
        a.iter().any(|s| !s.is_dict && !s.adds.is_empty()),
        "no set sequence re-fills a hole with an add"
    );
    // Every dict sequence observes BOTH read-only views; every set observes `for x`.
    for seq in &a {
        let names: Vec<String> = seq.observables().into_iter().map(|o| o.name).collect();
        if seq.is_dict {
            assert!(
                names.iter().any(|n| n.contains("_val_"))
                    && names.iter().any(|n| n.contains("_key_")),
                "{}: dict must observe both .values() and .keys()",
                seq.tag
            );
        } else {
            assert!(
                names.iter().any(|n| n.contains("_set_")),
                "{}: set must observe `for x in s`",
                seq.tag
            );
        }
    }
}

#[test]
fn iteration_matches_cpython_over_scrambled_sequences() {
    let seqs = corpus();

    // EMIT path holds regardless of WABT (also asserted in `iteration_fuzz_lowers`).
    for seq in &seqs {
        emit(&seq.wasm_source()).expect("iteration sequence lowers");
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1300: skipping EXECUTED iteration fuzz — WABT (wat2wasm / wasm-interp) \
             absent. Every sequence lowered through emit_module; a box with WABT + python3 \
             runs every observable and value-matches live CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1300: skipping iteration fuzz value-diff — python3 (the oracle) absent.");
        return;
    }

    let oracle = match python_oracle(&seqs) {
        Some(o) => o,
        None => {
            eprintln!("PMAT-1300: python3 oracle unavailable — skipping value diff.");
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
            "{} trapped (overflow or empty-container access):\n{stdout}",
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
                    "{}: WASM={got} CPython={expected}\n{}",
                    obs.name, obs.def_text
                ));
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "PMAT-1300: {} WASM/CPython divergence(s) over the iteration corpus:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert_eq!(
        checked,
        oracle.len(),
        "every CPython observable must be matched by a WASM export"
    );

    eprintln!(
        "PMAT-1300: iteration fuzz PASSED — {checked} commutative-body iteration observables \
         across {} dict/set sequences executed in WABT and matched live python3. \
         `for x in s` / `for v in d.values()` / `for k in d.keys()` stayed CPython-exact \
         for sum/product/xor/count/min/max after random del/discard/add swap-into-hole \
         scrambling.",
        seqs.len()
    );
}

/// The PMAT-1292 defense: an order-DEPENDENT loop body reads the arbitrary
/// bump-heap storage order and would silently diverge from CPython. Every such
/// iteration MUST refuse. EMIT path only — no WABT required.
#[test]
fn order_dependent_iteration_refuses() {
    let cases: &[(&str, &str)] = &[
        (
            "for-x-in-set",
            "def go() -> int:\n    s: set[int] = {50, 3, 27}\n    r: int = 0\n    for x in s:\n        r = r * 10 + x\n    return r\n",
        ),
        (
            "for-v-in-values",
            "def go() -> int:\n    d: dict[int, int] = {50: 7, 3: 2, 27: 9}\n    r: int = 0\n    for v in d.values():\n        r = r * 10 + v\n    return r\n",
        ),
        (
            "for-k-in-keys",
            "def go() -> int:\n    d: dict[int, int] = {50: 1, 3: 1, 27: 1}\n    r: int = 0\n    for k in d.keys():\n        r = r * 10 + k\n    return r\n",
        ),
    ];
    for (name, src) in cases {
        let err = emit(src).err().unwrap_or_else(|| {
            panic!(
                "PMAT-1300: `{name}` is an order-DEPENDENT iteration and MUST refuse — a silent \
                 accept is the PMAT-1292 storage-order-misread miscompile class"
            )
        });
        assert!(
            err.contains("order-dependent"),
            "`{name}` refusal should name the order-dependent iteration, got: {err}"
        );
    }
}

/// A bare `for k in d` over a dict mutated ANYWHERE in the function routes to the
/// keys-snapshot + size-change-guard form the WASM subset does not model, so it
/// refuses — a scrambled dict must be iterated through `.keys()` / `.values()`
/// (which lower as `List(K)` views and hit the order-safety gate instead).
#[test]
fn bare_for_k_over_mutated_dict_refuses() {
    let src = "def go() -> int:\n    d: dict[int, int] = {10: 1, 20: 2, 30: 3}\n    del d[20]\n    total: int = 0\n    for k in d:\n        total = total + k\n    return total\n";
    let err = emit(src).expect_err("bare `for k in d` over a mutated dict must refuse");
    assert!(
        err.contains("MUTATED dict") || err.contains("unsupported construct"),
        "refusal should name the mutated-dict snapshot form, got: {err}"
    );
}

/// A str-VALUED `.values()` iteration is not in the int-only value lane and MUST
/// refuse rather than misread the value slot as an i64.
#[test]
fn str_valued_values_iteration_refuses() {
    let src = "def go() -> int:\n    d: dict[int, str] = {1: \"aa\", 2: \"bbb\", 3: \"c\"}\n    total: int = 0\n    for v in d.values():\n        total = total + len(v)\n    return total\n";
    let err = emit(src).expect_err("str-valued `.values()` iteration must refuse");
    assert!(
        err.contains("unsupported construct") || err.contains("WASM") || err.contains("str"),
        "refusal should reject the str-valued value iteration, got: {err}"
    );
}
