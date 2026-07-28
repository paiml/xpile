//! XPILE-WITNESS (Lean lane) — PMAT-1425: every integer BINARY OPERATOR the
//! `--target lean` lane ACCEPTS emits Lean that `lean` can actually elaborate,
//! and the operators it cannot spell faithfully REFUSE instead of emitting.
//!
//! WHAT WAS WRONG. Six `emit_binop` arms covered the bitwise/shift/pow family.
//! FOUR of them named constants and instances that do not exist in Lean 4 core,
//! so `xpile transpile x.py --target lean` exited **0** writing a file `lean`
//! then rejected outright. MEASURED against lean 4.15.0:
//!
//! ```text
//!   a & b   -> (Int.land a b)      lean: unknown constant 'Int.land'
//!   a | b   -> (Int.lor a b)       lean: unknown constant 'Int.lor'
//!   a ^ b   -> (Int.xor a b)       lean: unknown constant 'Int.xor'
//!   a << b  -> (a <<< b.toNat)     lean: failed to synthesize HShiftLeft Int Nat
//! ```
//!
//! The remaining two DID elaborate and were SILENTLY WRONG at the edge, because
//! `Int.toNat` clamps a negative to `0` instead of failing:
//!
//! ```text
//!   1024 >> -1    lean: 1024   CPython: ValueError: negative shift count
//!      2 ** -1    lean:    1   CPython: 0.5
//! ```
//!
//! Every emission carried a `/-- xpile-contract: C-PY-INT-ARITH -/` docstring
//! claiming the construct was covered by a machine-checked contract — and the
//! three other integer lanes REPORT this same input: Rust and Ruchy emit
//! `panic!("xpile: negative shift amount (Python ValueError: negative shift
//! count; contract C-PY-INT-ARITH)")`, and WASM traps in `$__wasm_shr_i64`
//! (PMAT-1379). Only Lean — the lane whose whole purpose is machine-checked
//! semantics — answered `1024` at exit 0, citing the contract the other three
//! cite in their panic text.
//!
//! WHY IT SURVIVED. No witness ever drove these six operators. The Lean lane's
//! two oracles (`lean_elaborate_witness.rs`, `lean_default_emit_witness.rs`)
//! carry corpora of arithmetic, comparison and bool constructs only, so the
//! bitwise/shift/pow VOCABULARY was never elaborated once in the 0.1.x line —
//! and a source comment asserted the four constants existed ("Lean 4 core
//! provides Int.land / Int.lor / Int.xor …") without ever having been checked
//! against a toolchain.
//!
//! WHAT THIS ASSERTS. The invariant is deliberately stated over the OPERATOR
//! VOCABULARY rather than over the six known-bad arms, because the defect was
//! that a whole vocabulary went unexercised — a gate listing exactly the arms
//! already fixed would not have caught this one and will not catch the next:
//!
//!   1. ACCEPT ⟹ ELABORATES. For every corpus source `xpile` accepts, `lean`
//!      must accept the emitted file. This is the general property; all four
//!      `unknown constant` defects are instances of it.
//!   2. REFUSE ⟹ FOR THE STATED REASON. Each refusal is keyed to its own
//!      source's expected cause, never a bare `is_err()` — `a ** -2` already
//!      refused UPSTREAM (the frontend types it Float), so an unkeyed red half
//!      would pass vacuously on it.
//!   3. PRESERVED CAPABILITY. `a >> 3`, `a >> 0` and `a ** 2` must still lower,
//!      elaborate, AND compute CPython's value (pinned via `by decide`). Without
//!      this, refusing the whole family passes 1 and 2 — over-refusal is the
//!      natural failure mode of a refusal fix (PMAT-1419).
//!   4. ANTI-VACUITY. The corpus must contain a non-trivial number of BOTH
//!      dispositions, so neither a blanket refusal nor a blanket acceptance can
//!      satisfy this file.
//!
//! Skips with reason when `lean` / the xpile bin is absent (the hosted
//! workspace-test runner has no Lean toolchain) — never silently green.

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
        .join("xpile-lean-bitwise-shift-witness")
        .join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir probe dir");
    dir
}

/// What the lane is expected to do with a source.
enum Disposition {
    /// Lowers. The emitted Lean must elaborate; `by decide` obligations pin the
    /// value against CPython where given.
    Accept {
        obligations: &'static [&'static str],
    },
    /// Refuses. `reason` must appear in the diagnostic — keyed per source so a
    /// refusal that fires for an unrelated cause cannot satisfy the assertion.
    Refuse { reason: &'static str },
}

struct Case {
    name: &'static str,
    python: &'static str,
    disposition: Disposition,
}

/// The integer binary-operator vocabulary of the Lean lane. Arithmetic and
/// comparison operators are covered by `lean_elaborate_witness.rs`; this corpus
/// is the bitwise / shift / pow family that had no coverage at all, plus the
/// `//` and `%` rows whose EXISTING refusal boundary this fix mirrors (they are
/// here so that boundary and this one cannot drift apart unnoticed).
const CORPUS: &[Case] = &[
    // ---- no Lean 4 core spelling: were exit-0 uncompilable ----
    Case {
        name: "bitand",
        python: "def f(a: int, b: int) -> int:\n    return a & b\n",
        disposition: Disposition::Refuse {
            reason: "has no Lean 4 core spelling",
        },
    },
    Case {
        name: "bitor",
        python: "def f(a: int, b: int) -> int:\n    return a | b\n",
        disposition: Disposition::Refuse {
            reason: "has no Lean 4 core spelling",
        },
    },
    Case {
        name: "bitxor",
        python: "def f(a: int, b: int) -> int:\n    return a ^ b\n",
        disposition: Disposition::Refuse {
            reason: "has no Lean 4 core spelling",
        },
    },
    Case {
        name: "shl_runtime",
        python: "def f(a: int, b: int) -> int:\n    return a << b\n",
        disposition: Disposition::Refuse {
            reason: "has no Lean 4 core spelling",
        },
    },
    // `<<` has no spelling regardless of the count being a fine literal — the
    // refusal is about the OPERATOR, not the operand, and saying so keeps the
    // two refusal kinds from being conflated.
    Case {
        name: "shl_literal_count",
        python: "def f(a: int) -> int:\n    return a << 3\n",
        disposition: Disposition::Refuse {
            reason: "has no Lean 4 core spelling",
        },
    },
    // ---- faithful only for a non-negative count: were silently wrong ----
    Case {
        name: "shr_runtime",
        python: "def f(a: int, b: int) -> int:\n    return a >> b\n",
        disposition: Disposition::Refuse {
            reason: "not a provably-non-negative literal",
        },
    },
    Case {
        name: "shr_negative_literal",
        python: "def f(a: int) -> int:\n    return a >> -1\n",
        disposition: Disposition::Refuse {
            reason: "is a negative literal",
        },
    },
    Case {
        name: "pow_runtime",
        python: "def f(a: int, b: int) -> int:\n    return a ** b\n",
        disposition: Disposition::Refuse {
            reason: "not a provably-non-negative literal",
        },
    },
    // ---- the EXISTING zero-divisor boundary this one mirrors ----
    Case {
        name: "floordiv_runtime",
        python: "def f(a: int, b: int) -> int:\n    return a // b\n",
        disposition: Disposition::Refuse {
            reason: "not a provably-nonzero literal",
        },
    },
    Case {
        name: "mod_runtime",
        python: "def f(a: int, b: int) -> int:\n    return a % b\n",
        disposition: Disposition::Refuse {
            reason: "not a provably-nonzero literal",
        },
    },
    // ---- PRESERVED: must still lower, elaborate, and match CPython ----
    // Values re-derived from CPython 3, including the operands most likely to
    // diverge (negative base, oversized count, `0 ** 0`, arbitrary precision).
    Case {
        name: "shr_literal_count",
        python: "def f(a: int) -> int:\n    return a >> 3\n",
        disposition: Disposition::Accept {
            // CPython: 1024 >> 3 == 128 ; -5 >> 3 == -1 ; 1024 >> 3 with a
            // count past the width is covered by `shr_oversized_count` below.
            obligations: &[
                "example : f 1024 = 128 := by decide",
                "example : f (-5) = -1 := by decide",
            ],
        },
    },
    Case {
        name: "shr_zero_count",
        python: "def f(a: int) -> int:\n    return a >> 0\n",
        disposition: Disposition::Accept {
            obligations: &[
                "example : f 7 = 7 := by decide",
                "example : f (-7) = -7 := by decide",
            ],
        },
    },
    Case {
        name: "shr_oversized_count",
        python: "def f(a: int) -> int:\n    return a >> 70\n",
        disposition: Disposition::Accept {
            // CPython: 1024 >> 70 == 0 ; -1 >> 70 == -1 (arithmetic, sign-preserving).
            obligations: &[
                "example : f 1024 = 0 := by decide",
                "example : f (-1) = -1 := by decide",
            ],
        },
    },
    Case {
        name: "pow_literal_exponent",
        python: "def f(a: int) -> int:\n    return a ** 2\n",
        disposition: Disposition::Accept {
            obligations: &[
                "example : f 7 = 49 := by decide",
                "example : f (-7) = 49 := by decide",
            ],
        },
    },
    Case {
        name: "pow_zero_exponent",
        python: "def f(a: int) -> int:\n    return a ** 0\n",
        disposition: Disposition::Accept {
            // CPython: 0 ** 0 == 1.
            obligations: &[
                "example : f 0 = 1 := by decide",
                "example : f 5 = 1 := by decide",
            ],
        },
    },
    Case {
        name: "pow_bigint_result",
        python: "def f(a: int) -> int:\n    return a ** 70\n",
        disposition: Disposition::Accept {
            // CPython: 2 ** 70 == 1180591620717411303424 — past i64, and Lean's
            // `Int` is arbitrary-precision like Python's, so this must hold.
            obligations: &["example : f 2 = 1180591620717411303424 := by decide"],
        },
    },
];

/// Emit Lean for `python` with the CLI's DEFAULT flags, returning
/// `Ok(emitted)` or `Err(diagnostic)`.
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

/// Assertion 1 + 3: everything the lane ACCEPTS elaborates, and computes what
/// CPython computes. Assertion 2: everything it REFUSES does so for its own
/// stated reason.
#[test]
fn lean_binop_vocabulary_accepts_only_what_lean_elaborates() {
    if !lean_present() {
        eprintln!(
            "SKIP lean_binop_vocabulary_accepts_only_what_lean_elaborates: \
             `lean` not on PATH (hosted workspace-test runner has no Lean \
             toolchain). The refusal half is covered without a toolchain by \
             `lean_binop_refusals_do_not_need_a_toolchain`."
        );
        return;
    }

    let mut accepted = 0usize;
    let mut refused = 0usize;

    for case in CORPUS {
        let dir = probe_dir(case.name);
        let result = emit(&dir, case.python);

        match (&case.disposition, result) {
            (Disposition::Accept { obligations }, Ok(emitted)) => {
                accepted += 1;
                let file = dir.join("probe.lean");
                let mut text = emitted.clone();
                for o in *obligations {
                    text.push('\n');
                    text.push_str(o);
                }
                text.push('\n');
                std::fs::write(&file, &text).expect("write lean");
                if let Err(why) = run_lean(&file) {
                    panic!(
                        "PMAT-1425 assertion 1/3: `{}` was ACCEPTED by --target lean \
                         (exit 0) but `lean` rejects the emission, or its value \
                         disagrees with CPython.\n--- python ---\n{}\n--- emitted \
                         lean (+ obligations) ---\n{}\n--- lean said ---\n{}",
                        case.name, case.python, text, why
                    );
                }
            }
            (Disposition::Accept { .. }, Err(diag)) => panic!(
                "PMAT-1425 assertion 3: `{}` must still lower — refusing it is \
                 over-refusal, the natural failure mode of this fix.\n--- python \
                 ---\n{}\n--- xpile said ---\n{}",
                case.name, case.python, diag
            ),
            (Disposition::Refuse { reason }, Err(diag)) => {
                refused += 1;
                assert!(
                    diag.contains(reason),
                    "PMAT-1425 assertion 2: `{}` refused, but NOT for the expected \
                     reason. A refusal that fires for an unrelated cause satisfies \
                     a bare `is_err()` vacuously, so the cause is keyed per \
                     source.\n  expected substring: {reason}\n--- xpile said ---\n{}",
                    case.name,
                    diag
                );
            }
            (Disposition::Refuse { reason }, Ok(emitted)) => panic!(
                "PMAT-1425 assertion 2: `{}` was ACCEPTED but must refuse \
                 ({reason}).\n--- python ---\n{}\n--- emitted lean ---\n{}",
                case.name, case.python, emitted
            ),
        }
    }

    // Assertion 4 — anti-vacuity. Neither a blanket refusal nor a blanket
    // acceptance may satisfy this file.
    assert!(
        accepted >= 6,
        "PMAT-1425 assertion 4: only {accepted} sources were accepted; a fix that \
         refuses the whole vocabulary would satisfy assertions 1-2 trivially"
    );
    assert!(
        refused >= 8,
        "PMAT-1425 assertion 4: only {refused} sources were refused; the corpus \
         must keep exercising the refusal boundary"
    );
}

/// The refusal half, runnable WITHOUT a Lean toolchain — so the hosted
/// workspace-test runner (which has no `lean`) still holds the four
/// exit-0-uncompilable arms shut rather than skipping the whole file.
#[test]
fn lean_binop_refusals_do_not_need_a_toolchain() {
    let mut refused = 0usize;
    for case in CORPUS {
        let Disposition::Refuse { reason } = &case.disposition else {
            continue;
        };
        let dir = probe_dir(&format!("norefuse-{}", case.name));
        match emit(&dir, case.python) {
            Err(diag) => {
                refused += 1;
                assert!(
                    diag.contains(reason),
                    "PMAT-1425: `{}` refused for the wrong reason.\n  expected: \
                     {reason}\n--- xpile said ---\n{}",
                    case.name,
                    diag
                );
            }
            Ok(emitted) => panic!(
                "PMAT-1425: `{}` must refuse ({reason}) — through v0.1.617 it \
                 exited 0 emitting Lean that `lean` cannot elaborate.\n--- emitted \
                 ---\n{}",
                case.name, emitted
            ),
        }
    }
    assert!(
        refused >= 8,
        "PMAT-1425: expected the refusal corpus to be exercised, saw {refused}"
    );
}

/// The premise the fixed code rests on, MEASURED rather than restated: of the
/// spellings the emitter reached for, `Int.shiftRight` (`>>>`) is the ONLY one
/// Lean 4 core actually provides. If a future toolchain adds `Int.land` and
/// friends, this test reds — which is the signal to RE-ENABLE those arms rather
/// than leave a refusal standing on a premise that has expired (PMAT-1421: an
/// inverse that outlives its forward map).
#[test]
fn lean_core_still_lacks_the_int_bitwise_spellings() {
    if !lean_present() {
        eprintln!("SKIP lean_core_still_lacks_the_int_bitwise_spellings: `lean` not on PATH");
        return;
    }
    let dir = probe_dir("core-spellings");

    let absent = [
        "#check @Int.land",
        "#check @Int.lor",
        "#check @Int.xor",
        "#check @Int.shiftLeft",
        "#check (1:Int) <<< (2:Nat)",
        "#check (1:Int) &&& (2:Int)",
    ];
    for (i, probe) in absent.iter().enumerate() {
        let f = dir.join(format!("absent{i}.lean"));
        std::fs::write(&f, format!("{probe}\n")).expect("write");
        assert!(
            run_lean(&f).is_err(),
            "PMAT-1425: `{probe}` now ELABORATES. The refusal in `no_lean_spelling` \
             was measured against a toolchain that lacked it; re-enable the \
             corresponding `emit_binop` arm instead of keeping a refusal whose \
             premise has expired."
        );
    }

    // The positive control: the one spelling that DOES exist, and the reason
    // `>>` survives the fix. Without this, a broken `run_lean` (a missing
    // binary, a bad path) would make every assertion above pass for free.
    let f = dir.join("present.lean");
    std::fs::write(&f, "#check @Int.shiftRight\n#check (1:Int) >>> (2:Nat)\n").expect("write");
    run_lean(&f).expect(
        "PMAT-1425 positive control: `Int.shiftRight` / `>>>` must elaborate. If \
         this fails, `run_lean` itself is broken and the absence assertions above \
         proved nothing.",
    );
}
