//! XPILE-PYOPSURFACE-001 (PMAT-1438) — the book published the Python
//! frontend's operator surface as a UNIVERSAL ("all binary operators", "all
//! unary operators") while the frontend itself enumerates a FINITE list and
//! refuses four operators.
//!
//! ## The defect this locks out
//!
//! `book/src/reference/frontends.md` has carried, since 2026-05-15, under
//! "Python frontend — what's supported":
//!
//! ```text
//! - all binary operators (with Python-floor semantics for `//` and `%`)
//! - all unary operators
//! ```
//!
//! Neither is true. Measured by driving `PythonFrontend::parse_and_lower`
//! over one probe per Python operator (this file's `CORPUS`):
//!
//! | class | universe | lowers | REFUSES |
//! |---|---|---|---|
//! | `ast.BinOp` | 13 | 12 | `@` (`MatMult`) |
//! | `ast.Compare` | 10 | 8 | `is` (`Is`), `is not` (`IsNot`) |
//! | `ast.UnaryOp` | 4 | 3 | unary `+` (`UAdd`) |
//!
//! The frontend's own refusal message says so in the same breath —
//! `unsupported binary operator: MatMult — supported: + - * / // % & | ^ << >>
//! **` — so the page and the binary contradicted each other on the page's own
//! subject, and the page names the CHANGELOG's `Python subset (live,
//! runtime-verified)` section as "the canonical source of truth ... to avoid
//! duplication-and-drift" while BEING the duplicate that drifted. The
//! canonical list is ENUMERATIVE and correct ("Binary arithmetic: `+ - * // %`
//! ... Bitwise: `& | ^ << >>` ... Unary: `-x` ... `not x`"); the book
//! paraphrased it into a universal the canonical source never states. A
//! paraphrase that WIDENS a claim is the failure mode, not one that drops a
//! detail.
//!
//! ## What this file checks, and why in this shape
//!
//! 1. **The published block is DERIVED, and compared by EQUALITY.** The block
//!    between the `XPILE-PYOPSURFACE-001` markers in `frontends.md` must equal
//!    the block this file generates from live frontend behaviour. Adding an
//!    operator, or losing one, reds the page until it moves — the
//!    `emit_surface` idiom (PMAT-1350), one lane over.
//!
//! 2. **A refusal counts only if the FRONTEND refused THE OPERATOR.** Two
//!    guards, because the vacuity risk here is a mis-signatured probe recording
//!    a *type* error as an operator refusal (`a / b` with `-> int` refuses
//!    with "declared return type I64 but body produces F64", which has nothing
//!    to do with `/`):
//!    - the error must be `FrontendError::Lower` whose text names `operator`
//!      or `unary`; and
//!    - the CONTROL — the same program with a reference operator substituted
//!      into the same slot — must LOWER. If the scaffolding is what refused,
//!      the control refuses too and this file reds with a corpus-bug message
//!      rather than publishing a false REFUSES row.
//!
//! 3. **The frontend's own enumeration is checked against its own behaviour.**
//!    `lower_binop`'s "supported: ..." list is a hand-typed claim on the ERROR
//!    path — the surface a user meets at the wall. It is asserted equal to the
//!    measured set of `ast.BinOp` operators that lower. It was WRONG in the
//!    under-reporting direction when this file was written: it omitted `/`,
//!    which lowers (`Operator::Div` is handled on the float path and never
//!    reaches `lower_binop`), so the user who hits `@` was told by omission
//!    that `/` is unsupported too.
//!
//! 4. **The falsehood itself is forbidden by spelling.** PMAT-1437's lesson
//!    (1): a gate that validates a VOCABULARY does not reach prose that uses
//!    none, and the original text contains no backticked key, no table row and
//!    no marker — a structural check alone would have passed it unchanged. So
//!    `no_universal_operator_claim_anywhere` scans every published `.md` for
//!    an unqualified universal over the operator surface. That check is scoped
//!    to the CLAIM CLASS (any published page, any of the three operator
//!    classes), not to the file or the two bullets that happened to carry it.
//!
//! ## Known limit, disclosed rather than papered over
//!
//! The universe (13/10/4) is the Python 3.12 grammar's operator roster and is
//! hand-authored here — Rust cannot enumerate `rustpython_parser::ast`'s enum
//! variants, so this file cannot PROVE its corpus is exhaustive. What it does
//! enforce is that the corpus has exactly those counts and that every probe
//! program is distinct, so a spelling typo collapses two rows and reds.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use depyler_frontend::PythonFrontend;
use xpile_frontend::{Frontend, FrontendError};

/// The three Python operator classes, with the size of each class's universe
/// in the Python 3.12 grammar. Used to red a corpus that silently loses a row.
const CLASS_UNIVERSE: &[(&str, usize)] = &[("BinOp", 13), ("Compare", 10), ("UnaryOp", 4)];

/// One probe per Python operator.
///
/// `class` — the Python AST node the operator belongs to.
/// `variant` — the `rustpython_parser::ast` variant name, so a REFUSES row can
///   be tied to the frontend's own message.
/// `template` — a whole Python module with `{op}` where the operator goes. The
///   signature is chosen so the operator is the ONLY thing that can refuse.
/// `spelling` — what goes in `{op}`.
/// `control` — a reference operator for the same slot; substituted into the
///   same template to prove the scaffolding lowers (only consulted when the
///   probe REFUSES).
struct Probe {
    class: &'static str,
    variant: &'static str,
    template: &'static str,
    spelling: &'static str,
    control: &'static str,
}

const INT_BIN: &str = "def f(a: int, b: int) -> int:\n    return a {op} b\n";
const FLOAT_BIN: &str = "def f(a: float, b: float) -> float:\n    return a {op} b\n";
const INT_CMP: &str = "def f(a: int, b: int) -> bool:\n    return a {op} b\n";
const LIST_CMP: &str = "def f(a: int, b: list[int]) -> bool:\n    return a {op} b\n";
const INT_UNARY: &str = "def f(a: int) -> int:\n    return {op}a\n";
const BOOL_UNARY: &str = "def f(a: bool) -> bool:\n    return {op}a\n";

const CORPUS: &[Probe] = &[
    // ---- ast.BinOp (13) ----
    Probe {
        class: "BinOp",
        variant: "Add",
        template: INT_BIN,
        spelling: "+",
        control: "-",
    },
    Probe {
        class: "BinOp",
        variant: "Sub",
        template: INT_BIN,
        spelling: "-",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "Mult",
        template: INT_BIN,
        spelling: "*",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "MatMult",
        template: INT_BIN,
        spelling: "@",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "Div",
        template: FLOAT_BIN,
        spelling: "/",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "Mod",
        template: INT_BIN,
        spelling: "%",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "Pow",
        template: INT_BIN,
        spelling: "**",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "LShift",
        template: INT_BIN,
        spelling: "<<",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "RShift",
        template: INT_BIN,
        spelling: ">>",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "BitOr",
        template: INT_BIN,
        spelling: "|",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "BitXor",
        template: INT_BIN,
        spelling: "^",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "BitAnd",
        template: INT_BIN,
        spelling: "&",
        control: "+",
    },
    Probe {
        class: "BinOp",
        variant: "FloorDiv",
        template: INT_BIN,
        spelling: "//",
        control: "+",
    },
    // ---- ast.Compare (10) ----
    Probe {
        class: "Compare",
        variant: "Eq",
        template: INT_CMP,
        spelling: "==",
        control: "<",
    },
    Probe {
        class: "Compare",
        variant: "NotEq",
        template: INT_CMP,
        spelling: "!=",
        control: "<",
    },
    Probe {
        class: "Compare",
        variant: "Lt",
        template: INT_CMP,
        spelling: "<",
        control: "==",
    },
    Probe {
        class: "Compare",
        variant: "LtE",
        template: INT_CMP,
        spelling: "<=",
        control: "==",
    },
    Probe {
        class: "Compare",
        variant: "Gt",
        template: INT_CMP,
        spelling: ">",
        control: "==",
    },
    Probe {
        class: "Compare",
        variant: "GtE",
        template: INT_CMP,
        spelling: ">=",
        control: "==",
    },
    Probe {
        class: "Compare",
        variant: "Is",
        template: INT_CMP,
        spelling: "is",
        control: "==",
    },
    Probe {
        class: "Compare",
        variant: "IsNot",
        template: INT_CMP,
        spelling: "is not",
        control: "==",
    },
    Probe {
        class: "Compare",
        variant: "In",
        template: LIST_CMP,
        spelling: "in",
        control: "in",
    },
    Probe {
        class: "Compare",
        variant: "NotIn",
        template: LIST_CMP,
        spelling: "not in",
        control: "in",
    },
    // ---- ast.UnaryOp (4) ----
    Probe {
        class: "UnaryOp",
        variant: "USub",
        template: INT_UNARY,
        spelling: "-",
        control: "~",
    },
    Probe {
        class: "UnaryOp",
        variant: "UAdd",
        template: INT_UNARY,
        spelling: "+",
        control: "-",
    },
    Probe {
        class: "UnaryOp",
        variant: "Invert",
        template: INT_UNARY,
        spelling: "~",
        control: "-",
    },
    Probe {
        class: "UnaryOp",
        variant: "Not",
        template: BOOL_UNARY,
        spelling: "not ",
        control: "not ",
    },
];

const BEGIN: &str = "<!-- XPILE-PYOPSURFACE-001:BEGIN -->";
const END: &str = "<!-- XPILE-PYOPSURFACE-001:END -->";
const PAGE: &str = "book/src/reference/frontends.md";

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum Disposition {
    Lowers,
    Refuses,
}

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/xpile -> crates -> repo root
    p.pop();
    p.pop();
    p
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn program(p: &Probe) -> String {
    p.template.replace("{op}", p.spelling)
}

fn control_program(p: &Probe) -> String {
    p.template.replace("{op}", p.control)
}

/// Drive the real Python frontend over one source string.
fn lower(source: &str) -> Result<usize, FrontendError> {
    PythonFrontend
        .parse_and_lower(Path::new("probe.py"), source)
        .map(|m| m.items.len())
}

/// Classify one probe, refusing to record a REFUSES row unless the frontend
/// refused the OPERATOR — see the file header, guard 2.
fn observe(p: &Probe) -> Disposition {
    match lower(&program(p)) {
        Ok(0) => panic!(
            "CORPUS BUG: probe `{}` ({}) lowered to an EMPTY module — a hollow \
             Ok is not evidence the operator lowers.\n---\n{}",
            p.spelling,
            p.variant,
            program(p)
        ),
        Ok(_) => Disposition::Lowers,
        Err(e) => {
            let msg = e.to_string();
            let names_operator = msg.contains("operator") || msg.contains("unary");
            assert!(
                matches!(e, FrontendError::Lower(_)) && names_operator,
                "CORPUS BUG: probe `{}` ({}) failed for a reason that is not an \
                 operator refusal — the row would publish a FALSE `REFUSES`.\n\
                 error: {msg}\n---\n{}",
                p.spelling,
                p.variant,
                program(p)
            );
            // The control proves the scaffolding (signature, operand types,
            // return type) is not what refused.
            let control = control_program(p);
            match lower(&control) {
                Ok(n) if n > 0 => Disposition::Refuses,
                other => panic!(
                    "CORPUS BUG: probe `{}` ({}) refused, but so did its CONTROL \
                     `{}` ({other:?}) — the template, not the operator, is what \
                     this probe measures.\n---\n{control}",
                    p.spelling, p.variant, p.control
                ),
            }
        }
    }
}

/// The block published between the markers, generated from live behaviour.
fn generated_block() -> String {
    let mut out = String::from("```text\n");
    out.push_str("class     variant   probe       disposition\n");
    for p in CORPUS {
        let expr = program(p)
            .lines()
            .last()
            .unwrap()
            .trim_start()
            .trim_start_matches("return ")
            .to_string();
        let d = match observe(p) {
            Disposition::Lowers => "lowers",
            Disposition::Refuses => "REFUSES",
        };
        out.push_str(&format!(
            "{:<9} {:<9} {:<11} {}\n",
            p.class, p.variant, expr, d
        ));
    }
    for (class, universe) in CLASS_UNIVERSE {
        let rows: Vec<&Probe> = CORPUS.iter().filter(|p| p.class == *class).collect();
        let refused = rows
            .iter()
            .filter(|p| observe(p) == Disposition::Refuses)
            .count();
        out.push_str(&format!(
            "\nast.{class}: {universe} in Python, {} lower, {refused} REFUSE",
            rows.len() - refused,
        ));
    }
    out.push_str("\n```\n");
    out
}

fn marked_block(page: &str) -> String {
    let start = page.find(BEGIN).unwrap_or_else(|| {
        panic!(
            "{PAGE} has no `{BEGIN}` marker. The Python operator surface must be \
             PUBLISHED as a derived block, not summarised in prose — the prose it \
             replaced said \"all binary operators\" and \"all unary operators\", \
             and four operators refuse."
        )
    }) + BEGIN.len();
    let end = page[start..]
        .find(END)
        .unwrap_or_else(|| panic!("{PAGE} has `{BEGIN}` but no `{END}`"))
        + start;
    page[start..end].trim_matches('\n').to_string()
}

/// (1) The published block equals live frontend behaviour, both directions.
#[test]
fn published_operator_surface_equals_the_frontend() {
    let want = generated_block();
    let got = marked_block(&read(PAGE));
    assert_eq!(
        got.trim(),
        want.trim(),
        "\n{PAGE} disagrees with the Python frontend it describes.\n\
         Replace the block between the markers with EXACTLY:\n\n{want}"
    );
}

/// (2) Corpus integrity: one probe per operator, no collapsed spellings.
#[test]
fn corpus_covers_each_class_exactly_once_per_operator() {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for p in CORPUS {
        assert!(
            seen.insert(program(p)),
            "CORPUS BUG: two probes generate the same program — a spelling typo \
             collapses `{}` ({}) onto another row and the surface is published \
             one operator short.",
            p.spelling,
            p.variant
        );
    }
    let mut by_class: BTreeMap<&str, usize> = BTreeMap::new();
    for p in CORPUS {
        *by_class.entry(p.class).or_default() += 1;
    }
    for (class, universe) in CLASS_UNIVERSE {
        assert_eq!(
            by_class.get(class).copied().unwrap_or(0),
            *universe,
            "the corpus does not cover every `ast.{class}` operator — the \
             published surface would be a sample, and a universal claim cannot \
             be established by sampling"
        );
    }
    assert_eq!(
        CORPUS.len(),
        CLASS_UNIVERSE.iter().map(|(_, n)| n).sum::<usize>()
    );
}

/// (3) The frontend's own "supported: ..." enumeration — the surface a user
/// meets at the wall — equals the `ast.BinOp` operators that actually lower.
#[test]
fn frontend_refusal_message_enumerates_what_actually_lowers() {
    let matmult = CORPUS
        .iter()
        .find(|p| p.variant == "MatMult")
        .expect("corpus lost its refused-BinOp probe, so this check has no message to read");
    let msg = match lower(&program(matmult)) {
        Err(e) => e.to_string(),
        Ok(_) => {
            // `@` now lowers: the enumeration this test reads no longer exists.
            // That is a real change and must be re-pointed deliberately.
            panic!(
                "`a @ b` now LOWERS, so `lower_binop`'s \"supported: ...\" message \
                 is unreachable from this probe. Re-point this check at whatever \
                 operator still refuses, or delete it and say so on {PAGE}."
            );
        }
    };
    let listed = msg
        .split_once("supported:")
        .unwrap_or_else(|| {
            panic!(
                "the frontend's binary-operator refusal no longer enumerates a \
                 supported set, so nothing pins it to behaviour.\nmessage: {msg}"
            )
        })
        .1;
    let claimed: BTreeSet<&str> = listed.split_whitespace().collect();
    let actual: BTreeSet<&str> = CORPUS
        .iter()
        .filter(|p| p.class == "BinOp" && observe(p) == Disposition::Lowers)
        .map(|p| p.spelling)
        .collect();
    assert_eq!(
        claimed, actual,
        "`lower_binop`'s refusal message enumerates a supported set that is not \
         what the frontend lowers. This is the ERROR-PATH twin of the doc defect \
         PMAT-1438 fixed: it omitted `/`, which lowers, so the user who hit `@` \
         was told by omission that `/` is unsupported too."
    );
}

/// (4) The falsehood itself, by spelling, across every published page.
///
/// Scoped to the CLAIM CLASS: any published Markdown, any of the three
/// operator classes. The prose this replaced carried no backticked key, no
/// table row and no marker, so only a spelling check reaches it.
#[test]
fn no_universal_operator_claim_anywhere() {
    let root = repo_root();
    let mut pages: Vec<PathBuf> = Vec::new();
    collect_md(&root.join("book/src"), &mut pages);
    collect_md(&root.join("docs"), &mut pages);
    pages.push(root.join("README.md"));
    // The DECLARED canonical source of the Python subset. It is enumerative
    // today; this keeps it from acquiring the paraphrase that broke the book.
    pages.push(root.join("CHANGELOG.md"));
    assert!(
        pages.len() > 20,
        "the published-page sweep found only {} files — it would pass over \
         nearly nothing",
        pages.len()
    );

    let classes = ["binary", "unary", "comparison"];
    let mut offenders: Vec<String> = Vec::new();
    for page in &pages {
        let Ok(text) = std::fs::read_to_string(page) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            let lower = line.to_lowercase();
            for class in classes {
                for quantifier in ["all ", "every ", "any "] {
                    let needle = format!("{quantifier}{class} operator");
                    if lower.contains(&needle) {
                        offenders.push(format!(
                            "{}:{}: {}",
                            page.strip_prefix(&root).unwrap_or(page).display(),
                            i + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a published page states a UNIVERSAL over an operator class. Four Python \
         operators refuse (`@`, `is`, `is not`, unary `+`) and the frontend's own \
         message enumerates a finite list, so no such sentence can be true \
         without a derived block behind it (see the `{BEGIN}` block on {PAGE}):\n  {}",
        offenders.join("\n  ")
    );
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_md(&p, out);
        } else if p.extension().is_some_and(|x| x == "md") {
            out.push(p);
        }
    }
}
