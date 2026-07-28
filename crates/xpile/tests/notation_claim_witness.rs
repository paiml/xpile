//! XPILE-NOTATION-002 (PMAT-1431) — the notation contract's claims are
//! reconciled against the shipped `latex-contract-frontend`, BOTH WAYS.
//!
//! WHY THIS FILE EXISTS. `C-NOTATION-LATEX-MATH-TO-EQUATION` has
//! specified a theorem/proof-environment half since 2026-05-15, with a
//! Lean refinement theorem (`theorem_env_to_obligation`), a Silver
//! promotion (`theorem_env_obligation_kind_silver`), a Kani harness of
//! the same name, and a published book claim that "`theorem`/`lemma`
//! blocks lower to contract equations". None of it existed in Rust. On
//! 2026-07-28 the shipped parser answered `Ok` with ZERO
//! `proof_obligations` for all seven theorem-class environments, and
//! `\(...\)` and `gather` produced nothing at all — every one of them at
//! exit 0.
//!
//! The proofs were green the whole time because they range over ABSTRACT
//! models (`LeanTheoremEnv` is a bool plus a string; the Kani harness
//! quantifies over a single `bool`). A model with no implementation
//! behind it proves a property of nothing. This file is what ties the
//! two together, and it is deliberately keyed on the CLAIM CLASS — the
//! machine-readable `notation_surface` block — rather than on any one
//! construct, so a NEW claim added to the contract must be either
//! implemented or explicitly disclosed as `unimplemented` before it can
//! pass. That is the PMAT-1417 lesson (gate the class, not the file) and
//! the PMAT-1350 idiom (`emit_surface`, checked both ways).
//!
//! THE SHARPEST HALF is not the omission but the MISCLASSIFICATION.
//! `inline_math_to_equation`'s own domain says it covers math spans
//! "outside a theorem-class environment", and `lower_proof_env`'s Bronze
//! invariant is `body_leaked := false`. Before PMAT-1431 the math inside
//! a theorem body AND inside a proof body was emitted as free-standing
//! `eq_inline_*` equations. So the failure mode was not "the obligation
//! is missing" — a reader could see that — but "the theorem's content is
//! present, in the wrong bucket, indistinguishable from a free-standing
//! equation the author never wrote". `theorem_body_math_never_leaks_into_equations`
//! and `proof_body_math_never_leaks_into_equations` are the two tests
//! that pin it, and they are the two that matter most here.
//!
//! ARITY GUARDS. Every parse of the contract asserts a minimum row count
//! before asserting anything about the rows, so a YAML rename cannot turn
//! this file into a vacuous pass over an empty list (PMAT-1396: a
//! negative over an enumeration passes for free on an EMPTY enumeration).

use std::path::PathBuf;

use latex_contract_frontend::{LatexContractFrontend, THEOREM_CLASS_ENVIRONMENTS};
use xpile_contract_frontend::{ContractFrontend, EquationsBlock, ObligationType};

const CONTRACT: &str = "notation-latex-math-to-equation-v1.yaml";

/// Workspace-relative path, anchored on `CARGO_MANIFEST_DIR` (the
/// `xpile` crate dir) rather than the CWD. PMAT-1385 found four tests
/// that had silently resolved a relative fixture path against the
/// package root and measured a NONEXISTENT directory for months.
fn contract_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("contracts")
        .join(CONTRACT)
}

fn contract_yaml() -> serde_yaml::Value {
    let p = contract_path();
    let text = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("XPILE-NOTATION-002: cannot read {}: {e}", p.display()));
    serde_yaml::from_str(&text)
        .unwrap_or_else(|e| panic!("XPILE-NOTATION-002: {} is not valid YAML: {e}", p.display()))
}

/// The `notation_surface.<half>` rows, as `(id, note, probe, expect)`.
fn surface_rows(half: &str) -> Vec<SurfaceRow> {
    let doc = contract_yaml();
    let rows = doc
        .get("notation_surface")
        .unwrap_or_else(|| panic!("XPILE-NOTATION-002: {CONTRACT} has no `notation_surface` block"))
        .get(half)
        .unwrap_or_else(|| panic!("XPILE-NOTATION-002: `notation_surface` has no `{half}` list"))
        .as_sequence()
        .unwrap_or_else(|| panic!("XPILE-NOTATION-002: `notation_surface.{half}` is not a list"))
        .iter()
        .map(|r| SurfaceRow {
            id: str_field(r, "id", half),
            probe: str_field(r, "probe", half),
            expect: r.get("expect").and_then(|v| v.as_str()).map(str::to_string),
            expect_single_entry: r
                .get("expect_single_entry")
                .and_then(serde_yaml::Value::as_bool)
                .unwrap_or(false),
        })
        .collect::<Vec<_>>();
    rows
}

struct SurfaceRow {
    id: String,
    probe: String,
    expect: Option<String>,
    expect_single_entry: bool,
}

fn str_field(row: &serde_yaml::Value, key: &str, half: &str) -> String {
    row.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!("XPILE-NOTATION-002: a `notation_surface.{half}` row is missing `{key}`")
        })
        .to_string()
}

fn parse(src: &str) -> EquationsBlock {
    LatexContractFrontend
        .parse_to_equations(src)
        .unwrap_or_else(|e| panic!("XPILE-NOTATION-002: parse_to_equations refused: {e}"))
}

/// Total number of things a parse produced, across every output bucket.
fn total_yield(b: &EquationsBlock) -> usize {
    b.equations.len() + b.proof_obligations.len() + b.citations.len() + b.references.len()
}

/// The environment roster in the contract is the roster the parser
/// implements — checked in BOTH directions.
///
/// One direction alone is worthless here. "Every environment the
/// contract names is handled" passes trivially if the parser handles
/// everything under the sun; "every environment the parser handles is
/// named" passes trivially if the parser handles nothing, which is
/// exactly the state PMAT-1431 found.
#[test]
fn theorem_environment_roster_matches_the_contract_exactly() {
    let doc = contract_yaml();
    let declared: Vec<String> = doc
        .get("equations")
        .and_then(|e| e.get("theorem_env_to_obligation"))
        .and_then(|t| t.get("environments"))
        .unwrap_or_else(|| {
            panic!(
                "XPILE-NOTATION-002: {CONTRACT} equations.theorem_env_to_obligation has no \
                 machine-readable `environments` roster — the roster must not live in prose only"
            )
        })
        .as_sequence()
        .expect("`environments` is not a list")
        .iter()
        .map(|v| v.as_str().expect("env name is not a string").to_string())
        .collect();

    assert!(
        declared.len() >= 5,
        "XPILE-NOTATION-002: the contract declares only {} theorem environments; a roster \
         this short means the key was renamed or truncated and every assertion below would \
         pass vacuously",
        declared.len()
    );

    let mut want = declared.clone();
    want.sort();
    let mut got: Vec<String> = THEOREM_CLASS_ENVIRONMENTS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    got.sort();

    assert_eq!(
        want, got,
        "XPILE-NOTATION-002: the theorem-environment roster in {CONTRACT} and the roster \
         `latex_contract_frontend::THEOREM_CLASS_ENVIRONMENTS` implements have DRIFTED APART. \
         contract={want:?} parser={got:?}"
    );
}

/// Every environment on the roster produces exactly one obligation.
#[test]
fn every_rostered_theorem_environment_produces_exactly_one_obligation() {
    assert!(
        THEOREM_CLASS_ENVIRONMENTS.len() >= 5,
        "XPILE-NOTATION-002: arity guard — roster is {} long",
        THEOREM_CLASS_ENVIRONMENTS.len()
    );
    for env in THEOREM_CLASS_ENVIRONMENTS {
        let src =
            format!("\\begin{{{env}}}[Named]\nFor all $x > 0$, $f(x) \\ge 0$.\n\\end{{{env}}}");
        let b = parse(&src);
        assert_eq!(
            b.proof_obligations.len(),
            1,
            "XPILE-NOTATION-002: `{env}` is on the contract's roster but produced {} \
             proof_obligations (expected exactly 1). Block: {b:?}",
            b.proof_obligations.len()
        );
        let ob = &b.proof_obligations[0];
        assert_eq!(
            ob.ty,
            ObligationType::Postcondition,
            "XPILE-NOTATION-002: `{env}` carries no \\textbf{{Precondition:}} flag, so the \
             contract's polarity invariant requires `postcondition`"
        );
        assert_eq!(
            ob.applies_to, "Named",
            "XPILE-NOTATION-002: the amsthm `[label]` argument must land in `applies_to` \
             verbatim (resolution is identity at v0.1.x)"
        );
        assert!(
            !ob.formal.is_empty(),
            "XPILE-NOTATION-002: `{env}` body has two math spans, so `formal` must not be empty"
        );
    }
}

/// The load-bearing test. Before PMAT-1431 this was FALSE for all seven
/// environments: the theorem's math landed in `equations` as
/// `eq_inline_*`, the one bucket `inline_math_to_equation`'s domain
/// explicitly excludes.
#[test]
fn theorem_body_math_never_leaks_into_equations() {
    for env in THEOREM_CLASS_ENVIRONMENTS {
        let src = format!("\\begin{{{env}}}\nFor all $x > 0$, $f(x) \\ge 0$.\n\\end{{{env}}}");
        let b = parse(&src);
        assert!(
            b.equations.is_empty(),
            "XPILE-NOTATION-002: math inside a `{env}` environment leaked into `equations` \
             as {:?}. `inline_math_to_equation`'s domain covers spans OUTSIDE a theorem-class \
             environment; an entry here is indistinguishable from a free-standing equation the \
             author never wrote.",
            b.equations.keys().collect::<Vec<_>>()
        );
    }
}

/// `lower_proof_env`'s Bronze invariant is `body_leaked := false`. The
/// Lean theorem asserted it; the Rust violated it.
#[test]
fn proof_body_math_never_leaks_into_equations() {
    let b = parse("\\begin{proof}\nBy induction on $n$, using $k \\le n$.\n\\end{proof}");
    assert_eq!(
        total_yield(&b),
        0,
        "XPILE-NOTATION-002: a `proof` environment yielded {b:?}. \
         contracts/lean/Notation.lean models proof-env lowering with `body_leaked := false` \
         and the Kani harness `proof_env_body_never_leaks` mirrors it; the proof body's math \
         must not reach the EquationsBlock in any bucket."
    );
}

/// The `\textbf{Precondition:}` polarity — the single property the Lean
/// theorem and the Kani harness actually prove — executed over shipped
/// Rust rather than over a `bool`.
#[test]
fn precondition_flag_selects_obligation_polarity() {
    let flagged =
        parse("\\begin{theorem}\n\\textbf{Precondition:} $n > 0$ must hold.\n\\end{theorem}");
    assert_eq!(flagged.proof_obligations.len(), 1);
    assert_eq!(
        flagged.proof_obligations[0].ty,
        ObligationType::Precondition,
        "XPILE-NOTATION-002: a \\textbf{{Precondition:}}-flagged body must lower to \
         `precondition` — the polarity claim of the Lean theorem `theorem_env_to_obligation`"
    );

    let plain = parse("\\begin{theorem}\n$n > 0$ holds.\n\\end{theorem}");
    assert_eq!(plain.proof_obligations.len(), 1);
    assert_eq!(
        plain.proof_obligations[0].ty,
        ObligationType::Postcondition,
        "XPILE-NOTATION-002: an unflagged body must lower to `postcondition`. An emitter that \
         defaults the other way inverts what is assumed vs. what is proven."
    );
}

/// `inline_math_equiv_under_normaliser_silver` proves the `$...$` and
/// `\(...\)` forms lower to equal content while
/// `inline_kinds_are_distinct_silver` requires the source kind to remain
/// recoverable. Both were proofs about a paren lowering that did not
/// exist — `\( x \le 3 \)` produced zero equations.
#[test]
fn paren_and_dollar_inline_math_agree_on_content_and_differ_in_kind() {
    let dollar = parse("The bound $x \\le 3$ holds.");
    let paren = parse("The bound \\( x \\le 3 \\) holds.");

    assert_eq!(
        dollar.equations.len(),
        1,
        "XPILE-NOTATION-002: control — the `$...$` form must produce one equation"
    );
    assert_eq!(
        paren.equations.len(),
        1,
        "XPILE-NOTATION-002: `\\(...\\)` produced {} equations. The contract's \
         `inline_math_to_equation` domain names it explicitly and a Silver Lean theorem \
         proves it equivalent to the `$...$` form.",
        paren.equations.len()
    );

    let d_formula = dollar.equations.values().next().unwrap().formula.clone();
    let p_formula = paren.equations.values().next().unwrap().formula.clone();
    assert_eq!(
        d_formula, p_formula,
        "XPILE-NOTATION-002: the two inline forms must normalise to the same content — \
         `inline_math_equiv_under_normaliser_silver`"
    );

    let d_key = dollar.equations.keys().next().unwrap().clone();
    let p_key = paren.equations.keys().next().unwrap().clone();
    assert_ne!(
        d_key, p_key,
        "XPILE-NOTATION-002: the SOURCE KIND must stay recoverable from the entry key — \
         `inline_kinds_are_distinct_silver`. An emitter that quietly relabels `$...$` as \
         `\\(...\\)` must be falsifiable."
    );
}

/// Every `notation_surface.lowers` row produces what it says it does.
#[test]
fn every_declared_surface_row_lowers() {
    let rows = surface_rows("lowers");
    assert!(
        rows.len() >= 8,
        "XPILE-NOTATION-002: arity guard — `notation_surface.lowers` has only {} rows",
        rows.len()
    );
    for row in &rows {
        let b = parse(&row.probe);
        let bucket = row.expect.as_deref().unwrap_or_else(|| {
            panic!(
                "XPILE-NOTATION-002: `lowers` row `{}` has no `expect` bucket",
                row.id
            )
        });
        let n = match bucket {
            "equations" => b.equations.len(),
            "proof_obligations" => b.proof_obligations.len(),
            "citations" => b.citations.len(),
            "references" => b.references.len(),
            other => panic!(
                "XPILE-NOTATION-002: row `{}` names unknown bucket `{other}`",
                row.id
            ),
        };
        assert!(
            n >= 1,
            "XPILE-NOTATION-002: `notation_surface.lowers` row `{}` is published as lowering \
             to `{bucket}`, but the shipped parser produced {n} there. Probe:\n{}\nBlock: {b:?}",
            row.id,
            row.probe
        );
    }
}

/// Every `notation_surface.unimplemented` row produces NOTHING — so the
/// emptiness is disclosed rather than silent, and implementing one reds
/// this test and forces the row to move up to `lowers`.
///
/// Rows carrying `expect_single_entry` are a different shape: they are
/// disclosed as NOT SPLIT (one entry where a richer parser would produce
/// two), so the assertion is on the count, not on emptiness.
#[test]
fn every_unimplemented_surface_row_produces_nothing() {
    let rows = surface_rows("unimplemented");
    assert!(
        rows.len() >= 3,
        "XPILE-NOTATION-002: arity guard — `notation_surface.unimplemented` has only {} rows",
        rows.len()
    );
    for row in &rows {
        let b = parse(&row.probe);
        if row.expect_single_entry {
            let produced = total_yield(&b);
            assert_eq!(
                produced, 1,
                "XPILE-NOTATION-002: row `{}` is disclosed as producing a SINGLE unsplit \
                 entry, but the parser produced {produced}. Probe:\n{}\nBlock: {b:?}",
                row.id, row.probe
            );
            continue;
        }
        assert_eq!(
            total_yield(&b),
            0,
            "XPILE-NOTATION-002: row `{}` is disclosed as unimplemented, but the parser \
             produced output for it. Either the disclosure is stale (move the row to \
             `lowers`) or something is landing in the wrong bucket. Probe:\n{}\nBlock: {b:?}",
            row.id,
            row.probe
        );
    }
}

/// The published book claim and the shipped parser cannot drift apart.
///
/// PMAT-1430's lesson: gate the LOCATED row positively, and never use
/// bare substring containment for "the doc must mention X" — that
/// survives deleting the row it requires. Here the doc half is anchored
/// on the contract-table row for this contract, and the code half is the
/// live parser, so a regression on either side reds.
#[test]
fn the_published_theorem_claim_is_backed_by_the_parser() {
    let book = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("book/src/reference/contracts.md");
    let text = std::fs::read_to_string(&book)
        .unwrap_or_else(|e| panic!("XPILE-NOTATION-002: cannot read {}: {e}", book.display()));

    let row = text
        .lines()
        .find(|l| l.contains("notation-latex-math-to-equation-v1.yaml") && l.starts_with('|'))
        .unwrap_or_else(|| {
            panic!(
                "XPILE-NOTATION-002: {} has no contract-table row for {CONTRACT}",
                book.display()
            )
        });
    assert!(
        row.to_lowercase().contains("theorem"),
        "XPILE-NOTATION-002: the book's contract-table row for {CONTRACT} no longer mentions \
         theorem environments, but the parser implements them. Row: {row}"
    );

    let b = parse("\\begin{theorem}\nFor all $x > 0$, $f(x) \\ge 0$.\n\\end{theorem}");
    assert_eq!(
        b.proof_obligations.len(),
        1,
        "XPILE-NOTATION-002: the book publishes that theorem environments lower, and the \
         parser produced {} obligations. This is the exact pairing that was false for 74 days.",
        b.proof_obligations.len()
    );
}
