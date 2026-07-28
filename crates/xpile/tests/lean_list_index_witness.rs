//! XPILE-WITNESS (Lean lane) — PMAT-1426: everything the `--target lean` lane
//! ACCEPTS in its FLOAT / STRING / LIST vocabularies both elaborates under
//! `lean` **and computes CPython's value**.
//!
//! WHAT WAS WRONG. `Expr::Index` emitted a bare `xs[i.toNat]!`. `Int.toNat`
//! CLAMPS a negative to `0`, and CPython reads a negative subscript from the
//! END of the list, so the emission answered the FIRST element where Python
//! answers the LAST — at exit 0, citing `C-XLATE-PY-LIST-TO-VEC`:
//!
//! ```text
//!   def f(a: list[int]) -> int:  return a[-1]
//!
//!   emitted:  def f (a : List (Int)) : Int := a[(-1: Int).toNat]!
//!   lean   :  f [10, 20, 30]  =>  10
//!   CPython:    [10, 20, 30][-1]  =>  30
//! ```
//!
//! The under-range case was wrong in the same direction and worse: CPython
//! raises `IndexError` for `[10,20,30][-5]`, while `(-5).toNat` clamped to `0`
//! and returned `10`. Only the OVER-range case (`[10,20,30][5]`) was already
//! faithful, because `[...]!` has its own `outOfBounds` panic.
//!
//! This is the SAME clamping shape PMAT-1425 removed from `>>` and `**` one
//! vocabulary over, and it shows the same cross-lane asymmetry that was the
//! tell there: `--target rust` and `--target ruchy` both emit the Python rule
//! explicitly (`if __li < 0 { __lc.len() as i64 + __li }` plus a bounds
//! `panic!("xpile: IndexError: list index out of range")`). Only Lean — the
//! lane whose whole purpose is machine-checked semantics — returned a wrong
//! value at exit 0.
//!
//! WHY IT SURVIVED. PMAT-1425 closed ACCEPT ⟹ ELABORATES for the integer
//! BINARY OPERATOR vocabulary and left a recorded lead: the float, string and
//! list vocabularies had no corpus-driven gate of their own. They still do not
//! have an *elaboration* problem — a 55-source sweep of all three came back
//! with every accepted row elaborating, before and after this fix. **The
//! defect was invisible to elaboration.** A gate that only asked "does `lean`
//! accept the output" would have passed on `a[-1]` forever; it takes the
//! VALUE half to see it. So this file asserts both, and the value half is the
//! load-bearing one.
//!
//! WHAT THIS ASSERTS, over the vocabulary rather than over the fixed arm:
//!
//!   1. ACCEPT ⟹ ELABORATES. For every corpus source `xpile` accepts, `lean`
//!      must accept the emitted file.
//!   2. ACCEPT ⟹ COMPUTES CPYTHON'S VALUE. Each accepted row carries
//!      obligations pinned to a value re-derived from CPython 3, discharged
//!      in-file (`by decide`, or `by native_decide` for `Float`, which has no
//!      `DecidableEq`). This is the assertion that catches a clamp.
//!   3. REFUSE ⟹ FOR THE STATED REASON, keyed per source — never a bare
//!      `is_err()`, which several of these rows would satisfy vacuously for an
//!      unrelated upstream cause.
//!   4. PRESERVED CAPABILITY. Non-negative indexing, and the whole
//!      float/string/list surface around it, must still lower. Over-refusal is
//!      the natural failure mode of a semantics fix (PMAT-1419); the measured
//!      corpus delta of this one was ZERO (`xpile audit --target lean`:
//!      emitted 160, requiring 141, with_citation 119, errors 805, F1 84.3% —
//!      identical on both binaries).
//!   5. ANTI-VACUITY + a MEASURED premise. The corpus must carry both
//!      dispositions non-trivially, and `Int.toNat`'s clamping — the reason the
//!      guard exists at all — is measured against the toolchain rather than
//!      restated from a comment (PMAT-1425 lesson 3).
//!
//! Skips with reason when `lean` / the xpile bin is absent (the hosted
//! workspace-test runner has no Lean toolchain) — never silently green. The
//! refusal half and a structural half both run without a toolchain.

use std::process::Command;

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn lean_present() -> bool {
    Command::new("lean")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A per-CALL unique directory. Two probes sharing one directory have produced
/// cross-test clobbering in this repo before.
fn probe_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join("xpile-lean-list-index-witness")
        .join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir probe dir");
    dir
}

enum Disposition {
    /// Lowers. The emitted Lean must elaborate AND discharge every obligation.
    Accept {
        obligations: &'static [&'static str],
    },
    /// Refuses. `reason` must appear in the diagnostic — keyed per source so a
    /// refusal firing for an unrelated cause cannot satisfy the assertion.
    Refuse { reason: &'static str },
}

struct Case {
    name: &'static str,
    python: &'static str,
    disposition: Disposition,
}

/// The FLOAT / STRING / LIST vocabulary of the Lean lane — the three the
/// PMAT-1425 sweep left without a gate. Every `Accept` value was re-derived
/// from CPython 3, not copied from the emission.
const CORPUS: &[Case] = &[
    // ================= LIST INDEX — the defect and its boundary =================
    Case {
        name: "index_negative_literal",
        python: "def f(a: list[int]) -> int:\n    return a[-1]\n",
        disposition: Disposition::Accept {
            // CPython: [10,20,30][-1] == 30. Through v0.1.617 this answered 10.
            // The 1-element list is the row where the wrap and the clamp would
            // coincide, so both are pinned.
            obligations: &[
                "example : f [10, 20, 30] = 30 := by decide",
                "example : f [42] = 42 := by decide",
            ],
        },
    },
    Case {
        name: "index_negative_first",
        python: "def f(a: list[int]) -> int:\n    return a[-3]\n",
        disposition: Disposition::Accept {
            // CPython: [10,20,30][-3] == 10 — the wrap lands exactly on 0, so a
            // clamping emission would have been ACCIDENTALLY RIGHT here. Kept as
            // the row that distinguishes "wraps" from "happens to agree".
            obligations: &["example : f [10, 20, 30] = 10 := by decide"],
        },
    },
    Case {
        name: "index_runtime",
        python: "def f(a: list[int], i: int) -> int:\n    return a[i]\n",
        disposition: Disposition::Accept {
            // A runtime index takes BOTH branches of the resolution.
            obligations: &[
                "example : f [10, 20, 30] 0 = 10 := by decide",
                "example : f [10, 20, 30] 2 = 30 := by decide",
                "example : f [10, 20, 30] (-1) = 30 := by decide",
                "example : f [10, 20, 30] (-2) = 20 := by decide",
            ],
        },
    },
    Case {
        name: "index_zero_literal",
        python: "def f(a: list[int]) -> int:\n    return a[0]\n",
        disposition: Disposition::Accept {
            obligations: &["example : f [10, 20, 30] = 10 := by decide"],
        },
    },
    Case {
        name: "index_str_element",
        python: "def f(a: list[str]) -> str:\n    return a[-1]\n",
        disposition: Disposition::Accept {
            // The guard emits `panic!`, which needs `Inhabited` at the ELEMENT
            // type. `[...]!` needed it too, so this adds no requirement — but a
            // non-`Int` element type is the row that would show it if it did.
            obligations: &["example : f [\"a\", \"b\", \"c\"] = \"c\" := by decide"],
        },
    },
    Case {
        name: "index_nested_list_element",
        python: "def f(a: list[list[int]]) -> list[int]:\n    return a[-1]\n",
        disposition: Disposition::Accept {
            obligations: &["example : f [[1], [2]] = [2] := by decide"],
        },
    },
    Case {
        name: "index_of_index",
        python: "def f(a: list[int], b: list[int]) -> int:\n    return a[b[-1]]\n",
        disposition: Disposition::Accept {
            // Nesting reuses the emission's `let` names; `let` is lexically
            // scoped, so shadowing is correct. This row is what proves it.
            // CPython: a=[10,20,30], b=[0,1,2] -> b[-1]==2 -> a[2]==30.
            obligations: &["example : f [10, 20, 30] [0, 1, 2] = 30 := by decide"],
        },
    },
    // ================= LIST — the surrounding surface =================
    Case {
        name: "list_int_literal",
        python: "def f() -> list[int]:\n    return [1, 2, 3]\n",
        disposition: Disposition::Accept {
            obligations: &["example : f = [1, 2, 3] := by decide"],
        },
    },
    Case {
        name: "list_empty_literal",
        python: "def f() -> list[int]:\n    return []\n",
        disposition: Disposition::Accept {
            obligations: &["example : f = ([] : List Int) := by decide"],
        },
    },
    Case {
        name: "list_concat",
        python: "def f(a: list[int], b: list[int]) -> list[int]:\n    return a + b\n",
        disposition: Disposition::Accept {
            obligations: &["example : f [1, 2] [3] = [1, 2, 3] := by decide"],
        },
    },
    Case {
        name: "list_len",
        python: "def f(a: list[int]) -> int:\n    return len(a)\n",
        disposition: Disposition::Accept {
            obligations: &[
                "example : f [1, 2, 3] = 3 := by decide",
                "example : f [] = 0 := by decide",
            ],
        },
    },
    Case {
        name: "list_eq",
        python: "def f(a: list[int], b: list[int]) -> bool:\n    return a == b\n",
        disposition: Disposition::Accept {
            obligations: &[
                "example : f [1, 2] [1, 2] = true := by decide",
                "example : f [1, 2] [2, 1] = false := by decide",
            ],
        },
    },
    Case {
        name: "list_nested_literal",
        python: "def f() -> list[list[int]]:\n    return [[1], [2]]\n",
        disposition: Disposition::Accept {
            obligations: &["example : f = [[1], [2]] := by decide"],
        },
    },
    // ================= STRING =================
    Case {
        name: "str_concat",
        python: "def f(a: str, b: str) -> str:\n    return a + b\n",
        disposition: Disposition::Accept {
            obligations: &["example : f \"ab\" \"c\" = \"abc\" := by decide"],
        },
    },
    Case {
        name: "str_lt",
        python: "def f(a: str, b: str) -> bool:\n    return a < b\n",
        disposition: Disposition::Accept {
            obligations: &[
                "example : f \"abc\" \"abd\" = true := by decide",
                "example : f \"abd\" \"abc\" = false := by decide",
            ],
        },
    },
    Case {
        name: "str_eq",
        python: "def f(a: str, b: str) -> bool:\n    return a == b\n",
        disposition: Disposition::Accept {
            obligations: &[
                "example : f \"x\" \"x\" = true := by decide",
                "example : f \"x\" \"y\" = false := by decide",
            ],
        },
    },
    Case {
        name: "str_truthiness",
        python: "def f(a: str) -> bool:\n    return bool(a)\n",
        disposition: Disposition::Accept {
            // CPython: bool("") is False, bool("x") is True.
            obligations: &[
                "example : f \"\" = false := by decide",
                "example : f \"x\" = true := by decide",
            ],
        },
    },
    Case {
        name: "str_escapes_and_unicode",
        python: "def f() -> str:\n    return \"a\\\"b\\\\c\\nd\\te\\u00e9\"\n",
        disposition: Disposition::Accept {
            // The emitter writes `\n` and `\t` as RAW bytes inside the Lean
            // literal (Lean string literals span lines). Measured: the code
            // points round-trip exactly, so this is faithful — pinned here so a
            // future escape-table edit cannot silently change it. CPython:
            // 10 chars, ending U+00E9.
            obligations: &[
                "example : f.length = 10 := by decide",
                "example : f.toList.map Char.toNat = [97, 34, 98, 92, 99, 10, 100, 9, 101, 233] \
                 := by decide",
            ],
        },
    },
    // ================= FLOAT =================
    // `Float` has no `DecidableEq`, so these discharge via `native_decide` on a
    // `Bool` built with `Float`'s `BEq`.
    Case {
        name: "float_div_literal",
        python: "def f(a: float) -> float:\n    return a / 2.0\n",
        disposition: Disposition::Accept {
            obligations: &["example : (f 7.0 == 3.5) = true := by native_decide"],
        },
    },
    Case {
        name: "float_floordiv_literal",
        python: "def f(a: float) -> float:\n    return a // 2.0\n",
        disposition: Disposition::Accept {
            // CPython: -5.0 // 2.0 == -3.0 (floors toward -inf, not toward 0).
            obligations: &[
                "example : (f (-5.0) == -3.0) = true := by native_decide",
                "example : (f 5.0 == 2.0) = true := by native_decide",
            ],
        },
    },
    Case {
        name: "float_mod_literal",
        python: "def f(a: float) -> float:\n    return a % 2.0\n",
        disposition: Disposition::Accept {
            // CPython: -5.0 % 2.0 == 1.0 — the sign follows the DIVISOR.
            obligations: &[
                "example : (f (-5.0) == 1.0) = true := by native_decide",
                "example : (f 5.0 == 1.0) = true := by native_decide",
            ],
        },
    },
    Case {
        name: "float_pow",
        python: "def f(a: float, b: float) -> float:\n    return a ** b\n",
        disposition: Disposition::Accept {
            obligations: &["example : (f 2.0 10.0 == 1024.0) = true := by native_decide"],
        },
    },
    Case {
        name: "float_literal_round_trip",
        python: "def f() -> float:\n    return 1e300\n",
        disposition: Disposition::Accept {
            // The emitter prints an f64 with Rust's `Display`, which never uses
            // exponent notation — `1e300` becomes a 301-digit literal. Measured:
            // it round-trips to the same double. Pinned so a future literal-
            // formatting change cannot silently lose precision.
            obligations: &["example : (f == 1e300) = true := by native_decide"],
        },
    },
    Case {
        name: "float_cmp",
        python: "def f(a: float, b: float) -> bool:\n    return a < b\n",
        disposition: Disposition::Accept {
            obligations: &[
                "example : f 1.0 2.0 = true := by native_decide",
                "example : f 2.0 1.0 = false := by native_decide",
            ],
        },
    },
    // ================= REFUSALS, keyed per source =================
    // These bound the vocabulary. Each is here so a future widening cannot
    // quietly re-enable one without also stating what it now emits.
    Case {
        name: "list_slice",
        python: "def f(a: list[int]) -> list[int]:\n    return a[1:2]\n",
        disposition: Disposition::Refuse {
            reason: "not yet supported in the Lean lane",
        },
    },
    Case {
        name: "list_sum",
        python: "def f(a: list[int]) -> int:\n    return sum(a)\n",
        disposition: Disposition::Refuse {
            reason: "not yet supported in the Lean lane",
        },
    },
    Case {
        name: "list_contains",
        python: "def f(a: list[int]) -> bool:\n    return 3 in a\n",
        disposition: Disposition::Refuse {
            reason: "list membership",
        },
    },
    Case {
        name: "str_index",
        python: "def f(a: str) -> str:\n    return a[0]\n",
        disposition: Disposition::Refuse {
            // Refused UPSTREAM of `Expr::Index` (it lowers to `StrCharAt`), which
            // is why the clamping defect never reached strings.
            reason: "string indexing `s[i]`",
        },
    },
    Case {
        name: "str_upper",
        python: "def f(a: str) -> str:\n    return a.upper()\n",
        disposition: Disposition::Refuse {
            reason: "Python string methods",
        },
    },
    Case {
        name: "str_of_int",
        python: "def f(a: int) -> str:\n    return str(a)\n",
        disposition: Disposition::Refuse {
            reason: "not yet supported in the Lean lane",
        },
    },
    Case {
        name: "dict_read",
        python: "def f(d: dict[str, int]) -> int:\n    return d[\"a\"]\n",
        disposition: Disposition::Refuse {
            // The other `Expr::Index`-shaped read. It refuses upstream, so the
            // fix above is list-only by construction — recorded, not assumed.
            reason: "require the Std.HashMap Lean encoding",
        },
    },
    Case {
        name: "tuple_index",
        python: "def f(t: tuple[int, int]) -> int:\n    return t[0]\n",
        disposition: Disposition::Refuse {
            reason: "Python tuples",
        },
    },
    Case {
        name: "float_sqrt",
        python: "import math\ndef f(a: float) -> float:\n    return math.sqrt(a)\n",
        disposition: Disposition::Refuse {
            reason: "not yet supported in the Lean lane",
        },
    },
    Case {
        name: "float_to_int",
        python: "def f(a: float) -> int:\n    return int(a)\n",
        disposition: Disposition::Refuse {
            reason: "not yet supported in the Lean lane",
        },
    },
];

/// Emit Lean for `python` with the CLI's DEFAULT flags.
fn emit(dir: &std::path::Path, python: &str) -> Result<String, String> {
    let src = dir.join("src.py");
    std::fs::write(&src, python).expect("write py");
    let out = Command::new(xpile_bin())
        .args(["transpile", src.to_str().unwrap(), "--target", "lean"])
        .output()
        .expect("spawn xpile");
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

fn run_lean(file: &std::path::Path) -> Result<(), String> {
    let out = Command::new("lean")
        .arg(file)
        .output()
        .map_err(|e| format!("spawn lean: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

/// Assertions 1, 2 and 4.
#[test]
fn lean_float_str_list_vocabulary_elaborates_and_agrees_with_cpython() {
    if !lean_present() {
        eprintln!(
            "SKIP lean_float_str_list_vocabulary_elaborates_and_agrees_with_cpython: \
             `lean` not on PATH (the hosted workspace-test runner has no Lean \
             toolchain). The refusal half runs without a toolchain in \
             `lean_vocabulary_refusals_do_not_need_a_toolchain`, and the \
             index-resolution half in \
             `lean_list_index_emission_resolves_negative_subscripts`."
        );
        return;
    }

    let mut accepted = 0usize;
    let mut obligations_run = 0usize;

    for case in CORPUS {
        let Disposition::Accept { obligations } = &case.disposition else {
            continue;
        };
        let dir = probe_dir(case.name);
        let emitted = match emit(&dir, case.python) {
            Ok(e) => e,
            Err(diag) => panic!(
                "PMAT-1426 assertion 4: `{}` must still lower — refusing it is \
                 over-refusal, the natural failure mode of a semantics fix \
                 (PMAT-1419).\n--- python ---\n{}\n--- xpile said ---\n{}",
                case.name, case.python, diag
            ),
        };
        accepted += 1;

        let mut text = emitted.clone();
        for o in *obligations {
            text.push('\n');
            text.push_str(o);
            obligations_run += 1;
        }
        text.push('\n');
        let file = dir.join("probe.lean");
        std::fs::write(&file, &text).expect("write lean");

        if let Err(why) = run_lean(&file) {
            panic!(
                "PMAT-1426 assertion 1/2: `{}` was ACCEPTED by --target lean (exit \
                 0) but `lean` rejects the emission, or its value DISAGREES with \
                 CPython. Elaboration alone never caught this class — every row \
                 here elaborated before the fix too; it is the value obligation \
                 that fails.\n--- python ---\n{}\n--- emitted lean (+ obligations) \
                 ---\n{}\n--- lean said ---\n{}",
                case.name, case.python, text, why
            );
        }
    }

    // Assertion 5 — anti-vacuity on the accept side.
    assert!(
        accepted >= 20,
        "PMAT-1426 assertion 5: only {accepted} sources were accepted; the corpus \
         must keep exercising the float/string/list surface, not shrink to the one \
         fixed arm"
    );
    assert!(
        obligations_run >= 30,
        "PMAT-1426 assertion 5: only {obligations_run} value obligations ran. \
         ACCEPT ⟹ ELABORATES passed on this defect for the whole 0.1.x line; the \
         VALUE half is the load-bearing one and must stay populated"
    );
}

/// Assertion 3, runnable WITHOUT a Lean toolchain.
#[test]
fn lean_vocabulary_refusals_do_not_need_a_toolchain() {
    let mut refused = 0usize;
    for case in CORPUS {
        let Disposition::Refuse { reason } = &case.disposition else {
            continue;
        };
        let dir = probe_dir(&format!("refuse-{}", case.name));
        match emit(&dir, case.python) {
            Err(diag) => {
                refused += 1;
                assert!(
                    diag.contains(reason),
                    "PMAT-1426 assertion 3: `{}` refused, but NOT for the expected \
                     reason. A refusal firing for an unrelated cause satisfies a \
                     bare `is_err()` vacuously, so the cause is keyed per \
                     source.\n  expected substring: {reason}\n--- xpile said \
                     ---\n{}",
                    case.name,
                    diag
                );
            }
            Ok(emitted) => panic!(
                "PMAT-1426 assertion 3: `{}` was ACCEPTED but the corpus records it \
                 as refusing ({reason}). If the lane genuinely gained this \
                 construct, move the row to `Accept` WITH value obligations \
                 re-derived from CPython — do not delete it.\n--- emitted ---\n{}",
                case.name, emitted
            ),
        }
    }
    assert!(
        refused >= 8,
        "PMAT-1426 assertion 5: only {refused} sources were refused; the corpus must \
         keep bounding the vocabulary"
    );
}

/// The structural half of the fix, also toolchain-free: a list subscript must
/// RESOLVE the index Python-style before use. Stated as the absence of the
/// exact defective spelling plus the presence of the resolution, because the
/// executed half above cannot run on the hosted runner.
#[test]
fn lean_list_index_emission_resolves_negative_subscripts() {
    for name in [
        "index_negative_literal",
        "index_runtime",
        "index_zero_literal",
    ] {
        let case = CORPUS.iter().find(|c| c.name == name).expect("corpus row");
        let dir = probe_dir(&format!("shape-{name}"));
        let emitted = emit(&dir, case.python).expect("must lower");

        assert!(
            emitted.contains("if __xi < 0 then ((__xc).length : Int) + __xi else __xi"),
            "PMAT-1426: `{name}` no longer resolves a negative subscript against \
             the list length. Python reads `a[-1]` from the END; `Int.toNat` \
             clamps it to 0.\n--- emitted ---\n{emitted}"
        );
        assert!(
            emitted.contains("xpile: IndexError: list index out of range"),
            "PMAT-1426: `{name}` lost the still-negative guard. Without it \
             `[10,20,30][-5]` clamps back into range and answers 10 where CPython \
             raises IndexError.\n--- emitted ---\n{emitted}"
        );
        // The defective spelling, keyed exactly: a `.toNat]!` applied to the
        // SOURCE index rather than to the resolved one.
        assert!(
            !emitted.contains("[(-1: Int).toNat]!") && !emitted.contains("[i.toNat]!"),
            "PMAT-1426: `{name}` re-emitted the bare clamping subscript. This is \
             the v0.1.617 defect verbatim.\n--- emitted ---\n{emitted}"
        );
    }
}

/// The PREMISE the guard rests on, MEASURED against the toolchain rather than
/// restated from a source comment (PMAT-1425 lesson 3): `Int.toNat` clamps a
/// negative to `0`, which is exactly why a bare `xs[i.toNat]!` was wrong. If a
/// future Lean changes this, the guard's justification has expired and this
/// reds — the signal to re-derive the emission, not to leave a rationale
/// standing on a premise that no longer holds (PMAT-1421).
#[test]
fn int_tonat_still_clamps_negatives_to_zero() {
    if !lean_present() {
        eprintln!("SKIP int_tonat_still_clamps_negatives_to_zero: `lean` not on PATH");
        return;
    }
    let dir = probe_dir("premise");

    let f = dir.join("clamp.lean");
    std::fs::write(
        &f,
        "example : (-1 : Int).toNat = 0 := by decide\n\
         example : (-5 : Int).toNat = 0 := by decide\n\
         -- and the shape it produced: the FIRST element, not the last.\n\
         example : [10, 20, 30][(-1 : Int).toNat]! = 10 := by decide\n",
    )
    .expect("write");
    run_lean(&f).expect(
        "PMAT-1426 premise: `Int.toNat` must still clamp a negative to 0, and a \
         bare `xs[(-1).toNat]!` must still select element 0. If this fails the \
         toolchain has changed under the fix — re-derive the emission.",
    );

    // Positive control: a broken `run_lean` (missing binary, bad path) would
    // make the assertion above pass for free if it were phrased as `is_err()`,
    // and would fail here loudly instead.
    let g = dir.join("control.lean");
    std::fs::write(&g, "example : (3 : Int).toNat = 3 := by decide\n").expect("write");
    run_lean(&g).expect(
        "PMAT-1426 positive control: `(3 : Int).toNat = 3` must elaborate. If this \
         fails, `run_lean` itself is broken and the premise above proved nothing.",
    );
}
