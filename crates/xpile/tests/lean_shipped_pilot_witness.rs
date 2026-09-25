//! PMAT-2157 (epic #2125 row 2): the first Lean model checked against what
//! xpile SHIPS, not against a second copy of itself.
//!
//! `contracts/lean/PyIntArith.lean` models Python `//`, `%` and `>>` on i64 as
//! `Int.fdiv`, `Int.fmod` and `Int.fdiv a (2 ^ n)`. Its refinement theorems are
//! `rfl` between two Lean defs that are the same expression, so no edit to
//! `xpile-rust-codegen` could ever turn one red (PMAT-1512's finding).
//!
//! The chain is split in two because no CI job has both toolchains:
//!
//! 1. **Model = pin**, checked by Lean. PyIntArith.lean ends with a block of
//!    `example : i64_floor_div (a) (b) = (v) := by decide` lines. `lake build`
//!    (the `lake-build` job) fails if the model disagrees with any pinned `v`.
//!    Each `=false` dual, `example : Int.tdiv (a) (b) ≠ (v) := by decide`,
//!    proves the pin tells floor from truncation, so the chain is not vacuous.
//! 2. **Pin = shipped**, checked here. This test transpiles a Python module
//!    using `//`, `%` and `>>` through the shipped `xpile transpile --target
//!    rust`, compiles it with `rustc`, runs it on every pinned input, and
//!    asserts every pinned `v` (both the equations and the duals) is the
//!    shipped output.
//!
//! The pins are parsed out of the Lean file, never typed here. [`grid`] is the
//! input set they must cover exactly, so deleting a pin reds this test, and
//! every op must keep at least one dual.
//!
//! ## Falsification, executed
//!
//! Replacing `emit_floor_div`'s floor correction with a plain `__q` (Rust's
//! truncating `/`) turns this test RED on `floor_div(-7, 2)`: pinned -4,
//! shipped -3. Changing `i64_floor_div` in the Lean model to `Int.tdiv` turns
//! `lake build` RED on the same pin. Both runs are recorded in the PR.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const LEAN: &str = "contracts/lean/PyIntArith.lean";
const BEGIN: &str = "-- BEGIN shipped pins (PMAT-2157)";
const END: &str = "-- END shipped pins (PMAT-2157)";

/// Dividends. The i64 extremes are in so the pins cover the boundary the
/// `fits_i64` hypotheses talk about.
const A: [i64; 6] = [-7, 7, -6, 0, i64::MAX, i64::MIN];
/// Divisors for `//` and `%`. No `-1`, since `i64::MIN // -1` overflows and
/// the shipped code panics there by contract.
const B: [i64; 5] = [2, -2, 3, -3, 1];
/// Shift amounts for `>>`.
const N: [i64; 4] = [0, 1, 3, 63];

const PY: &str = "\
def floor_div(a: int, b: int) -> int:
    return a // b


def floor_mod(a: int, b: int) -> int:
    return a % b


def shift_right(a: int, b: int) -> int:
    return a >> b
";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Op {
    FloorDiv,
    FloorMod,
    ShiftRight,
}

impl Op {
    fn shipped_fn(self) -> &'static str {
        match self {
            Op::FloorDiv => "floor_div",
            Op::FloorMod => "floor_mod",
            Op::ShiftRight => "shift_right",
        }
    }
}

/// Every (op, a, b) the pins must cover, no more and no less.
fn grid() -> BTreeSet<(Op, i64, i64)> {
    let mut g = BTreeSet::new();
    for a in A {
        for b in B {
            g.insert((Op::FloorDiv, a, b));
            g.insert((Op::FloorMod, a, b));
        }
        for n in N {
            g.insert((Op::ShiftRight, a, n));
        }
    }
    g
}

#[derive(Debug)]
struct Pin {
    op: Op,
    a: i64,
    b: i64,
    v: i64,
    dual: bool,
    line: usize,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

/// The parenthesised integer arguments of one pin, e.g. `(-7) (2) = (-4)`.
fn paren_ints(s: &str) -> Vec<i64> {
    s.split('(')
        .skip(1)
        .filter_map(|t| t.split(')').next())
        .filter_map(|t| t.trim().parse().ok())
        .collect()
}

/// Parses one line of the pin block. `None` for anything that is not an
/// `example`, which the caller rejects: the block holds pins and comments only.
fn parse_pin(line: &str, lineno: usize) -> Option<Pin> {
    let body = line
        .strip_prefix("example : ")?
        .strip_suffix(" := by decide")?;
    let (op, dual, rest) = if let Some(r) = body.strip_prefix("i64_floor_div ") {
        (Op::FloorDiv, false, r)
    } else if let Some(r) = body.strip_prefix("i64_mod ") {
        (Op::FloorMod, false, r)
    } else if let Some(r) = body.strip_prefix("i64_shr ") {
        (Op::ShiftRight, false, r)
    } else if let Some(r) = body.strip_prefix("Int.tdiv ") {
        // `Int.tdiv (a) (2 ^ n)` is the shift dual; `Int.tdiv (a) (b)` the `//` one.
        if r.contains("(2 ^ ") {
            (Op::ShiftRight, true, r)
        } else {
            (Op::FloorDiv, true, r)
        }
    } else if let Some(r) = body.strip_prefix("Int.tmod ") {
        (Op::FloorMod, true, r)
    } else {
        return None;
    };
    if dual != rest.contains(" ≠ ") || (!dual && !rest.contains(" = ")) {
        return None;
    }
    let rest = rest.replace("(2 ^ ", "(");
    let ints = paren_ints(&rest);
    let [a, b, v] = ints[..] else { return None };
    Some(Pin {
        op,
        a,
        b,
        v,
        dual,
        line: lineno,
    })
}

fn read_pins() -> Vec<Pin> {
    let src = std::fs::read_to_string(workspace_root().join(LEAN)).expect("read PyIntArith.lean");
    let lines: Vec<&str> = src.lines().collect();
    let begin = lines
        .iter()
        .position(|l| l.trim() == BEGIN)
        .unwrap_or_else(|| panic!("{LEAN} has no `{BEGIN}` line"));
    let end = lines
        .iter()
        .position(|l| l.trim() == END)
        .unwrap_or_else(|| panic!("{LEAN} has no `{END}` line"));
    assert!(
        begin < end,
        "{LEAN}: the pin block's END comes before its BEGIN"
    );
    let mut pins = Vec::new();
    for (i, l) in lines.iter().enumerate().take(end).skip(begin + 1) {
        let t = l.trim();
        if t.is_empty() || t.starts_with("--") {
            continue;
        }
        let pin = parse_pin(t, i + 1)
            .unwrap_or_else(|| panic!("{LEAN}:{}: not a pin this witness can read: {t}", i + 1));
        pins.push(pin);
    }
    pins
}

/// Transpiles [`PY`] with the shipped CLI, links it to a `main` that prints
/// `op a b result` for every grid point, and returns the printed results.
fn shipped_outputs() -> std::collections::BTreeMap<(Op, i64, i64), i64> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("lean_shipped_pilot");
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let py = dir.join("ops.py");
    std::fs::write(&py, PY).expect("write ops.py");
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(["transpile"])
        .arg(&py)
        .args(["--target", "rust"])
        .output()
        .expect("spawn xpile");
    assert!(
        out.status.success(),
        "xpile transpile --target rust failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut rs = String::from_utf8(out.stdout).expect("utf-8 Rust");
    for f in ["floor_div", "floor_mod", "shift_right"] {
        assert!(
            rs.contains(&format!("pub fn {f}(a: i64, b: i64) -> i64")),
            "the shipped emit no longer has `pub fn {f}(a: i64, b: i64) -> i64`:\n{rs}"
        );
    }
    rs.push_str("\nfn main() {\n");
    for (op, a, b) in grid() {
        let f = op.shipped_fn();
        rs.push_str(&format!(
            "    println!(\"{f} {{}} {{}} {{}}\", {a}i64, {b}i64, {f}({a}i64, {b}i64));\n"
        ));
    }
    rs.push_str("}\n");
    let src = dir.join("ops.rs");
    std::fs::write(&src, &rs).expect("write ops.rs");
    let bin = dir.join("ops");
    let c = Command::new("rustc")
        .args(["--edition", "2021", "-O", "-o"])
        .arg(&bin)
        .arg(&src)
        .output()
        .expect("spawn rustc");
    assert!(
        c.status.success(),
        "rustc rejected the shipped emit:\n{}",
        String::from_utf8_lossy(&c.stderr)
    );
    let run = Command::new(&bin).output().expect("run ops");
    assert!(run.status.success(), "the shipped ops binary failed");
    let mut got = std::collections::BTreeMap::new();
    for l in String::from_utf8_lossy(&run.stdout).lines() {
        let mut w = l.split_whitespace();
        let op = match w.next() {
            Some("floor_div") => Op::FloorDiv,
            Some("floor_mod") => Op::FloorMod,
            Some("shift_right") => Op::ShiftRight,
            other => panic!("unexpected line from the ops binary: {other:?}"),
        };
        let n: Vec<i64> = w.map(|t| t.parse().expect("an i64")).collect();
        got.insert((op, n[0], n[1]), n[2]);
    }
    assert_eq!(
        got.len(),
        grid().len(),
        "the ops binary skipped a grid point"
    );
    got
}

/// The pin block covers the grid exactly and every op keeps a dual. This half
/// needs no toolchain beyond reading the file.
#[test]
fn the_pins_cover_the_grid_and_every_op_has_a_dual() {
    let pins = read_pins();
    assert!(!pins.is_empty(), "{LEAN}: the pin block holds no pins");
    let eqs: BTreeSet<(Op, i64, i64)> = pins
        .iter()
        .filter(|p| !p.dual)
        .map(|p| (p.op, p.a, p.b))
        .collect();
    let n_eqs = pins.iter().filter(|p| !p.dual).count();
    assert_eq!(n_eqs, eqs.len(), "{LEAN}: a pin is repeated");
    let want = grid();
    let missing: Vec<_> = want.difference(&eqs).collect();
    let extra: Vec<_> = eqs.difference(&want).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{LEAN}'s pins do not cover the grid exactly: missing {missing:?}, extra {extra:?}"
    );
    for op in [Op::FloorDiv, Op::FloorMod, Op::ShiftRight] {
        let duals = pins.iter().filter(|p| p.dual && p.op == op).count();
        assert!(
            duals > 0,
            "{LEAN}: {op:?} has no `≠` dual, so nothing shows its pins tell floor \
             from truncation"
        );
    }
    for p in pins.iter().filter(|p| p.dual) {
        assert!(
            want.contains(&(p.op, p.a, p.b)),
            "{LEAN}:{}: a dual off the grid is never compared with the shipped output",
            p.line
        );
    }
}

/// Every pinned value, equation and dual alike, is what the shipped Rust
/// codegen's output computes.
#[test]
fn every_pin_is_the_shipped_output() {
    let pins = read_pins();
    assert!(!pins.is_empty(), "{LEAN}: the pin block holds no pins");
    let got = shipped_outputs();
    let wrong: Vec<String> = pins
        .iter()
        .filter(|p| got[&(p.op, p.a, p.b)] != p.v)
        .map(|p| {
            format!(
                "{LEAN}:{}: {}({}, {}) pinned {}, shipped {}",
                p.line,
                p.op.shipped_fn(),
                p.a,
                p.b,
                p.v,
                got[&(p.op, p.a, p.b)]
            )
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "the Lean model's pins disagree with xpile's shipped Rust:\n{}",
        wrong.join("\n")
    );
}
