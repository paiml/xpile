//! PMAT-1363: an annotated comprehension whose element type contradicts its
//! annotation REFUSES at the frontend instead of emitting Rust that rustc
//! rejects.
//!
//! Before this slice, `xs: list[str] = [i for i in range(5)]` was ACCEPTED. The
//! annotation was stamped onto the binding while the comprehension lowered
//! independently, and nothing ever compared them, so the emitted Rust was
//!
//! ```text
//! let xs: Vec<String> = (0i64..5i64).collect::<Vec<i64>>()… .collect::<Vec<_>>();
//!                       ^^^ expected `Vec<String>`, found `Vec<i64>`  (E0308)
//! ```
//!
//! Accept-then-fail-rustc is the worst of the three dispositions: worse than
//! refusing (which costs the user one clear message) and worse than emitting
//! correct code, because it spends a whole backend round-trip to rediscover
//! what the frontend already knew. Six distinct shapes had it — list, set, and
//! dict comprehensions, the last in BOTH key and value position.
//!
//! Two halves, and the second is the load-bearing one:
//!
//!  * **RED** — each contradicting shape now returns a `FrontendError::Lower`
//!    (the STAGE is pinned, not just "some error") whose message names the
//!    position and both types.
//!  * **GREEN** — the check is deliberately conservative, and these tests are
//!    what "conservative" means operationally. `infer_type_in_ctx` answers
//!    `I64` for anything it cannot type, so a refusal resting on that default
//!    would falsely reject working programs. Every agreeing shape, and every
//!    shape with a non-scalar leaf, must still lower. The differential half of
//!    the same property lives in `tests/oracle_fixtures/ann_comp_types.py`,
//!    which additionally proves those shapes still MATCH CPython byte for byte.
//!
//! Note `xs: list[float] = [i for i in range(3)]` refuses rather than coercing.
//! CPython prints `1`, not `1.0` — a `float` annotation is non-enforcing, the
//! same fact PMAT-906 encodes for the scalar path — and the int→float
//! container-literal policy is an OPEN owner decision. A refusal is the
//! reversible disposition; coercing would pre-empt that decision.

use depyler_frontend::PythonFrontend;
use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};

fn lower(body: &str) -> Result<xpile_meta_hir::Module, FrontendError> {
    let src = format!("def main() -> None:\n{body}");
    PythonFrontend.parse_and_lower(Path::new("t.py"), &src)
}

/// The six shapes that used to emit E0308-failing Rust. Each is `(label,
/// python body, the annotated type as rendered, the exact verdict fragment)`.
/// The fragment pins the POSITION and BOTH types: a message that degenerated
/// into an unactionable "types disagree" would fail here, and so would one
/// that reported the key's type for a value-position conflict.
const CONFLICTS: &[(&str, &str, &str, &str)] = &[
    (
        "list: str annotation over an int comprehension",
        "    xs: list[str] = [i for i in range(5)]\n    print(xs[2])\n",
        "List(Str)",
        "produces elements of type I64 where the annotation says Str",
    ),
    (
        "list: float annotation over an int comprehension",
        "    xs: list[float] = [i for i in range(3)]\n    print(xs[1])\n",
        "List(F64)",
        "produces elements of type I64 where the annotation says F64",
    ),
    (
        "list: int annotation over a str comprehension",
        "    ws = ['a', 'bb']\n    xs: list[int] = [w for w in ws]\n    print(xs[1])\n",
        "List(I64)",
        "produces elements of type Str where the annotation says I64",
    ),
    (
        "set: str annotation over an int comprehension",
        "    s: set[str] = {i for i in range(4)}\n    print(len(s))\n",
        "Set(Str)",
        "produces elements of type I64 where the annotation says Str",
    ),
    (
        "dict: str VALUE annotation over an int comprehension",
        "    d: dict[int, str] = {i: i * 2 for i in range(4)}\n    print(d[3])\n",
        "Dict(I64, Str)",
        "produces values of type I64 where the annotation says Str",
    ),
    (
        "dict: str KEY annotation over an int comprehension",
        "    d: dict[str, int] = {i: i * 2 for i in range(4)}\n    print(d[3])\n",
        "Dict(Str, I64)",
        "produces keys of type I64 where the annotation says Str",
    ),
];

/// RED: every contradicting shape refuses, at the LOWERING stage, with a
/// message that names the annotated type, the produced type, and the position.
#[test]
fn contradicting_annotated_comprehension_refuses_at_lowering() {
    for (label, body, declared, verdict) in CONFLICTS {
        let err = match lower(body) {
            Err(e) => e,
            Ok(_) => panic!(
                "{label}: lowered without complaint — this shape emits Rust that \
                 rustc rejects with E0308, so accepting it is an accept-then-fail"
            ),
        };
        // Pin the STAGE. A `Parse` error would mean the fixture is malformed
        // rather than refused, and `Unimplemented` would mean the language is
        // unread — neither is the disposition this slice installs.
        assert!(
            matches!(err, FrontendError::Lower(_)),
            "{label}: expected a LOWERING refusal, got {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains(declared),
            "{label}: the refusal must name the ANNOTATED type `{declared}`; got:\n{msg}"
        );
        assert!(
            msg.contains(verdict),
            "{label}: the refusal must pin the position and BOTH types \
             (`{verdict}`); got:\n{msg}"
        );
        assert!(
            msg.contains("comprehension"),
            "{label}: the refusal must say WHICH construct disagrees; got:\n{msg}"
        );
    }
}

/// The refusal is specifically about the ANNOTATION, not about the
/// comprehension. Dropping the annotation from each conflicting shape leaves a
/// program that still lowers — so the check cannot be silently over-refusing
/// comprehensions in general and passing the RED half for the wrong reason.
#[test]
fn the_same_comprehension_without_the_annotation_still_lowers() {
    let unannotated = [
        "    xs = [i for i in range(5)]\n    print(xs[2])\n",
        "    ws = ['a', 'bb']\n    xs = [w for w in ws]\n    print(xs[1])\n",
        "    s = {i for i in range(4)}\n    print(len(s))\n",
        "    d = {i: i * 2 for i in range(4)}\n    print(d[3])\n",
    ];
    for body in unannotated {
        assert!(
            lower(body).is_ok(),
            "dropping the annotation must leave a program that lowers; body:\n{body}"
        );
    }
}

/// GREEN: annotations that AGREE keep lowering. These are the shapes a
/// careless equality check would falsely refuse — identity bodies (element
/// type comes from the iterable, not the body), method-call and builtin-call
/// bodies, bool bodies, genuinely-float bodies, and the two-generator
/// nested-loop lowering.
#[test]
fn agreeing_annotated_comprehensions_still_lower() {
    let agreeing = [
        ("list[int] over an int expr", "    xs: list[int] = [i * i for i in range(5)]\n    print(xs[3])\n"),
        ("list[int] with a filter", "    xs: list[int] = [i for i in range(6) if i % 2 == 0]\n    print(xs[1])\n"),
        ("list[str] identity body", "    ws = ['a', 'bb']\n    xs: list[str] = [w for w in ws]\n    print(xs[1])\n"),
        ("list[str] method-call body", "    ws = ['a', 'bb']\n    xs: list[str] = [w.upper() for w in ws]\n    print(xs[1])\n"),
        ("list[str] over a str literal", "    xs: list[str] = [c for c in 'abc']\n    print(xs[1])\n"),
        ("list[int] builtin body", "    ws = ['a', 'bb']\n    xs: list[int] = [len(w) for w in ws]\n    print(xs[1])\n"),
        ("list[bool] comparison body", "    xs: list[bool] = [i % 2 == 0 for i in range(4)]\n    print(xs[1])\n"),
        ("list[float] true-division body", "    xs: list[float] = [i / 2 for i in range(3)]\n    print(xs[1])\n"),
        ("list[int] two-generator", "    xs: list[int] = [i + j for i in range(2) for j in range(2)]\n    print(len(xs))\n"),
        ("set[int]", "    s: set[int] = {i for i in range(4)}\n    print(len(s))\n"),
        ("set[str]", "    ws = ['a', 'bb']\n    s: set[str] = {w for w in ws}\n    print(len(s))\n"),
        ("dict[int, int]", "    d: dict[int, int] = {i: i * 2 for i in range(4)}\n    print(d[3])\n"),
        ("dict[str, int]", "    ws = ['a', 'bb']\n    d: dict[str, int] = {w: len(w) for w in ws}\n    print(d['bb'])\n"),
        ("dict[int, str]", "    ws = ['a', 'bb']\n    d: dict[int, str] = {len(w): w for w in ws}\n    print(d[2])\n"),
    ];
    for (label, body) in agreeing {
        assert!(
            lower(body).is_ok(),
            "{label}: an annotation that AGREES with its comprehension must lower; \
             a false refusal here rejects a program that compiles and matches CPython"
        );
    }
}

/// GREEN, the conservatism boundary: when either leaf is NOT a confidently
/// inferred scalar, the check declines to judge at all. `infer_type_in_ctx`
/// falls back to `I64` for anything it cannot type, so refusing on a
/// non-scalar leaf would rest a hard error on a default — these shapes must
/// pass through untouched even though a naive comparison would flag some of
/// them.
#[test]
fn non_scalar_leaves_are_never_judged() {
    let unjudged = [
        ("nested list elements", "    xs: list[list[int]] = [[i, i + 1] for i in range(3)]\n    print(xs[2][1])\n"),
        ("tuple elements", "    xs: list[tuple[int, int]] = [(i, i) for i in range(2)]\n    print(xs[1][0])\n"),
        ("list-valued dict", "    ws = ['a', 'bb']\n    d: dict[str, list[int]] = {w: [len(w)] for w in ws}\n    print(d['bb'][0])\n"),
    ];
    for (label, body) in unjudged {
        assert!(
            lower(body).is_ok(),
            "{label}: a non-scalar leaf must not be judged — the check only fires \
             where the emitted Rust is CERTAIN to be rejected by rustc"
        );
    }
}
