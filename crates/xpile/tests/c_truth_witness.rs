//! PMAT-1382 (XPILE-CTRUTH-001): the C lane's TRUTH-VALUE BRIDGE.
//!
//! C has no boolean type. A comparison, a logical `&&`/`||` and a logical `!`
//! all have type `int` and yield `0`/`1`, and CONVERSELY any scalar is a legal
//! controlling expression — `if (a)` means `if (a != 0)`. Rust's `bool` is a
//! distinct type in both directions, so the C→Rust path needs an explicit
//! bridge wherever the two typings disagree. Through v0.1.617 there was none,
//! and `emit_c_binop`'s own comment asserted the opposite: that a comparison is
//! "correct for `if`/`&&`/`||` operand positions, which is where the C frontend
//! places them". The frontend places them wherever C does, which is anywhere an
//! `int` may appear.
//!
//! THE SILENT WRONG ANSWER. C `!x` emitted Rust `!(x)` — BYTE IDENTICAL to the
//! `~x` arm — so `int f(int a) { return !a; }` compiled cleanly and returned
//! `-6` for `a = 5` where gcc returns `0`. Measured through the shipped CLI
//! against live gcc, not asserted. Two more of the same class rode along:
//! a leading-zero integer literal is OCTAL in C (C17 6.4.4.1) but took a
//! base-10 `parse()`, so `010` computed 10 where gcc computes 8; and the
//! "widest wins" width pick silently retyped a whole function across the
//! INT/FLOAT boundary, so `int trunc_it(double a) { return a; }` returned
//! `3.9` where gcc returns `3` (C's truncating return conversion deleted).
//!
//! THE ACCEPT-THEN-REJECT CLASS. Ten further shapes exited 0 emitting Rust
//! `rustc` REJECTS — the shape PMAT-1378 closed for WASM and PMAT-1381 for the
//! Rust lane: `if (a)` / `while (a)` / `a ? x : y` on an int condition (E0308),
//! `a && b` / `a || b` on int operands (E0308), a comparison used as a value
//! (`return a < b;`, `(a < b) + 10`, `a & b == 0`) (E0308/E0599), and a
//! REASSIGNED PARAMETER — the idiomatic C countdown `while (n) { n = n - 1; }`
//! — which emitted a non-`mut` binding (E0384) because decy's `mark_mutable`
//! reaches only `Stmt::Let` locals.
//!
//! PMAT-1399 EXTENDED the accept-then-reject class to INTEGER LITERAL RANGE.
//! C converts a constant that does not fit its destination MODULO 2^N (C17
//! 6.3.1.3p2) with only a `-Woverflow`; Rust REJECTS it. The emitter wrote
//! `<digits><suffix>` verbatim, so `unsigned int f(void) { return 5000000000; }`
//! emitted `5000000000u32` and exited 0 on Rust rustc refuses — in the return,
//! local, call-argument and arithmetic-operand positions alike, at both the
//! `i32` and `u32` widths. The fix CONVERTS (matching the WASM lane's
//! PMAT-1395), which is exact under `+ - * & | ^` but not under `/ % `, a
//! comparison, a logical `&&`/`||`/`!` or a controlling expression, where C
//! evaluates the constant at its own wider type first — those REFUSE, so the
//! fix cannot swap an uncompilable emission for a silent wrong answer.
//!
//! The load-bearing test is [`transpiled_c_either_refuses_or_rustc_accepts_it`]:
//! it asserts the PROPERTY `Ok(rust) ==> rustc accepts it` over the corpus
//! rather than pinning one message, because a per-shape assertion cannot catch
//! the NEXT shape that leaks. [`c_truth_bridge_agrees_with_cc`] is the
//! EXECUTING half — it compiles the same C with `cc` and byte-compares the two
//! programs' stdout, so a bridge that merely type-checks cannot pass.
//!
//! Gated on `cc` + `rustc` presence (skips with a reason, like the oracle).
//! `XPILE_REQUIRE_CC=1` turns a missing `cc` into a FAILURE rather than a skip,
//! so the witness cannot silently decay to skip-green in CI.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn tool_present(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn rustc_present() -> bool {
    tool_present("rustc")
}

/// The system C compiler. `cc` is the POSIX spelling; `gcc` is the fallback.
fn cc_bin() -> Option<&'static str> {
    if tool_present("cc") {
        Some("cc")
    } else if tool_present("gcc") {
        Some("gcc")
    } else {
        None
    }
}

/// PMAT-1375 TRAP 4, applied here: without a tripwire, deleting the toolchain
/// silently returns this witness to skip-green. Scoped to its own env var —
/// never `CI=true`, because a runner without a C compiler must still be able
/// to skip by design.
fn require_cc() -> bool {
    std::env::var("XPILE_REQUIRE_CC").is_ok_and(|v| v == "1")
}

/// A per-CALL unique scratch directory. Keying it on (tag, pid) is NOT enough:
/// `transpile`, `rustc_accepts` and `cc_run` are separate calls for the same
/// tag and the tests run on parallel threads — a shared directory gets wiped
/// mid-compile and the compiler fails to LINK its own object files, which reads
/// exactly like an emitter defect. The atomic counter makes each call disjoint.
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join("xpile-c-truth").join(format!(
        "{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// `Ok(rust_source)` when the C frontend accepts, `Err(stderr)` when it refuses.
fn transpile(csrc: &str, tag: &str) -> Result<String, String> {
    let dir = scratch(tag);
    let c = dir.join("probe.c");
    std::fs::write(&c, csrc).expect("write probe");
    let out = Command::new(xpile_bin())
        .args(["transpile", c.to_str().unwrap(), "--target", "rust"])
        .output()
        .expect("spawn xpile");
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// `Ok(())` when `rustc` type-checks the emitted Rust AS A LIBRARY.
///
/// The emit path produces `pub fn` items and no `main`, so a bin-crate check
/// would fail every source with E0601 and the property would be measuring the
/// harness rather than the emitter.
fn rustc_accepts_lib(rust: &str, tag: &str) -> Result<(), String> {
    let dir = scratch(tag);
    let rs = dir.join("probe.rs");
    std::fs::write(&rs, rust).expect("write rust");
    let out = Command::new("rustc")
        .arg("--edition=2021")
        .arg("--crate-type=lib")
        .arg("-A")
        .arg("dead_code")
        .arg("--emit=metadata")
        .arg("--out-dir")
        .arg(&dir)
        .arg(&rs)
        .output()
        .expect("spawn rustc");
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// `Ok(binary)` when `rustc` accepts the emitted Rust, `Err(stderr)` otherwise.
fn rustc_accepts(rust: &str, tag: &str) -> Result<PathBuf, String> {
    let dir = scratch(tag);
    let rs = dir.join("probe.rs");
    std::fs::write(&rs, rust).expect("write rust");
    let bin = dir.join("probe");
    let out = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-A")
        .arg("dead_code")
        .arg("-o")
        .arg(&bin)
        .arg(&rs)
        .output()
        .expect("spawn rustc");
    if out.status.success() {
        Ok(bin)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

fn run(bin: &PathBuf) -> String {
    let out = Command::new(bin).output().expect("run probe binary");
    String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string()
}

/// Compile `csrc` + `driver` as ONE translation unit with `cc` and run it.
/// One TU matters: with a separate driver TU every callee silently defaults to
/// `int f()`, which fakes divergences on `long`/`unsigned`/`double` returns.
fn cc_run(csrc: &str, driver: &str, tag: &str) -> String {
    let cc = cc_bin().expect("cc present");
    let dir = scratch(tag);
    let c = dir.join("whole.c");
    std::fs::write(&c, format!("#include <stdio.h>\n{csrc}\n{driver}")).expect("write c");
    let bin = dir.join("cprobe");
    let out = Command::new(cc)
        .arg("-O0")
        .arg("-w")
        .arg("-o")
        .arg(&bin)
        .arg(&c)
        .output()
        .expect("spawn cc");
    assert!(
        out.status.success(),
        "[{tag}] the PROBE ITSELF is not valid C — cc rejected it:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    run(&bin)
}

// ---------------------------------------------------------------------------
// The corpus. `(tag, c_source, calls)` — `calls` is the shared driver body,
// printed identically by both languages so the two stdouts are byte-comparable.
// ---------------------------------------------------------------------------

/// Every entry names a function `f` and a list of `(args)` call spellings.
/// Both drivers print `(long long)` / `as i64` of each call, one per line.
const CORPUS: &[(&str, &str, &[&str])] = &[
    // --- logical `!`: the silent wrong answer (was BYTE IDENTICAL to `~`) ---
    (
        "lognot",
        "int f(int a) { return !a; }",
        &["5", "0", "-1", "1", "-7"],
    ),
    (
        "lognot_double",
        "int f(int a) { return !!a; }",
        &["5", "0", "-3"],
    ),
    (
        "lognot_triple",
        "int f(int a) { return !!!a; }",
        &["5", "0", "-3"],
    ),
    (
        "lognot_of_cmp",
        "int f(int a) { return !(a < 3); }",
        &["1", "5"],
    ),
    (
        "lognot_in_arith",
        "int f(int a) { return !a + 5; }",
        &["0", "7"],
    ),
    (
        "lognot_in_mul",
        "int f(int a) { return (!a) * 3; }",
        &["0", "7"],
    ),
    (
        "lognot_in_bitand",
        "int f(int a) { return (!a) & 3; }",
        &["0", "1"],
    ),
    ("lognot_of_neg", "int f(int a) { return !-a; }", &["0", "5"]),
    (
        "lognot_of_bitnot",
        "int f(int a) { return !~a; }",
        &["-1", "0"],
    ),
    // --- `~` must remain BITWISE (the arm `!` was colliding with) ---
    (
        "bitnot_stays_bitwise",
        "int f(int a) { return ~a; }",
        &["5", "0", "-1"],
    ),
    // --- int in a controlling position (was E0308) ---
    (
        "if_int_cond",
        "int f(int a) { if (a) { return 100; } return 200; }",
        &["5", "0"],
    ),
    (
        "while_int_cond",
        "int f(int a) { int s = 0; while (a) { s = s + a; a = a - 1; } return s; }",
        &["4", "0"],
    ),
    (
        "ternary_int_cond",
        "int f(int a) { return a ? 10 : 20; }",
        &["5", "0", "-2"],
    ),
    (
        "nested_int_cond",
        "int f(int a) { if (a) { if (a - 1) { return 1; } return 2; } return 3; }",
        &["5", "1", "0"],
    ),
    // --- `&&` / `||` on ints (was E0308), short-circuit preserved ---
    (
        "andand_int",
        "int f(int a) { return a && (a - 1); }",
        &["0", "1", "5"],
    ),
    (
        "oror_int",
        "int f(int a) { return a || (a + 1); }",
        &["0", "5", "-1"],
    ),
    (
        "and_shortcircuit",
        "int g(int x) { return 100 / x; }\nint f(int a) { return a && g(a); }",
        &["0", "5"],
    ),
    (
        "or_shortcircuit",
        "int g(int x) { return 100 / x; }\nint f(int a) { return a || g(a); }",
        &["4", "1"],
    ),
    // --- truth value used as an int (was E0308 / E0599) ---
    (
        "cmp_as_value",
        "int f(int a) { return a < 3; }",
        &["1", "5"],
    ),
    (
        "cmp_in_arith",
        "int f(int a) { return (a < 3) + 10; }",
        &["1", "5"],
    ),
    (
        "cmp_in_shift",
        "int f(int a) { return (a < 3) << 4; }",
        &["1", "5"],
    ),
    (
        "cmp_chain",
        "int f(int a) { return a < 2 < 1; }",
        &["1", "5"],
    ),
    (
        "bitand_eq_prec",
        "int f(int a) { return a & 0 == 0; }",
        &["6", "7"],
    ),
    (
        "truth_into_local",
        "int f(int a) { int t = a < 3; if (t) { return 7; } return 8; }",
        &["1", "5"],
    ),
    (
        "deep_mixed",
        "int f(int a, int b) { return ((a && b) || !a) + (a < b) * 2; }",
        &["1, 1", "0, 1", "2, 0"],
    ),
    // --- reassigned PARAMETER (was E0384) ---
    (
        "param_reassign",
        "int f(int a) { a = a + 1; return a; }",
        &["5"],
    ),
    (
        "param_reassign_if",
        "int f(int a) { if (a > 0) { a = a * 2; } return a; }",
        &["5", "-5"],
    ),
    (
        "param_countdown",
        "int f(int n) { int s = 0; while (n) { s = s + n; n = n - 1; } return s; }",
        &["4", "0"],
    ),
    // --- octal (was a base-10 parse) ---
    ("octal_010", "int f(void) { return 010; }", &[""]),
    ("octal_0777", "int f(void) { return 0777; }", &[""]),
    ("octal_07", "int f(void) { return 07; }", &[""]),
    ("octal_zero", "int f(void) { return 0; }", &[""]),
    ("octal_00", "int f(void) { return 00; }", &[""]),
    ("octal_in_expr", "int f(int a) { return a + 010; }", &["1"]),
    ("decimal_unaffected", "int f(void) { return 10; }", &[""]),
    // --- the bridge at the other integer widths ---
    ("long_lognot", "long f(long a) { return !a; }", &["5", "0"]),
    (
        "long_truthy",
        "long f(long a) { if (a) { return 100; } return 200; }",
        &["5", "0"],
    ),
    (
        "unsigned_lognot",
        "unsigned f(unsigned a) { return !a; }",
        &["5", "0"],
    ),
    (
        "unsigned_and",
        "unsigned f(unsigned a) { return a && (a - 1); }",
        &["0", "3"],
    ),
    (
        "ulong_lognot",
        "unsigned long f(unsigned long a) { return !a; }",
        &["5", "0"],
    ),
    // --- neighbours the bridge must not have disturbed ---
    (
        "plain_arith",
        "int f(int a) { return a * 3 + a - 1; }",
        &["6"],
    ),
    (
        "bitor_and_prec",
        "int f(int a) { return a | 6 & 3; }",
        &["8"],
    ),
    (
        "shift_add_prec",
        "int f(int a) { return a + 2 << 3; }",
        &["1"],
    ),
    ("div_trunc", "int f(int a) { return a / 2; }", &["7", "-7"]),
    ("mod_trunc", "int f(int a) { return a % 2; }", &["7", "-7"]),
    (
        "recursion",
        "int f(int n) { return n <= 1 ? 1 : n * f(n - 1); }",
        &["6"],
    ),
    (
        "early_return",
        "int f(int x) { if (x > 0) { return 1; } if (x < 0) { return -1; } return 0; }",
        &["3", "-3", "0"],
    ),
    // --- PMAT-1399: an integer literal OUTSIDE the declared width's range.
    // C converts it modulo 2^N (C17 6.3.1.3p2) with only a `-Woverflow`; Rust
    // REJECTS it. Through v0.1.617 the emitter wrote `<digits><suffix>`
    // verbatim, so every one of these exited 0 emitting Rust rustc refuses —
    // in the return, local, argument AND arithmetic-operand positions alike.
    (
        "lit_u32_over",
        "unsigned f(void) { return 5000000000; }",
        &[""],
    ),
    (
        "lit_u32_over_local",
        "unsigned f(void) { unsigned x = 5000000000; return x; }",
        &[""],
    ),
    (
        "lit_u32_over_arg",
        "unsigned g(unsigned y) { return y; }\nunsigned f(void) { return g(5000000000); }",
        &[""],
    ),
    (
        "lit_u32_over_add",
        "unsigned f(void) { return 5000000000 + 1; }",
        &[""],
    ),
    (
        "lit_u32_over_mul",
        "unsigned f(void) { return 5000000000 * 2; }",
        &[""],
    ),
    (
        "lit_u32_over_bitand",
        "unsigned f(void) { return 5000000000 & 255; }",
        &[""],
    ),
    ("lit_u32_neg_one", "unsigned f(void) { return -1; }", &[""]),
    ("lit_i32_over", "int f(void) { return 5000000000; }", &[""]),
    (
        "lit_i32_intmax_plus1",
        "int f(void) { return 2147483648; }",
        &[""],
    ),
    (
        "lit_i32_intmin_minus1",
        "int f(void) { return -2147483649; }",
        &[""],
    ),
    (
        "lit_i32_over_sub",
        "int f(void) { return 5000000000 - 1; }",
        &[""],
    ),
    (
        "lit_i32_over_in_param_fn",
        "int f(int a) { return a + 5000000000; }",
        &["1", "-1"],
    ),
    // The widths that CANNOT be out of range must be untouched: `i64` is the
    // literal's own width, and a float width renders `<v>f64`/`<v>f32`.
    (
        "lit_i64_in_range",
        "long f(void) { return 9000000000000000000; }",
        &[""],
    ),
    (
        "lit_u64_neg_one",
        "unsigned long f(void) { return -1; }",
        &[""],
    ),
    (
        "lit_f32_rounds",
        "float f(void) { return 16777217; }",
        &[""],
    ),
    (
        "lit_f64_exact",
        "double f(void) { return 16777217; }",
        &[""],
    ),
    (
        "lit_in_range_unaffected",
        "unsigned f(void) { return 7; }",
        &[""],
    ),
    // The NON-MODULAR contexts, which REFUSE today (see
    // `out_of_range_literal_in_a_non_modular_context_refuses`). They are in the
    // corpus so that if a future change ever makes them EMIT, the executing
    // half catches the divergence from `cc` — a refusal is invisible to a
    // compile-only property, and reducing the literal early is exactly the
    // silent wrong answer the guard exists to prevent (`5000000000u / 2` is
    // 2500000000 in C, 352516352 once reduced). MEASURED, not asserted: with
    // the guard removed and the conversion left on, `c_truth_bridge_agrees_with_cc`
    // reds on all five — div cc=2500000000/xpile=352516352, mod cc=2/xpile=5,
    // cmp cc=1/xpile=0, truthy cc=0/xpile=1, ternary cc=1/xpile=0.
    (
        "lit_u32_over_div",
        "unsigned f(void) { return 5000000000 / 2; }",
        &[""],
    ),
    (
        "lit_u32_over_mod",
        "unsigned f(void) { return 5000000000 % 7; }",
        &[""],
    ),
    (
        "lit_u32_over_cmp",
        "unsigned f(void) { return 5000000000 > 4000000000; }",
        &[""],
    ),
    (
        "lit_u32_over_truthy",
        "unsigned f(void) { return !4294967296; }",
        &[""],
    ),
    (
        "lit_u32_over_ternary",
        "unsigned f(void) { return 4294967296 ? 1 : 0; }",
        &[""],
    ),
];

fn c_driver(calls: &[&str]) -> String {
    let body: Vec<String> = calls
        .iter()
        .map(|a| format!(r#"printf("%lld\n", (long long)(f({a})));"#))
        .collect();
    format!("int main(void) {{\n{}\nreturn 0;\n}}\n", body.join("\n"))
}

fn rust_driver(calls: &[&str]) -> String {
    let body: Vec<String> = calls
        .iter()
        .map(|a| format!(r#"println!("{{}}", (f({a})) as i64);"#))
        .collect();
    format!("fn main() {{\n{}\n}}\n", body.join("\n"))
}

// ---------------------------------------------------------------------------
// The load-bearing property.
// ---------------------------------------------------------------------------

/// THE PROPERTY: `Ok(rust) ==> rustc accepts it`.
///
/// A refusal is always acceptable — the C lane refuses most of C. What is NOT
/// acceptable is exiting 0 on Rust that does not compile. Stated as a property
/// rather than a per-shape message pin so it also catches the next shape.
#[test]
fn transpiled_c_either_refuses_or_rustc_accepts_it() {
    if !rustc_present() {
        eprintln!("SKIP: rustc not present");
        return;
    }
    let mut accepted = 0usize;
    let mut refused = 0usize;
    let mut leaks: Vec<String> = Vec::new();
    for (tag, csrc, _) in CORPUS {
        match transpile(csrc, tag) {
            Err(_) => refused += 1,
            Ok(rust) => match rustc_accepts_lib(&rust, tag) {
                Ok(()) => accepted += 1,
                Err(e) => {
                    let first = e
                        .lines()
                        .find(|l| l.starts_with("error"))
                        .unwrap_or("<no error line>");
                    leaks.push(format!("  {tag}: {first}\n--- emitted ---\n{rust}"));
                }
            },
        }
    }
    assert!(
        leaks.is_empty(),
        "XPILE-CTRUTH-001: `xpile transpile <c> --target rust` EXITED 0 emitting Rust \
         that rustc REJECTS for {} of {} corpus sources:\n{}",
        leaks.len(),
        CORPUS.len(),
        leaks.join("\n")
    );
    eprintln!(
        "XPILE-CTRUTH-001[property]: {accepted} accepted+compiled, {refused} refused, \
         0 accept-then-reject over {} sources",
        CORPUS.len()
    );
    assert!(
        accepted >= 40,
        "the corpus stopped exercising the bridge: only {accepted} sources still \
         transpile (expected >= 40) — a blanket refusal would pass the property \
         vacuously"
    );
}

/// THE EXECUTING HALF: the emitted Rust must compute what `cc` computes.
///
/// A bridge that merely type-checks is not enough — emitting `!(x)` for C `!x`
/// type-checked perfectly and returned the wrong number. This compiles the same
/// C with `cc`, runs both programs and byte-compares their stdout.
#[test]
fn c_truth_bridge_agrees_with_cc() {
    if !rustc_present() {
        eprintln!("SKIP: rustc not present");
        return;
    }
    let Some(cc) = cc_bin() else {
        assert!(
            !require_cc(),
            "XPILE_REQUIRE_CC=1 but no `cc`/`gcc` on PATH — the executing half of \
             XPILE-CTRUTH-001 would have silently skipped"
        );
        eprintln!("SKIP: no cc/gcc present (set XPILE_REQUIRE_CC=1 to make this a failure)");
        return;
    };
    let mut executed = 0usize;
    let mut diverged: Vec<String> = Vec::new();
    for (tag, csrc, calls) in CORPUS {
        let Ok(rust) = transpile(csrc, tag) else {
            continue;
        };
        let Ok(bin) = rustc_accepts(&format!("{rust}\n{}", rust_driver(calls)), tag) else {
            continue; // the property test above is what fails on this
        };
        let got = run(&bin);
        let want = cc_run(csrc, &c_driver(calls), tag);
        executed += 1;
        if got != want {
            diverged.push(format!("  {tag}: cc={want:?} xpile-rust={got:?}\n{rust}"));
        }
    }
    assert!(
        diverged.is_empty(),
        "XPILE-CTRUTH-001: the emitted Rust DISAGREES with {cc} on {} of {executed} \
         executed sources:\n{}",
        diverged.len(),
        diverged.join("\n")
    );
    eprintln!("XPILE-CTRUTH-001[executing]: {executed} sources agree with {cc} byte-for-byte");
    assert!(
        executed >= 40,
        "only {executed} sources executed (expected >= 40) — the executing half \
         must not decay into a handful of probes"
    );
}

// ---------------------------------------------------------------------------
// Targeted pins for each root cause, so a regression names itself.
// ---------------------------------------------------------------------------

/// The specific collision: C `!` and C `~` are DIFFERENT operators and must not
/// emit the same Rust. Through v0.1.617 both emitted `!(operand)`.
#[test]
fn logical_not_and_bitwise_not_do_not_emit_the_same_rust() {
    let lognot = transpile("int f(int a) { return !a; }", "pin_lognot").expect("! accepted");
    let bitnot = transpile("int f(int a) { return ~a; }", "pin_bitnot").expect("~ accepted");
    let body = |s: &str| {
        s.lines()
            .find(|l| l.trim_start().starts_with("((") || l.trim_start().starts_with("!("))
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let (lb, bb) = (body(&lognot), body(&bitnot));
    assert_ne!(
        lb, bb,
        "C `!a` and `~a` emit IDENTICAL Rust — `!` is being lowered as a bitwise \
         invert, so `!5` returns -6 where C returns 0"
    );
    assert!(
        lb.contains("!= 0"),
        "C `!a` must lower through the `!= 0` truthiness test, got: {lb}"
    );
    assert_eq!(bb, "!(a)", "C `~a` must stay a bare Rust bitwise invert");
}

/// C17 6.4.4.1: a leading zero makes an integer literal OCTAL.
#[test]
fn leading_zero_integer_literal_is_octal() {
    let rust = transpile("int f(void) { return 010; }", "pin_octal").expect("octal accepted");
    assert!(rust.contains("8i32"), "C `010` is octal 8, emitted: {rust}");
    let rust = transpile("int f(void) { return 0777; }", "pin_octal2").expect("octal accepted");
    assert!(
        rust.contains("511i32"),
        "C `0777` is octal 511, emitted: {rust}"
    );
    // 8 and 9 are not octal digits — refuse rather than guess a base.
    let err = transpile("int f(void) { return 08; }", "pin_octal_bad")
        .expect_err("`08` is not valid C and must not lift");
    assert!(
        err.contains("octal"),
        "the refusal must name the octal reading, got: {err}"
    );
}

/// "Widest wins" is value-preserving across the SIGNED INTEGER widths but not
/// across the int/float boundary, where it deletes C's truncating conversion.
#[test]
fn mixed_int_and_float_width_refuses_rather_than_retyping() {
    for (tag, src) in [
        ("ret_int_param_double", "int f(double a) { return a; }"),
        ("param_int_ret_double", "double f(int a) { return a; }"),
        (
            "mixed_params",
            "double f(double a, int b) { return a + b; }",
        ),
        ("cmp_ret_int", "int f(double a, double b) { return a < b; }"),
    ] {
        let err = transpile(src, tag).expect_err(
            "a function mixing an integer type with the float width must REFUSE — \
             retyping it silently drops C's truncating conversion",
        );
        assert!(
            err.contains("mixes the float width"),
            "[{tag}] the refusal must name the mixed width, got: {err}"
        );
    }
    // The UNIFORMLY-float functions must still transpile — the refusal is
    // narrow, not a retreat from the float lane.
    for (tag, src) in [
        (
            "uniform_add",
            "double f(double a, double b) { return a + b; }",
        ),
        ("uniform_lognot", "double f(double a) { return !a; }"),
        (
            "uniform_cmp",
            "double f(double a, double b) { return a < b; }",
        ),
        (
            "uniform_if",
            "double f(double a) { if (a) { return 1.5; } return 2.5; }",
        ),
    ] {
        transpile(src, tag).unwrap_or_else(|e| {
            panic!("[{tag}] a uniformly-double function must still transpile, got: {e}")
        });
    }
}

/// C parameters are ordinary mutable locals; decy's `mark_mutable` reaches only
/// `Stmt::Let`, so a reassigned parameter emitted a non-`mut` binding (E0384).
#[test]
fn reassigned_parameter_is_emitted_mut() {
    let rust = transpile("int f(int a) { a = a + 1; return a; }", "pin_mut").expect("accepted");
    assert!(
        rust.contains("pub fn f(mut a: i32)"),
        "a reassigned parameter must be `mut`, emitted: {rust}"
    );
    // A parameter that is NOT reassigned must stay immutable — `clippy
    // -D warnings` runs over the emit-crate path and an unused `mut` is a lint.
    let rust = transpile("int f(int a) { int b = a + 1; return b; }", "pin_nomut").expect("ok");
    assert!(
        rust.contains("pub fn f(a: i32)"),
        "an un-reassigned parameter must NOT be `mut`, emitted: {rust}"
    );
}

/// PMAT-1399: C converts an integer constant that does not fit the destination
/// type MODULO 2^N (C17 6.3.1.3p2), diagnosing at most `-Woverflow`; Rust
/// REJECTS an out-of-range literal outright (`deny(overflowing_literals)`).
/// Through v0.1.617 the C→Rust emitter wrote `<digits><suffix>` verbatim, so
/// `unsigned int f(void) { return 5000000000; }` emitted `5000000000u32` and
/// `--target rust` exited 0 on Rust `rustc` refuses. This is the RUST-lane dual
/// of the WASM lane's PMAT-1395 fix; both convert rather than refuse so the two
/// lanes agree on the value.
#[test]
fn integer_literal_outside_the_width_converts_modulo_like_c() {
    for (tag, src, want) in [
        (
            "u32_ret",
            "unsigned f(void) { return 5000000000; }",
            "705032704u32",
        ),
        (
            "i32_ret",
            "int f(void) { return 5000000000; }",
            "705032704i32",
        ),
        (
            "i32_intmax_plus1",
            "int f(void) { return 2147483648; }",
            "-2147483648i32",
        ),
        (
            "u32_local",
            "unsigned f(void) { unsigned x = 5000000000; return x; }",
            "705032704u32",
        ),
        (
            "u32_add",
            "unsigned f(void) { return 5000000000 + 1; }",
            "705032704u32",
        ),
        (
            "u32_arg",
            "unsigned g(unsigned y) { return y; }\nunsigned f(void) { return g(5000000000); }",
            "705032704u32",
        ),
        // The NEGATED out-of-range operand: `-2147483649` lifts as
        // `Neg(LitInt(2147483649))`, so the OPERAND is what is out of range.
        // Converting it to `-2147483647i32` and then applying the existing
        // `wrapping_neg` reproduces C's 2147483647 exactly.
        (
            "i32_intmin_minus1",
            "int f(void) { return -2147483649; }",
            "-2147483647i32",
        ),
    ] {
        let rust = transpile(src, tag).unwrap_or_else(|e| panic!("[{tag}] must transpile: {e}"));
        assert!(
            rust.contains(want),
            "[{tag}] expected the C-converted literal `{want}`, emitted: {rust}"
        );
    }
    // An IN-RANGE literal is emitted verbatim — the conversion is not a
    // blanket rewrite that happens to make the out-of-range case compile.
    for (tag, src, want) in [
        ("inrange_u32", "unsigned f(void) { return 7; }", "7u32"),
        (
            "inrange_i32",
            "int f(void) { return 2147483647; }",
            "2147483647i32",
        ),
        (
            "inrange_i64",
            "long f(void) { return 9000000000000000000; }",
            "9000000000000000000i64",
        ),
        (
            "float_width",
            "float f(void) { return 16777217; }",
            "16777217f32",
        ),
    ] {
        let rust = transpile(src, tag).unwrap_or_else(|e| panic!("[{tag}] must transpile: {e}"));
        assert!(
            rust.contains(want),
            "[{tag}] an in-range literal must be emitted verbatim as `{want}`, emitted: {rust}"
        );
    }
}

/// PMAT-1399: the modular conversion is exact ONLY under the operators that
/// depend on the low N bits (`+ - * & | ^`). Under `/`, `%`, a comparison, a
/// logical `&&`/`||`/`!` or a controlling expression, C first evaluates the
/// constant at its own wider type, so reducing it early computes a DIFFERENT
/// answer — `5000000000u / 2` is 2500000000 in C but 352516352 once the
/// literal is reduced. Refuse there rather than trade one exit-0 lie
/// (uncompilable Rust) for the worse one (a wrong answer that compiles).
#[test]
fn out_of_range_literal_in_a_non_modular_context_refuses() {
    for (tag, src) in [
        ("div", "unsigned f(void) { return 5000000000 / 2; }"),
        ("mod", "unsigned f(void) { return 5000000000 % 7; }"),
        (
            "cmp",
            "unsigned f(void) { return 5000000000 > 4000000000; }",
        ),
        ("andand", "unsigned f(void) { return 4294967296 && 1; }"),
        ("oror", "unsigned f(void) { return 4294967296 || 0; }"),
        ("lognot", "unsigned f(void) { return !4294967296; }"),
        (
            "if_cond",
            "unsigned f(void) { if (4294967296) { return 1; } return 0; }",
        ),
        ("ternary", "unsigned f(void) { return 4294967296 ? 1 : 0; }"),
        (
            "while_cond",
            "unsigned f(unsigned a) { unsigned s = 0; while (4294967296) { s = a; } return s; }",
        ),
        // The hazard is TRANSITIVE — a modular subtree under a non-modular
        // parent is still reduced before the non-modular step happens.
        (
            "nested_under_div",
            "unsigned f(void) { return (5000000000 + 0) / 2; }",
        ),
        (
            "nested_in_call_arg",
            "unsigned g(unsigned y) { return y; }\nunsigned f(void) { return g(5000000000 / 2); }",
        ),
    ] {
        let err = transpile(src, tag).expect_err(
            "an out-of-range literal in a NON-MODULAR context must REFUSE — reducing \
             it early computes a different value than C",
        );
        assert!(
            err.contains("outside the range of its arithmetic width"),
            "[{tag}] the refusal must name the range, got: {err}"
        );
    }
    // The refusal must be NARROW: the modular operators, every in-range
    // program, and the widths that cannot overflow all still transpile.
    for (tag, src) in [
        ("add", "unsigned f(void) { return 5000000000 + 1; }"),
        ("sub", "int f(void) { return 5000000000 - 1; }"),
        ("mul", "unsigned f(void) { return 5000000000 * 2; }"),
        ("bitand", "unsigned f(void) { return 5000000000 & 255; }"),
        ("bitor", "unsigned f(void) { return 5000000000 | 1; }"),
        ("bitxor", "unsigned f(void) { return 5000000000 ^ 3; }"),
        ("inrange_div", "unsigned f(void) { return 100 / 2; }"),
        ("inrange_mod", "int f(int a) { return a % 2; }"),
        ("inrange_cmp", "unsigned f(void) { return 5 > 4; }"),
        (
            "inrange_if",
            "int f(void) { if (3) { return 1; } return 0; }",
        ),
        ("inrange_lognot", "int f(int a) { return !a; }"),
        (
            "i64_width_div",
            "long f(void) { return 9000000000000000000 / 2; }",
        ),
        ("float_width_div", "double f(void) { return 16777217 / 2; }"),
    ] {
        transpile(src, tag)
            .unwrap_or_else(|e| panic!("[{tag}] must still transpile — the refusal over-red: {e}"));
    }
}

/// PMAT-1399: a C constant too large for the meta-HIR's own `i64` literal never
/// reaches the emitter — the FRONTEND refuses it. Pinned so the emitter-side
/// conversion above is not credited with covering a case it never sees.
#[test]
fn integer_literal_wider_than_i64_refuses_at_the_frontend() {
    let err = transpile(
        "unsigned long f(void) { return 10000000000000000000UL; }",
        "pin_over_i64",
    )
    .expect_err("a constant above i64::MAX must refuse, not wrap silently");
    assert!(
        err.contains("does not fit in i64"),
        "the refusal must name the i64 literal width, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Residuals: measured, still wrong, PINNED rather than hidden (PMAT-1379/1381
// posture). Each asserts TODAY's behaviour so closing it reds this test.
// ---------------------------------------------------------------------------

/// C's `int` is 32-bit but decy lifts it to the meta-HIR `Type::I64`, and a
/// function mixing `int` with `long` rides `i64`. That is value-preserving for
/// the arithmetic, but a C `int` function's WRAP WIDTH is 32 bits: an `int`
/// function that overflows wraps at `i32` in C. The emitted `i32` width does
/// match; this pins the MIXED case, where an `int` parameter silently gains
/// 32 bits of range because the function was widened to `i64`.
#[test]
fn known_residual_int_param_widens_in_a_long_function() {
    let rust = transpile("long f(int a, long b) { return a + b; }", "res_widen").expect("accepted");
    assert!(
        rust.contains("pub fn f(a: i64, b: i64)"),
        "RESIDUAL (v0.1.618): a C `int` parameter in a `long` function is emitted \
         `i64`, so a caller may pass a value no C caller could. Value-preserving \
         for in-range inputs, hence not refused with the int/float case; if this \
         test reds, the widening was fixed and the pin should be removed. \
         Emitted: {rust}"
    );
}

/// The same widening across the SIGNEDNESS boundary is NOT value-preserving:
/// C converts the `int` to `unsigned` (mod 2^32), while the emitted `u32`
/// parameter simply cannot receive a negative argument.
#[test]
fn known_residual_signed_param_retyped_unsigned() {
    let rust = transpile(
        "unsigned f(int a, unsigned b) { return a + b; }",
        "res_sign",
    )
    .expect("accepted");
    assert!(
        rust.contains("pub fn f(a: u32, b: u32)"),
        "RESIDUAL (v0.1.618): a C `int` parameter in an `unsigned` function is \
         emitted `u32`. C's usual arithmetic conversions make `f(-1, 0)` yield \
         4294967295; the emitted signature cannot express the call at all. \
         Narrower than the int/float case (no value is silently mis-COMPUTED, \
         the call is simply unspellable), so it is pinned rather than refused. \
         Emitted: {rust}"
    );
}
