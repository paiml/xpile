//! PMAT-1342 — ADVERSARIAL-VERIFY the scalar `NumBuiltin` belt (PMAT-1338..1341):
//! `abs` (1338), integer `min`/`max` (1339), the `math.floor`/`ceil`/`trunc`
//! rounding trio (1340), and domain-guarded `math.sqrt` (1341). Runs on the
//! scalar runtime (`C-COMPILE-RUST-TO-WASM`).
//!
//! ## Why this is a SKEPTIC pass, not a feature
//!
//! Four slices grew off ONE shared node — `Expr::NumBuiltin { op, of_float, args }`
//! — routed through four helpers (`$__wasm_abs_i64`, `$__wasm_min_i64`,
//! `$__wasm_max_i64`, `$__wasm_sqrt_f64`) plus two inline native forms
//! (`f64.abs`, the `f64.floor`/`ceil`/`trunc` + `i64.trunc_f64_s` narrow). A
//! regression hides at the SEAMS the per-slice witnesses do not individually
//! stress: the per-op GATE walkers, the CROSS-op nestings (each op's gate must
//! fire THROUGH another op's node), the rounding SENSE on negatives (where
//! `floor` and `trunc` part company), single-EVALUATION of a side-effecting
//! operand, and the auxiliary classifiers a new int-valued expression must be
//! threaded into.
//!
//! ## What this pass REFUTED (and this slice fixes)
//!
//! **`f"{min(a, b)}"` / `f"{max(a, b)}"` REFUSED to lower.** `concat_operand_is_int`
//! — the classifier that decides which format operands get auto-wrapped in the
//! sign-aware `str(int)` (`$__wasm_int_to_str`) — was threaded by PMAT-1338 for
//! `abs` and by PMAT-1340 for `floor`/`ceil`/`trunc`, but PMAT-1339 shipped the
//! int `min`/`max` emit WITHOUT it. So an int-valued `min` in a format position
//! fell through to the generic "expression in a string position" refusal while
//! its two neighbours in the same belt lowered fine. The fix adds `Min | Max` to
//! the `abs` arm (same `of_float` key — an all-int min/max is i64-valued because
//! a min/max never leaves its operand set). Safe direction (a REFUSAL, never a
//! miscompile), but a real capability hole and exactly the belt-seam class this
//! cadence exists to catch.
//!
//! ## What this pass CONFIRMED (nothing else refuted)
//!
//!   * GATE completeness — the `stmt_has_num_builtin` walker's `_ => false` tail
//!     is SOUND: the only `Stmt` variants carrying an `Expr` that the WASM lane
//!     actually emits are already armed. `ForEach`/`ForEachPair` are rewritten to
//!     `Let`+`While` by `desugar_module_foreach` BEFORE any gate scan (verified by
//!     executing a min in a `for` and in an `enumerate` body); `Print` /
//!     `FieldIndexAssign` / `LetTuple` / `IndexAppend` / `NestedSubscriptAssign` /
//!     `ListExtend` / `Assert` refuse outright; `ListMutate` carries no `Expr`
//!     (`Sort`/`SortDesc`/`Reverse`/`Clear`); and `DictUpdate`'s `other` must be a
//!     plain `Ident`, so no `NumBuiltin` can hide in any of them;
//!   * CROSS-op gate firing — each gate fires THROUGH another op's node
//!     (`abs(math.sqrt(x))` arms sqrt, `abs(min(a, b))` arms min, `min(abs(a), b)`
//!     arms abs, `max(a, min(b, c))` arms BOTH) and each helper is declared
//!     EXACTLY once; an undeclared helper at a `call` site is a hard `wat2wasm`
//!     failure, the recurring gate-hole class;
//!   * GATE independence — a min-only module carries no `max` helper (and
//!     vice-versa); a NumBuiltin-free module carries none of the four;
//!   * ROUNDING SENSE on NEGATIVES — the seam where the trio parts:
//!     `floor(-2.5) == -3` but `trunc(-2.5) == -2` and `ceil(-2.5) == -2`; and
//!     `floor(-0.5) == -1` vs `ceil(-0.5)`/`trunc(-0.5) == 0` (a `trunc`-for-`floor`
//!     swap is invisible on POSITIVES, which is why the negatives carry the check);
//!   * SINGLE-EVALUATION — a side-effecting operand (`d.pop(k)`) is emitted
//!     exactly ONCE under `abs`, under `min` in first position, and under `max` in
//!     SECOND position (the fold's tail leg);
//!   * the VARIADIC left fold — `min(a, b, c, d)` emits 3 `call`s against 1 helper
//!     declaration and reduces left-to-right like CPython, ties included.
//!
//! ## A claim SHARPENED (documented, not a defect)
//!
//! The f-string int path re-emits its operand FIVE times (`f"{abs(n)}"` yields 5
//! `call $__wasm_abs_i64`), so the belt's single-evaluation guarantee does NOT
//! extend through `str(int)`. It is UNOBSERVABLE: a side-effecting operand in a
//! format position (`f"{abs(d.pop(k))}"`) is REFUSED outright, and every operand
//! that IS admitted there is pure and deterministic, so N evaluations yield one
//! value. Recorded so a future slice that widens the format subset knows to
//! re-check it.
//!
//! Every executed probe is FULL-pipeline (REAL Python → `PythonFrontend` →
//! `emit_module` → `wat2wasm` → `wasm-interp`), value-matched against LIVE
//! python3 running the IDENTICAL source. Gated on WABT — a clean skip (still
//! asserting emit, gates and refusals) without it.

use std::path::Path;
use std::process::Command;

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

fn decl_count(wat: &str, helper: &str) -> usize {
    wat.matches(&format!("(func {helper} ")).count()
}

// ---- the probe corpus --------------------------------------------------------

/// Each `(name, body)` becomes a zero-arg `def <name>() -> int`. Every probe is
/// INT-valued so the interp's `i64:N` line compares EXACTLY against python3 —
/// including the string probes, which are observed through `len` / `ord` so the
/// f-string CONTENT is checked in int space with no byte-readback driver.
fn probes() -> Vec<(&'static str, &'static str)> {
    vec![
        // ── abs (PMAT-1338): sign, zero, nesting, expression operand ─────────
        ("ab_neg", "    n = -42\n    return abs(n)\n"),
        ("ab_pos", "    n = 42\n    return abs(n)\n"),
        ("ab_zero", "    n = 0\n    return abs(n)\n"),
        ("ab_expr", "    a = 3\n    b = 10\n    return abs(a - b)\n"),
        ("ab_nested", "    n = -5\n    return abs(abs(n))\n"),
        (
            "ab_big",
            "    n = -9223372036854775807\n    return abs(n)\n",
        ),
        // ── min / max (PMAT-1339): order, ties, negatives, variadic fold ─────
        ("mn_pair", "    a = 3\n    b = -7\n    return min(a, b)\n"),
        ("mx_pair", "    a = 3\n    b = -7\n    return max(a, b)\n"),
        ("mn_tie", "    a = 5\n    b = 5\n    return min(a, b)\n"),
        ("mx_tie", "    a = 5\n    b = 5\n    return max(a, b)\n"),
        (
            "mn_negs",
            "    a = -3\n    b = -9\n    return min(a, b)\n",
        ),
        (
            "mn_var3",
            "    a = 9\n    b = 2\n    c = 7\n    return min(a, b, c)\n",
        ),
        (
            "mx_var4",
            "    a = 1\n    b = 8\n    c = 3\n    d = 8\n    return max(a, b, c, d)\n",
        ),
        (
            "mn_var_left",
            // the LEFT fold observed: min(min(min(5,1),9),4) == 1
            "    a = 5\n    b = 1\n    c = 9\n    d = 4\n    return min(a, b, c, d)\n",
        ),
        (
            "mn_i64_min",
            "    a = -9223372036854775808\n    b = 0\n    return min(a, b)\n",
        ),
        (
            "mx_i64_max",
            "    a = 9223372036854775807\n    b = 0\n    return max(a, b)\n",
        ),
        // ── CROSS-op composition (each gate must fire through the other node) ─
        (
            "mx_of_mn",
            "    a = 4\n    b = 1\n    c = 9\n    return max(a, min(b, c))\n",
        ),
        (
            "mn_of_ab",
            "    a = -6\n    b = 4\n    return min(abs(a), b)\n",
        ),
        (
            "ab_of_mn",
            "    a = -6\n    b = 4\n    return abs(min(a, b))\n",
        ),
        // ── rounding SENSE (PMAT-1340) — the NEGATIVES carry the check ───────
        ("fl_neg", "    x = -2.5\n    return math.floor(x)\n"),
        ("ce_neg", "    x = -2.5\n    return math.ceil(x)\n"),
        ("tr_neg", "    x = -2.5\n    return math.trunc(x)\n"),
        ("fl_pos", "    x = 2.5\n    return math.floor(x)\n"),
        ("ce_pos", "    x = 2.5\n    return math.ceil(x)\n"),
        ("tr_pos", "    x = 2.5\n    return math.trunc(x)\n"),
        ("fl_halfneg", "    x = -0.5\n    return math.floor(x)\n"),
        ("ce_halfneg", "    x = -0.5\n    return math.ceil(x)\n"),
        ("tr_halfneg", "    x = -0.5\n    return math.trunc(x)\n"),
        ("fl_exact", "    x = 3.0\n    return math.floor(x)\n"),
        ("tr_negexact", "    x = -3.0\n    return math.trunc(x)\n"),
        ("ce_negexact", "    x = -3.0\n    return math.ceil(x)\n"),
        // ── sqrt (PMAT-1341) composed with the rest of the belt ──────────────
        (
            "sq_floor",
            "    x = 17.0\n    return math.floor(math.sqrt(x))\n",
        ),
        (
            "sq_of_fabs",
            "    x = -16.0\n    return math.floor(math.sqrt(abs(x)))\n",
        ),
        (
            "sq_scaled",
            "    return math.floor(math.sqrt(2.0) * 1000000.0)\n",
        ),
        // ── control-flow nesting (post-desugar reachability, executed) ───────
        (
            "mn_in_for",
            "    xs = [4, -2, 9]\n    t = 0\n    for x in xs:\n        t = t + min(x, 3)\n    return t\n",
        ),
        (
            "mx_in_enumerate",
            "    xs = [7, 1, 5]\n    t = 0\n    for i, x in enumerate(xs):\n        t = t + max(i, x)\n    return t\n",
        ),
        (
            "ab_in_while",
            "    n = 0\n    t = 0\n    while n < 3:\n        t = t + abs(n - 2)\n        n = n + 1\n    return t\n",
        ),
        // ── the REFUTED-then-FIXED seam: min/max in a FORMAT position ────────
        // Observed through len/ord so the CONTENT is checked exactly.
        (
            "fs_mn_len",
            "    a = 3\n    b = -7\n    s = f\"m={min(a, b)}\"\n    return len(s)\n",
        ),
        (
            "fs_mn_c2",
            "    a = 3\n    b = -7\n    s = f\"m={min(a, b)}\"\n    return ord(s[2])\n",
        ),
        (
            "fs_mn_c3",
            "    a = 3\n    b = -7\n    s = f\"m={min(a, b)}\"\n    return ord(s[3])\n",
        ),
        (
            "fs_mx_len",
            "    a = 3\n    b = -7\n    s = f\"m={max(a, b)}\"\n    return len(s)\n",
        ),
        (
            "fs_mx_c2",
            "    a = 3\n    b = -7\n    s = f\"m={max(a, b)}\"\n    return ord(s[2])\n",
        ),
        (
            "fs_mn_var3",
            "    a = 9\n    b = 2\n    c = 7\n    s = f\"v={min(a, b, c)}!\"\n    return len(s)\n",
        ),
        // the NEIGHBOURS that already worked — regression pins for the fix
        (
            "fs_ab_len",
            "    n = -42\n    s = f\"m={abs(n)}\"\n    return len(s)\n",
        ),
        (
            "fs_fl_c2",
            "    x = -2.5\n    s = f\"m={math.floor(x)}\"\n    return ord(s[2])\n",
        ),
    ]
}

fn observable_names() -> Vec<String> {
    probes().iter().map(|(n, _)| (*n).to_string()).collect()
}

/// The whole corpus as ONE python module — the identical text goes to both
/// `emit_module` and live python3.
fn corpus_source() -> String {
    let mut src = String::from("import math\n");
    for (name, body) in probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}"));
    }
    src
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn min_max_in_a_format_position_lowers() {
    // The REFUTED seam: PMAT-1339 shipped int min/max without threading
    // `concat_operand_is_int`, so this refused while `abs`/`floor` lowered.
    for (what, src) in [
        (
            "min",
            "def f(a: int, b: int) -> str:\n    return f\"m={min(a, b)}\"\n",
        ),
        (
            "max",
            "def f(a: int, b: int) -> str:\n    return f\"m={max(a, b)}\"\n",
        ),
        (
            "variadic min",
            "def f(a: int, b: int, c: int) -> str:\n    return f\"m={min(a, b, c)}\"\n",
        ),
        (
            "min nested under abs",
            "def f(a: int, b: int) -> str:\n    return f\"m={abs(min(a, b))}\"\n",
        ),
    ] {
        let wat = match emit(src) {
            Ok(w) => w,
            Err(e) => panic!("`{what}` in a format position must lower, got refusal: {e}"),
        };
        // It must materialise through the SIGN-AWARE int→str helper — a min can
        // be negative, so a naive unsigned rendering would be wrong.
        assert!(
            wat.contains("call $__wasm_int_to_str"),
            "`{what}` in a format position must render via the sign-aware str(int):\n{wat}"
        );
    }
}

#[test]
fn a_float_min_max_still_refuses_in_every_position() {
    // The fix keys on `of_float`, so a FLOAT min/max must stay refused (WASM's
    // `f64.min`/`max` do not match Python's order-dependent NaN semantics) —
    // both in value position and in a format position.
    assert!(
        emit("def f(a: float, b: float) -> float:\n    return min(a, b)\n").is_err(),
        "a float min must still refuse (NaN-order mismatch)"
    );
    assert!(
        emit("def f(a: float, b: float) -> str:\n    return f\"m={min(a, b)}\"\n").is_err(),
        "a float min in a format position must still refuse — the fix must not \
         mislabel it as int-valued"
    );
}

#[test]
fn every_gate_fires_through_another_num_builtin_node() {
    // The recurring gate-hole class: a `call` whose helper the walker missed is
    // an UNDECLARED function — a hard wat2wasm failure. Each op's gate must fire
    // even when its node is buried in ANOTHER op's argument.
    for (what, src, helper) in [
        (
            "sqrt under abs",
            "import math\ndef f(x: float) -> float:\n    return abs(math.sqrt(x))\n",
            "$__wasm_sqrt_f64",
        ),
        (
            "min under abs",
            "def f(a: int, b: int) -> int:\n    return abs(min(a, b))\n",
            "$__wasm_min_i64",
        ),
        (
            "abs under min",
            "def f(a: int, b: int) -> int:\n    return min(abs(a), b)\n",
            "$__wasm_abs_i64",
        ),
        (
            "min under max",
            "def f(a: int, b: int, c: int) -> int:\n    return max(a, min(b, c))\n",
            "$__wasm_min_i64",
        ),
        (
            "sqrt under floor",
            "import math\ndef f(x: float) -> int:\n    return math.floor(math.sqrt(x))\n",
            "$__wasm_sqrt_f64",
        ),
        (
            "min in a for body",
            "def f(xs: list[int]) -> int:\n    t = 0\n    for x in xs:\n        t = t + min(x, 10)\n    return t\n",
            "$__wasm_min_i64",
        ),
        (
            "max in an enumerate body",
            "def f(xs: list[int]) -> int:\n    t = 0\n    for i, x in enumerate(xs):\n        t = t + max(i, x)\n    return t\n",
            "$__wasm_max_i64",
        ),
        (
            "min in a while condition",
            "def f(a: int) -> int:\n    n = 0\n    while min(n, a) < 5:\n        n = n + 1\n    return n\n",
            "$__wasm_min_i64",
        ),
        (
            "abs in an if inside a while",
            "def f(a: int) -> int:\n    t = 0\n    n = 0\n    while n < 3:\n        if n > 0:\n            t = t + abs(a)\n        n = n + 1\n    return t\n",
            "$__wasm_abs_i64",
        ),
    ] {
        let wat = emit(src).unwrap_or_else(|e| panic!("`{what}` must lower: {e}"));
        assert!(
            wat.contains(&format!("call {helper}")),
            "`{what}` must CALL {helper}:\n{wat}"
        );
        assert_eq!(
            decl_count(&wat, helper),
            1,
            "`{what}` must DECLARE {helper} exactly once (undeclared = hard wat2wasm fail):\n{wat}"
        );
    }
}

#[test]
fn the_per_op_gates_stay_independent() {
    // A min-only module carries no `max` helper and vice-versa; a belt-free
    // module carries none of the four.
    let min_only = emit("def f(a: int, b: int) -> int:\n    return min(a, b)\n").expect("min");
    assert_eq!(decl_count(&min_only, "$__wasm_min_i64"), 1);
    assert!(
        !min_only.contains("$__wasm_max_i64"),
        "a min-only module must carry no dead max helper:\n{min_only}"
    );
    let max_only = emit("def f(a: int, b: int) -> int:\n    return max(a, b)\n").expect("max");
    assert_eq!(decl_count(&max_only, "$__wasm_max_i64"), 1);
    assert!(
        !max_only.contains("$__wasm_min_i64"),
        "a max-only module must carry no dead min helper:\n{max_only}"
    );
    let none = emit("def f(a: int, b: int) -> int:\n    return a + b\n").expect("plain");
    for helper in [
        "$__wasm_abs_i64",
        "$__wasm_min_i64",
        "$__wasm_max_i64",
        "$__wasm_sqrt_f64",
    ] {
        assert!(
            !none.contains(helper),
            "a NumBuiltin-free module must not carry {helper}:\n{none}"
        );
    }
}

#[test]
fn the_variadic_fold_is_pairwise_and_shares_one_helper() {
    let wat = emit("def f(a: int, b: int, c: int, d: int) -> int:\n    return min(a, b, c, d)\n")
        .expect("variadic min");
    assert_eq!(
        wat.matches("call $__wasm_min_i64").count(),
        3,
        "a 4-operand min folds through 3 pairwise calls:\n{wat}"
    );
    assert_eq!(
        decl_count(&wat, "$__wasm_min_i64"),
        1,
        "the 3 calls share ONE helper declaration:\n{wat}"
    );
}

#[test]
fn a_side_effecting_operand_is_evaluated_exactly_once() {
    // The belt CLAIMS the operand is pushed as the helper arg (single-eval), so
    // a `d.pop(k)` operand must not be double-run — in FIRST and in TAIL
    // position of the fold.
    for (what, src) in [
        (
            "abs",
            "def f(d: dict[int, int]) -> int:\n    return abs(d.pop(1))\n",
        ),
        (
            "min, first operand",
            "def f(d: dict[int, int]) -> int:\n    return min(d.pop(1), 5)\n",
        ),
        (
            "max, tail operand",
            "def f(d: dict[int, int]) -> int:\n    return max(5, d.pop(1))\n",
        ),
    ] {
        let wat = emit(src).unwrap_or_else(|e| panic!("`{what}` must lower: {e}"));
        let pops = wat.matches("call $__wasm_dict_pop").count()
            + wat.matches("call $__wasm_hash_pop").count();
        assert_eq!(
            pops, 1,
            "`{what}`: a side-effecting operand must be emitted EXACTLY once, got {pops}:\n{wat}"
        );
    }
    // The SHARPENED claim: the f-string int path re-emits its operand (5×), so
    // single-evaluation does NOT extend through `str(int)`. It is unobservable
    // because a side-effecting operand in a format position is REFUSED — pin
    // that refusal so a future format-subset widening re-checks the guarantee.
    assert!(
        emit("def f(d: dict[int, int]) -> str:\n    return f\"v={abs(d.pop(1))}\"\n").is_err(),
        "a side-effecting operand in a FORMAT position must refuse (the f-string \
         int path re-emits its operand, so single-eval does not hold there)"
    );
}

// ---- WABT harness -------------------------------------------------------------

fn export_line<'a>(stdout: &'a str, name: &str) -> &'a str {
    let needle = format!("{name}() =>");
    stdout
        .lines()
        .find(|l| l.contains(&needle))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"))
}

/// Parse the value out of a `name() => <ty>:<v>` interp line as an `f64`.
///
/// `wasm-interp` prints an integer result UNSIGNED — `math.trunc(-3.0)` comes
/// back as `i64:18446744073709551613`, not `i64:-3` — so the raw digits must be
/// read as a `u64`/`u32` and REINTERPRETED at the declared width before the
/// differential. (This corpus is the first in the belt whose observables go
/// NEGATIVE, which is exactly why it surfaces the encoding: a witness whose
/// probes are all non-negative never sees it. Reading the digits directly would
/// compare `1.8e19` against CPython's `-7`.)
fn parse_export_f64(stdout: &str, name: &str) -> f64 {
    let line = export_line(stdout, name);
    let (ty, raw) = line
        .rsplit_once("=> ")
        .and_then(|(_, v)| v.trim().split_once(':'))
        .unwrap_or_else(|| panic!("malformed export line for {name}: {line:?}"));
    let raw = raw.trim();
    match ty {
        "i64" => raw
            .parse::<u64>()
            .map(|u| u as i64 as f64)
            .unwrap_or_else(|e| panic!("parse i64 {name}: {e}")),
        "i32" => raw
            .parse::<u32>()
            .map(|u| u as i32 as f64)
            .unwrap_or_else(|e| panic!("parse i32 {name}: {e}")),
        _ => raw
            .parse::<f64>()
            .unwrap_or_else(|e| panic!("parse {ty} {name}: {e}")),
    }
}

/// Assemble + run. `tag` keeps per-test work dirs disjoint (parallel libtest
/// threads must not race on one `prog.wat`).
fn assemble_and_run(wat: &str, tag: &str) -> (String, bool) {
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-numverify-{}-{tag}", std::process::id()));
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
        "wat2wasm failed:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    (stdout, run.status.success())
}

/// Execute the IDENTICAL corpus in live python3 — the differential ground truth.
fn python_truth(src: &str) -> Option<Vec<(String, f64)>> {
    let names = observable_names();
    let driver =
        format!("{src}\nprint(';'.join(f'{{n}}={{float(globals()[n]())}}' for n in {names:?}))\n");
    let out = Command::new("python3")
        .arg("-c")
        .arg(&driver)
        .output()
        .ok()?;
    if !out.status.success() {
        panic!(
            "python3 failed on the witness corpus:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Some(
        stdout
            .trim()
            .split(';')
            .map(|kv| {
                let (k, v) = kv.split_once('=').expect("k=v");
                (k.to_string(), v.parse::<f64>().expect("float"))
            })
            .collect(),
    )
}

// ---- EXECUTED witness (gated on WABT + python3) --------------------------------

#[test]
fn the_num_builtin_belt_executes_and_matches_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the NumBuiltin verify corpus must lower");

    // All four helpers coexist in ONE module, each declared exactly once.
    for helper in [
        "$__wasm_abs_i64",
        "$__wasm_min_i64",
        "$__wasm_max_i64",
        "$__wasm_sqrt_f64",
    ] {
        assert_eq!(
            decl_count(&wat, helper),
            1,
            "the corpus must declare {helper} exactly once:\n"
        );
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1342: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit/gate level only (the executed leg runs every export and \
             value-matches live python3 on the identical source when WABT is present)."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1342: python3 absent — witness asserted at emit/gate level only");
        return;
    };
    assert_eq!(
        truth.len(),
        observable_names().len(),
        "python3 must produce one value per observable probe"
    );

    let (stdout, ok) = assemble_and_run(&wat, "corpus");
    assert!(ok, "wasm-interp run failed:\n{stdout}");
    assert!(
        !stdout.contains("=> error:"),
        "no probe in this corpus may TRAP:\n{stdout}"
    );

    for (name, expected) in &truth {
        let got = parse_export_f64(&stdout, name);
        assert_eq!(
            got, *expected,
            "NumBuiltin export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    assert!(
        truth.len() >= 44,
        "expected at least 44 observable probes value-matched, got {}",
        truth.len()
    );
}
