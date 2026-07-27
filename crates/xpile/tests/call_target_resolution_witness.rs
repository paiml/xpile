//! PMAT-1410 (XPILE-CALLTARGET-001): the two verified exit-0-uncompilable
//! Python shapes whose common cause is a CALL TARGET that does not resolve in
//! the emitted Rust.
//!
//! Both reproduce from two lines of Python and both directly refute the
//! README's central promise, and through v0.1.617 both exited 0:
//!
//!  * **A parameter used in call position.** An unannotated parameter defaults
//!    to `Type::I64`, so `def apply(f, x: int): return f(x)` emitted
//!    `pub fn apply(f: i64, x: i64) -> i64 { f(x) }` — `rustc` rejects it with
//!    `error[E0618]: expected function, found `i64``. The refusal for RETURNING
//!    a callable existed; the mirror-image refusal for RECEIVING one did not.
//!    Measured on the pre-fix binary: the ANNOTATED spelling `f: int` has the
//!    identical defect, so the refusal keys on the call, not on the annotation.
//!
//!  * **A `from`-import that binds a bare name.** Every `ImportFrom` was
//!    silently DROPPED, so `from pkg.util import double` + `double(3)` emitted
//!    `double(3i64)` against nothing — `error[E0425]: cannot find function
//!    `double` in this scope`. This was NOT limited to third-party modules:
//!    `from math import sqrt` + `sqrt(x)` was E0425 too, while `import math` +
//!    `math.sqrt(x)` compiled.
//!
//! The load-bearing test is [`transpiled_rust_either_refuses_or_rustc_accepts_it`]:
//! it asserts the PROPERTY `Ok(rust) ==> rustc accepts it` over BOTH corpora,
//! rather than pinning one message. A per-shape refusal assertion cannot catch
//! the NEXT shape that leaks; the property can.
//!
//! A refusal is invisible to a compile-only property — it satisfies
//! `Ok(rust) ==> …` vacuously — so
//! [`refused_shapes_are_valid_python_that_cpython_runs`] EXECUTES the refused
//! corpus under CPython. Without it, a probe that is merely bad Python would
//! make every refusal look justified for the wrong reason.
//!
//! Gated on `python3` + `rustc` presence (skips with a reason, like the oracle).

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn rustc_present() -> bool {
    Command::new("rustc")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn python3_present() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A per-CALL unique scratch directory. Keying it on (tag, pid) is NOT enough:
/// `transpile` and `rustc_accepts` are separate calls for the same tag, and the
/// tests run on parallel threads — a shared directory gets wiped mid-compile
/// and `rustc` fails to LINK its own object files, which reads exactly like an
/// emitter defect. The atomic counter is what makes each call disjoint.
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join("xpile-call-target").join(format!(
        "{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// `Ok(rust_source)` when the frontend accepts, `Err(stderr)` when it refuses.
///
/// Deliberately invokes the DEFAULT flag set — no `--contracts off`, no
/// opt-in. PMAT-1405 shipped a broken default because the lane's own witness
/// exercised a non-default flag.
fn transpile(src: &str, tag: &str) -> Result<String, String> {
    let dir = scratch(tag);
    let py = dir.join("p.py");
    std::fs::write(&py, src).expect("write probe");
    let out = Command::new(xpile_bin())
        .args(["transpile", py.to_str().unwrap(), "--target", "rust"])
        .output()
        .expect("spawn xpile");
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// `Ok(binary)` when `rustc` accepts the emitted Rust, `Err(stderr)` otherwise.
fn rustc_accepts(rust: &str, tag: &str) -> Result<PathBuf, String> {
    let dir = scratch(tag);
    let rs = dir.join("p.rs");
    std::fs::write(&rs, rust).expect("write rust");
    let bin = dir.join("p");
    let out = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-O")
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

/// Run `src` under CPython with a trailing `main()`.
fn run_python(src: &str) -> Result<String, String> {
    let out = Command::new("python3")
        .arg("-c")
        .arg(format!("{src}\nmain()\n"))
        .output()
        .expect("spawn python3");
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

// ---------------------------------------------------------------------------
// The corpora. Every entry is a self-contained module defining `main()`.
// ---------------------------------------------------------------------------

/// Shapes that MUST refuse with the PARAMETER-in-call-position message, and
/// that CPython runs successfully — so the refusal is demonstrably about
/// xpile's limits, not about a malformed probe. All five were measured
/// exit-0-then-`error[E0618]` on the pre-fix binary.
///
/// There is deliberately no positional-only (`f, x, /`) or keyword-only
/// (`*, f, x`) entry: both parameter kinds refuse EARLIER, with their own
/// "not supported at v0.1.0" messages, so a probe using them would pass this
/// corpus without ever reaching the new check. `check_fn` still scans
/// `posonlyargs`/`kwonlyargs` so the check is correct if those land, but that
/// arm is unreachable today and is not claimed as covered.
const REFUSED_PARAM_CALL: &[(&str, &str)] = &[
    (
        "unannotated_param_called",
        "def apply(f, x: int) -> int:\n    return f(x)\n\ndef inc(n: int) -> int:\n    return n + 1\n\ndef main() -> None:\n    print(apply(inc, 4))\n",
    ),
    // The ANNOTATED spelling has the identical defect: `f: int` is not callable
    // either. Measured exit-0-uncompilable on the pre-fix binary.
    (
        "annotated_int_param_called",
        "def apply(f: int, x: int) -> int:\n    return f(x)\n\ndef inc(n: int) -> int:\n    return n + 1\n\ndef main() -> None:\n    print(apply(inc, 4))\n",
    ),
    (
        "param_called_inside_a_binop",
        "def apply(f, x: int) -> int:\n    return f(x) + 1\n\ndef inc(n: int) -> int:\n    return n + 1\n\ndef main() -> None:\n    print(apply(inc, 4))\n",
    ),
    (
        "param_called_in_a_for_body",
        "def apply_all(f, xs: list[int]) -> int:\n    total: int = 0\n    for v in xs:\n        total = total + f(v)\n    return total\n\ndef inc(n: int) -> int:\n    return n + 1\n\ndef main() -> None:\n    print(apply_all(inc, [1, 2, 3]))\n",
    ),
    // A METHOD's parameter, i.e. the walk descends through `ClassDef`.
    (
        "param_called_in_a_method",
        "class R:\n    def run(self, f, x: int) -> int:\n        return f(x)\n\ndef inc(n: int) -> int:\n    return n + 1\n\ndef main() -> None:\n    print(R().run(inc, 4))\n",
    ),
];

/// Shapes that MUST refuse with the `from`-import message, and that CPython
/// runs successfully.
///
/// Pre-fix classification, measured — NOT uniform, and stated rather than
/// averaged: `from_math_import_sqrt` exited 0 into `error[E0425]`, while
/// `from_collections_import_deque` and `from_os_import_getcwd` exited 0 and
/// COMPILED, because their imported name is never used. Those two are a real
/// behaviour change: sources that used to compile now refuse. That is the
/// intended trade — the import was silently dropped either way, and any USE of
/// the name was E0425 — but it is a change, not a pure defect fix.
const REFUSED_IMPORT: &[(&str, &str)] = &[
    // STDLIB, and still broken: the qualified form is the modelled one.
    (
        "from_math_import_sqrt",
        "from math import sqrt\n\ndef main() -> None:\n    print(sqrt(16.0))\n",
    ),
    (
        "from_collections_import_deque",
        "from collections import deque\n\ndef main() -> None:\n    print(1)\n",
    ),
    (
        "from_os_import_getcwd",
        "from os import getcwd\n\ndef main() -> None:\n    print(1)\n",
    ),
];

/// The queue's literal shape. Held apart from the corpora above because
/// CPython cannot run it either — there is no `pkg` module — so it belongs in
/// the property corpus but not in the CPython-executes check. Its own test
/// below pins that distinction rather than hiding it.
const REFUSED_ABSENT_MODULE: &[(&str, &str)] = &[(
    "from_pkg_util_import_double",
    "from pkg.util import double\n\ndef main() -> None:\n    print(double(3))\n",
)];

/// Shapes that MUST keep working. A refusal that over-fires would take these
/// with it, so every one is COMPILED and EXECUTED against CPython below.
const STILL_ACCEPTED: &[(&str, &str)] = &[
    // The qualified stdlib form that `from math import sqrt` must point at.
    (
        "math_qualified_attribute_call",
        "import math\n\ndef main() -> None:\n    print(math.sqrt(16.0))\n",
    ),
    // The annotation-only modules on the allowlist.
    (
        "from_typing_import_list",
        "from typing import List\n\ndef total(xs: List[int]) -> int:\n    s: int = 0\n    for x in xs:\n        s = s + x\n    return s\n\ndef main() -> None:\n    print(total([1, 2, 3]))\n",
    ),
    (
        "from_typing_import_optional",
        "from typing import Optional\n\ndef f(x: Optional[int]) -> int:\n    if x is None:\n        return 0\n    return x\n\ndef main() -> None:\n    print(f(3))\n",
    ),
    (
        "from_dataclasses_import_dataclass",
        "from dataclasses import dataclass\n\n@dataclass\nclass P:\n    x: int\n\ndef main() -> None:\n    print(P(1).x)\n",
    ),
    (
        "from_enum_import_enum",
        "from enum import Enum\n\nclass Color(Enum):\n    RED = 1\n    BLUE = 2\n\ndef main() -> None:\n    print(Color.RED.value)\n",
    ),
    (
        "from_future_import_annotations",
        "from __future__ import annotations\n\ndef f(x: int) -> int:\n    return x + 1\n\ndef main() -> None:\n    print(f(4))\n",
    ),
    // A parameter that is merely READ, not called.
    (
        "param_read_but_not_called",
        "def f(g: int, x: int) -> int:\n    return g + x\n\ndef main() -> None:\n    print(f(1, 2))\n",
    ),
    // A module-level function called by name — the shape the refusal must not
    // be confused by.
    (
        "module_level_function_call",
        "def helper(x: int) -> int:\n    return x + 1\n\ndef main() -> None:\n    print(helper(4))\n",
    ),
    (
        "builtin_len_call",
        "def f(xs: list[int]) -> int:\n    return len(xs)\n\ndef main() -> None:\n    print(f([1, 2, 3]))\n",
    ),
    (
        "self_method_call",
        "class C:\n    def __init__(self, x: int) -> None:\n        self.x = x\n    def get(self) -> int:\n        return self.x\n\ndef main() -> None:\n    print(C(3).get())\n",
    ),
    // PMAT-770's callable-instance protocol: a parameter annotated with a user
    // class that registers `__call__` IS callable and lowers to that method.
    // This is the exemption the refusal must honour.
    (
        "callable_instance_param",
        "class Adder:\n    def __init__(self, n: int) -> None:\n        self.n = n\n    def __call__(self, x: int) -> int:\n        return self.n + x\n\ndef use(a: Adder, x: int) -> int:\n    return a(x)\n\ndef main() -> None:\n    print(use(Adder(2), 3))\n",
    ),
    // A nested `def` shadowing a same-named parameter: the call resolves to the
    // nested function, so the parameter is not what is being called.
    (
        "nested_def_shadows_the_param",
        "def f(g, x: int) -> int:\n    def g(y: int) -> int:\n        return y + 1\n    return g(x)\n\ndef main() -> None:\n    print(f(0, 4))\n",
    ),
];

/// Every refused source that is legal, runnable Python.
fn refused_valid_python() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    REFUSED_PARAM_CALL.iter().chain(REFUSED_IMPORT)
}

/// Every refused source, including the one CPython cannot run either.
fn all_refused() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    refused_valid_python().chain(REFUSED_ABSENT_MODULE)
}

// ---------------------------------------------------------------------------
// The load-bearing property.
// ---------------------------------------------------------------------------

/// XPILE-CALLTARGET-001. `Ok(rust) ==> rustc accepts it`, over BOTH corpora.
///
/// This is the assertion that generalises: it does not care WHICH shapes refuse
/// and which compile, only that the CLI never exits 0 on Rust the compiler
/// rejects. Through v0.1.617 every `REFUSED_*` source failed it.
#[test]
fn transpiled_rust_either_refuses_or_rustc_accepts_it() {
    if !rustc_present() {
        eprintln!("warning: rustc not on PATH; skipping XPILE-CALLTARGET-001 property");
        return;
    }
    let mut accept_then_fail: Vec<String> = Vec::new();
    let mut refused = 0usize;
    let mut compiled = 0usize;
    for (tag, src) in all_refused().chain(STILL_ACCEPTED.iter()) {
        match transpile(src, tag) {
            Err(_) => refused += 1,
            Ok(rust) => match rustc_accepts(&rust, tag) {
                Ok(_) => compiled += 1,
                Err(stderr) => accept_then_fail.push(format!(
                    "{tag}: xpile exited 0 but rustc REJECTED the emitted Rust:\n{}\n--- emitted ---\n{rust}",
                    stderr.trim()
                )),
            },
        }
    }
    let total = REFUSED_PARAM_CALL.len()
        + REFUSED_IMPORT.len()
        + REFUSED_ABSENT_MODULE.len()
        + STILL_ACCEPTED.len();
    eprintln!(
        "XPILE-CALLTARGET-001: {total} sources — {refused} refused, {compiled} compiled, {} accept-then-fail",
        accept_then_fail.len()
    );
    assert!(
        accept_then_fail.is_empty(),
        "{} source(s) transpiled to Rust that rustc rejects:\n\n{}",
        accept_then_fail.len(),
        accept_then_fail.join("\n\n")
    );
}

/// The property above is satisfied VACUOUSLY by a refusal, so this pins the
/// split explicitly: every `REFUSED_*` source refuses and every
/// `STILL_ACCEPTED` source does not. Without it, a change that refused the
/// whole language would keep the property green.
#[test]
fn the_refused_and_accepted_corpora_land_on_the_sides_they_claim() {
    for (tag, src) in all_refused() {
        assert!(
            transpile(src, tag).is_err(),
            "{tag}: must REFUSE, but xpile exited 0"
        );
    }
    for (tag, src) in STILL_ACCEPTED {
        assert!(
            transpile(src, tag).is_ok(),
            "{tag}: must still transpile, but xpile refused:\n{}",
            transpile(src, tag).unwrap_err()
        );
    }
}

/// Every refusal is a FRONTEND lowering refusal — not a parse failure, not a
/// backend refusal, not a panic — and names the construct plus its rustc
/// consequence. Pinning the STAGE matters: an opaque non-zero exit would
/// satisfy "exits non-zero" while telling the user nothing.
#[test]
fn each_refusal_names_the_construct_its_rustc_error_and_a_way_forward() {
    for (tag, src) in REFUSED_PARAM_CALL {
        let stderr = transpile(src, tag).expect_err(&format!("{tag} must refuse"));
        assert!(
            stderr.contains("lowering error"),
            "{tag}: refusal is not a frontend lowering refusal:\n{stderr}"
        );
        assert!(
            stderr.contains("calls its own parameter"),
            "{tag}: refusal does not name the construct:\n{stderr}"
        );
        assert!(
            stderr.contains("E0618"),
            "{tag}: refusal does not name the rustc error it prevents:\n{stderr}"
        );
        assert!(
            stderr.contains("PMAT-1410"),
            "{tag}: refusal is not attributable:\n{stderr}"
        );
    }
    for (tag, src) in REFUSED_IMPORT.iter().chain(REFUSED_ABSENT_MODULE) {
        let stderr = transpile(src, tag).expect_err(&format!("{tag} must refuse"));
        assert!(
            stderr.contains("lowering error"),
            "{tag}: refusal is not a frontend lowering refusal:\n{stderr}"
        );
        assert!(
            stderr.contains("xpile has no module system"),
            "{tag}: refusal does not name the construct:\n{stderr}"
        );
        assert!(
            stderr.contains("E0425"),
            "{tag}: refusal does not name the rustc error it prevents:\n{stderr}"
        );
        assert!(
            stderr.contains("import math") && stderr.contains("math.sqrt"),
            "{tag}: refusal does not point at the qualified form:\n{stderr}"
        );
        assert!(
            stderr.contains("PMAT-1410"),
            "{tag}: refusal is not attributable:\n{stderr}"
        );
    }
}

/// The non-vacuity half. A refusal satisfies the compile-only property for
/// free, so these sources are EXECUTED under CPython: each is legal Python that
/// produces output. That is what makes "xpile refuses it" a statement about
/// xpile's coverage rather than about a broken probe.
#[test]
fn refused_shapes_are_valid_python_that_cpython_runs() {
    if !python3_present() {
        eprintln!("warning: python3 not on PATH; skipping XPILE-CALLTARGET-001 CPython half");
        return;
    }
    for (tag, src) in refused_valid_python() {
        let out = run_python(src).unwrap_or_else(|e| {
            panic!("{tag}: the probe must be valid Python, but CPython failed:\n{e}")
        });
        assert!(
            !out.is_empty(),
            "{tag}: CPython produced no output — the probe does not exercise the shape"
        );
    }
}

/// `from pkg.util import double` is the queue's literal shape and the ONE
/// refused source CPython also rejects. Stated here rather than quietly
/// omitted: xpile refuses it for the module-system reason, CPython for the
/// module-does-not-exist reason, and they are not the same reason.
#[test]
fn the_absent_module_shape_is_refused_by_xpile_and_by_cpython_for_different_reasons() {
    let (tag, src) = REFUSED_ABSENT_MODULE[0];
    let stderr = transpile(src, tag).expect_err("xpile must refuse the absent-module shape");
    assert!(
        stderr.contains("xpile has no module system"),
        "xpile's reason should be the missing module system:\n{stderr}"
    );
    if !python3_present() {
        eprintln!(
            "warning: python3 not on PATH; skipping the CPython half of the absent-module pin"
        );
        return;
    }
    let py = run_python(src).expect_err("CPython has no `pkg` module either");
    assert!(
        py.contains("ModuleNotFoundError"),
        "CPython's reason should be the absent module, got:\n{py}"
    );
}

/// The non-regression half, EXECUTED: every `STILL_ACCEPTED` shape compiles and
/// its stdout matches CPython byte for byte. Pinning "it still transpiles"
/// alone would not catch a refusal that silently changed the emitted VALUE.
#[test]
fn still_accepted_shapes_execute_and_match_cpython() {
    if !rustc_present() || !python3_present() {
        eprintln!(
            "warning: rustc/python3 not on PATH; skipping XPILE-CALLTARGET-001 execution half"
        );
        return;
    }
    for (tag, src) in STILL_ACCEPTED {
        let rust =
            transpile(src, tag).unwrap_or_else(|e| panic!("{tag} must still transpile: {e}"));
        let bin = rustc_accepts(&rust, tag)
            .unwrap_or_else(|e| panic!("{tag}: rustc rejected the emitted Rust: {e}"));
        let actual = Command::new(&bin).output().expect("run probe binary");
        assert!(
            actual.status.success(),
            "{tag}: emitted binary exited {}: {}",
            actual.status,
            String::from_utf8_lossy(&actual.stderr)
        );
        let actual = String::from_utf8_lossy(&actual.stdout)
            .trim_end_matches('\n')
            .to_string();
        let expected = run_python(src).unwrap_or_else(|e| panic!("{tag}: CPython reference: {e}"));
        assert_eq!(actual, expected, "{tag}: diverges from CPython");
    }
}

/// The `__call__` exemption, isolated. `callable_instance_param` compiling is
/// only meaningful if the emitted Rust actually routes the call to the method —
/// a shape that compiled for some unrelated reason would pass vacuously.
#[test]
fn the_callable_instance_exemption_emits_a_call_method_invocation() {
    let src = STILL_ACCEPTED
        .iter()
        .find(|(t, _)| *t == "callable_instance_param")
        .expect("the callable-instance probe is in the corpus")
        .1;
    let rust = transpile(src, "callable_instance_emit").expect("the exemption must transpile");
    assert!(
        rust.contains("__call__(x)"),
        "the parameter call must lower to the `__call__` method, got:\n{rust}"
    );
}
