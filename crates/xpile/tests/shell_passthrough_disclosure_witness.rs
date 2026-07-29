//! XPILE-SHELLPASS-001 — frontend ACCEPTANCE is not evidence of MODELLING,
//! and both shipping documents have to say so (PMAT-1479).
//!
//! ## The claim this exists to falsify
//!
//! `CLAUDE.md` closed its bashrs scope paragraph with a universally quantified
//! promise: every shell construct outside the enumerated surface *refuses with
//! a hard `FrontendError`* rather than being shredded into barewords. The
//! enumerated surface is quoting, `$VAR`, `$(…)`, pipelines, single-value
//! assignment, the three loop dialects, `if`/`elif`/`else`, and top-level
//! `case`. `[Unreleased]`'s "What still REFUSES" section carried the same model
//! in list form: a refusal roster, plus a disclosure that *conditions and case
//! patterns* are opaque — and nothing else.
//!
//! Measured through the shipped CLI on 2026-07-29: twenty shapes outside that
//! surface were probed and ALL TWENTY were accepted at exit 0, none refused.
//! Twenty is a probe count, not a survey of POSIX — the honest claim is that the
//! screen found no counterexample, not that the class has exactly twenty
//! members. They are not shredded: the emitted
//! script is `sh -n`-clean, byte-identical, and executes identically. They are
//! also not modelled — every one of them lowers to `Stmt::Cmd`, with the
//! operator riding along as an ordinary `Expr::LitStr` *word*. That is the
//! deliberate `LitStr`-passthrough design the frontend's own tests lock in by
//! name (PMAT-085..092 + PMAT-119, with `XPILE-BASHRS-SUBSHELL-001` and
//! `XPILE-BASHRS-STMT-SEP-001` filed as the structural follow-ups). The
//! behaviour was never the defect. The two documents that describe it were.
//!
//! ## Why the direction matters
//!
//! This is an over-claimed REFUSAL, which is the dangerous half. The standing
//! rule of this window is that converting a silent wrong answer into an honest
//! refusal outranks new capability — so a reader treats the refusal roster as
//! the load-bearing guarantee and concludes: *my script was accepted at exit 0,
//! therefore it is inside the modelled subset and the round-trip is certified.*
//! Twenty probed shapes make that inference false, and one of them is in the
//! repo's own gated corpus: `crates/xpile/examples/inputs/install.sh` ends with
//! `echo "done" > /tmp/out/install.log`, whose `>` is a `LitStr` word.
//!
//! `shell_artifact_policy_witness.rs` (XPILE-SHELLPOLICY-001) draws exactly
//! that inference in prose — every tracked artifact "is accepted by
//! `bashrs-frontend`, so none is an opaque blob sitting outside the
//! substrate-quality regime". Its five invariants are true and worth keeping;
//! what does not follow is the *so*. Acceptance plus byte-identical re-emission
//! is what passthrough gives you **for free**: a verbatim word is a fixed point
//! by construction. That gate's honest claim is structural round-trip, which it
//! measures; "inside the regime" is a stronger claim that no gate checks, and
//! this file is the reason it now reads as the weaker one.
//!
//! ## What is enforced here, and what is deliberately not
//!
//! 1. `passthrough_operators_are_accepted_but_lower_to_an_unmodelled_command`
//!    drives the REAL `BashrsFrontend` over the pinned table and asserts each
//!    shape is `Ok` **and** lands as `Stmt::Cmd` — accepted, unmodelled, both
//!    measured on the same run. A future slice that models one structurally
//!    reds this and has to move the prose row with it.
//! 2. `modelled_control_flow_is_distinguishable_from_passthrough` is the
//!    non-vacuity control: `Stmt::Cmd` has to be a *discriminating* answer, so
//!    a `for` loop, an `if` and a `case` must lower to their own variants. If
//!    the frontend ever regressed to emitting `Stmt::Cmd` for everything,
//!    assertion 1 would pass for the wrong reason and this test fails.
//! 3. `refused_shapes_still_refuse` pins the other side of the boundary — the
//!    roster the release notes publish. A row that stops refusing is a silent
//!    shred, which is the whole reason the roster exists.
//! 4. `both_documents_disclose_the_passthrough_class` requires the marker and
//!    every pinned family name in its paragraph, in `CLAUDE.md` and
//!    `CHANGELOG.md` alike — PMAT-1478's lesson, that a gate written for a
//!    disclosure defect had only one of the two documents as its subject.
//! 5. `no_document_makes_a_universal_refusal_claim` is the screen for the
//!    sentence that started this. Needle and haystack are normalised
//!    identically (PMAT-1476: choosing a spelling just moves the hole), block
//!    quotes are exempt as quotation, and a synthetic positive control proves
//!    the screen can still fire.
//!
//! It does NOT execute the emitted scripts. Byte-identity of the round-trip is
//! already gated over the whole tracked corpus by XPILE-SHELLPOLICY-001 and
//! executed over the curated subset by `shell_diff_exec.rs`; re-running it here
//! would add a third copy of the weakest half of the evidence, which is the
//! opposite of the point.

use bashrs_frontend::BashrsFrontend;
use std::path::{Path, PathBuf};
use xpile_frontend::Frontend;
use xpile_meta_hir::{Expr, Item, Module, Stmt};

/// The pinned passthrough table: `(family, source, the operator word)`.
///
/// `family` is the token that must appear in both documents' disclosure
/// paragraph, so this array is the single source of truth for the prose too —
/// adding a row here reds assertion 4 until the documents are updated.
const PASSTHROUGH: &[(&str, &str, &str)] = &[
    ("background", "echo hi &\n", "&"),
    ("&&", "echo a && echo b\n", "&&"),
    ("||", "false || echo b\n", "||"),
    (";", "echo a ; echo b\n", ";"),
    ("redirection", "echo hi > out.txt\n", ">"),
    ("redirection", "echo hi >> out.txt\n", ">>"),
    ("redirection", "cat < in.txt\n", "<"),
    ("redirection", "ls /nope 2>&1\n", "2>&1"),
    ("subshell", "( cd /tmp && ls )\n", "("),
    ("brace group", "{ echo a; echo b; }\n", "{"),
    (
        "function definition",
        "greet() { echo hello; }\n",
        "greet()",
    ),
    ("negation", "! false\n", "!"),
    ("test bracket", "[ 1 -eq 1 ]\n", "["),
];

/// Shapes the release notes publish as REFUSED. A row that starts returning
/// `Ok` is a silent shred, not a capability win.
const REFUSED: &[(&str, &str)] = &[
    ("here-doc, spaced", "cat <<EOF\nhi\nEOF\n"),
    ("here-doc, attached", "cat<<EOF\nhi\nEOF\n"),
    ("here-doc, fd-prefixed", "cat 0<<EOF\nhi\nEOF\n"),
    ("here-doc, dash", "cat<<-EOF\nhi\nEOF\n"),
    (
        "case fall-through `;&`",
        "case x in\na) echo a ;&\nb) echo b ;;\nesac\n",
    ),
    (
        "case fall-through `;;&`",
        "case x in\na) echo a ;;&\nb) echo b ;;\nesac\n",
    ),
    ("`&` in command position", "& echo hi\n"),
    (
        "case nested in a loop body",
        "for i in 1 2; do\ncase $i in\n1) echo one ;;\nesac\ndone\n",
    ),
];

/// The families that must be named in each document's disclosure paragraph,
/// derived from `PASSTHROUGH` so the two can never disagree.
fn pinned_families() -> Vec<&'static str> {
    let mut fams: Vec<&str> = PASSTHROUGH.iter().map(|(f, _, _)| *f).collect();
    fams.dedup();
    fams.sort_unstable();
    fams.dedup();
    fams
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

fn lower(source: &str) -> Result<Module, String> {
    BashrsFrontend
        .parse_and_lower(Path::new("probe.sh"), source)
        .map_err(|e| format!("{e:?}"))
}

/// The statements of the single synthetic function every shell module lowers to.
fn stmts(module: &Module) -> &[Stmt] {
    let Some(Item::Function(f)) = module.items.iter().find(|i| matches!(i, Item::Function(_)))
    else {
        panic!("shell module lowered without a function item: {module:?}");
    };
    &f.body.stmts
}

/// Every word the lowered command carries — program plus `LitStr` args. The
/// operator has to show up in here for the passthrough claim to be measured
/// rather than assumed.
fn words(stmt: &Stmt) -> Vec<String> {
    let Stmt::Cmd { program, args } = stmt else {
        return Vec::new();
    };
    let mut out = vec![program.clone()];
    for a in args {
        if let Expr::LitStr(s) = a {
            out.push(s.clone());
        }
    }
    out
}

/// Assertion 1. Accepted AND unmodelled, measured on the same run.
#[test]
fn passthrough_operators_are_accepted_but_lower_to_an_unmodelled_command() {
    assert!(
        PASSTHROUGH.len() >= 13,
        "the passthrough table shrank below the 13 shapes measured for PMAT-1479 \
         ({} rows) — a removal has to be justified in the CHANGELOG, not silently \
         dropped from the gate",
        PASSTHROUGH.len()
    );

    for (family, source, operator) in PASSTHROUGH {
        let module = lower(source).unwrap_or_else(|e| {
            panic!(
                "XPILE-SHELLPASS-001: `{}` ({family}) is documented as ACCEPTED via \
                 LitStr passthrough, but the frontend refused it: {e}. If this shape \
                 now refuses, that is a capability CHANGE — move the row to the \
                 refusal roster in CHANGELOG.md and CLAUDE.md in the same commit.",
                source.trim_end()
            )
        });

        let body = stmts(&module);
        assert!(
            !body.is_empty(),
            "`{}` lowered to an EMPTY body — acceptance with nothing lowered is the \
             shred this gate exists to catch",
            source.trim_end()
        );

        // Unmodelled: no structured shell variant claims it.
        for stmt in body {
            assert!(
                matches!(stmt, Stmt::Cmd { .. }),
                "XPILE-SHELLPASS-001: `{}` ({family}) now lowers to a STRUCTURED \
                 statement ({stmt:?}) instead of `Stmt::Cmd`. That is a real \
                 improvement — and it invalidates the disclosure paragraph in \
                 CHANGELOG.md and CLAUDE.md, which say this family is carried as an \
                 opaque word. Update both, then move this row.",
                source.trim_end()
            );
        }

        // The operator survives as a WORD — the observable signature of
        // passthrough, and the reason `Stmt::Cmd` here means "unmodelled"
        // rather than "modelled as a command".
        let all: Vec<String> = body.iter().flat_map(words).collect();
        assert!(
            all.iter().any(|w| w.contains(operator)),
            "`{}` ({family}) lowered to `Stmt::Cmd` but the operator `{operator}` \
             appears in NO word of {all:?} — it was DROPPED, which is a silent \
             semantic change, not passthrough",
            source.trim_end()
        );
    }
}

/// Assertion 2 — non-vacuity. `Stmt::Cmd` has to be a discriminating answer.
#[test]
fn modelled_control_flow_is_distinguishable_from_passthrough() {
    let cases: &[(&str, &str)] = &[
        ("for i in 1 2; do\necho $i\ndone\n", "ShellLoop"),
        ("if true; then\necho yes\nfi\n", "ShellIf"),
        ("case x in\nx) echo hit ;;\nesac\n", "ShellCase"),
    ];
    for (source, want) in cases {
        let module = lower(source).unwrap_or_else(|e| panic!("`{source}` must lower: {e}"));
        let body = stmts(&module);
        let modelled = body.iter().any(|s| {
            matches!(
                (s, *want),
                (Stmt::ShellLoop { .. }, "ShellLoop")
                    | (Stmt::ShellIf { .. }, "ShellIf")
                    | (Stmt::ShellCase { .. }, "ShellCase")
            )
        });
        assert!(
            modelled,
            "control: `{}` must lower to `Stmt::{want}`, else \
             `Stmt::Cmd` in the passthrough test is not evidence of anything — \
             got {body:?}",
            source.trim_end()
        );
    }
}

/// Assertion 3. The published refusal roster still refuses.
#[test]
fn refused_shapes_still_refuse() {
    for (name, source) in REFUSED {
        let got = lower(source);
        assert!(
            got.is_err(),
            "XPILE-SHELLPASS-001: {name} is published as REFUSED in `[Unreleased]`'s \
             \"What still REFUSES\", but the frontend accepted it. Either it is now \
             modelled (move the row and say so) or it is being shredded (the defect \
             PMAT-1371/PMAT-1377 closed). Source: {source:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The prose half.
// ---------------------------------------------------------------------------

/// Normalise haystack and needle IDENTICALLY (PMAT-1476: picking a spelling
/// moves the hole). Backticks and brackets are FORMATTING, not quotation, so
/// they are stripped as characters and their contents kept.
fn normalise(s: &str) -> String {
    let lowered = s.to_lowercase();
    let stripped: String = lowered
        .chars()
        .filter(|c| !matches!(c, '`' | '[' | ']' | '*' | '_'))
        .collect();
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Blank-line-delimited paragraphs, block quotes dropped as quotation.
fn paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n")
        .map(|p| {
            p.lines()
                .filter(|l| !l.trim_start().starts_with('>'))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

const DOCS: [&str; 2] = ["CLAUDE.md", "CHANGELOG.md"];

/// Assertion 4. BOTH documents, PMAT-1478's lesson.
#[test]
fn both_documents_disclose_the_passthrough_class() {
    let families = pinned_families();
    for doc in DOCS {
        let path = repo_root().join(doc);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {doc}: {e}"));
        let marked: Vec<String> = paragraphs(&text)
            .into_iter()
            .filter(|p| p.contains("XPILE-SHELLPASS-001"))
            .collect();
        assert!(
            !marked.is_empty(),
            "XPILE-SHELLPASS-001: {doc} carries no paragraph marked \
             `XPILE-SHELLPASS-001`. {} shell operator shapes are accepted at exit 0 \
             without being modelled; a document that describes the refusal roster \
             and omits that class tells a reader exit 0 means more than it does.",
            PASSTHROUGH.len()
        );
        let joined = normalise(&marked.join(" "));
        for family in &families {
            assert!(
                joined.contains(&normalise(family)),
                "XPILE-SHELLPASS-001: {doc}'s disclosure paragraph never names the \
                 `{family}` family, which `PASSTHROUGH` pins as accepted-but-unmodelled. \
                 The table is the source of truth; the prose has to follow it."
            );
        }
    }
}

/// Assertion 5. No universally quantified refusal promise, anywhere.
#[test]
fn no_document_makes_a_universal_refusal_claim() {
    // Each needle is a claim that EVERYTHING outside the enumerated surface
    // refuses. The measured answer is twenty counterexamples out of twenty probes.
    const UNIVERSAL: [&str; 6] = [
        "everything else refuses",
        "anything else refuses",
        "all other constructs refuse",
        "every other construct refuses",
        "everything outside it refuses",
        "everything else is refused",
    ];

    // Positive control: the screen has to be able to fire at all. This is the
    // sentence CLAUDE.md actually carried until PMAT-1479.
    let control = normalise(
        "Everything else refuses with a hard `FrontendError` rather than shredding \
         into barewords.",
    );
    assert!(
        UNIVERSAL.iter().any(|n| control.contains(&normalise(n))),
        "the universal-claim screen does not flag its own positive control — the \
         needles have drifted away from the defect they were written for"
    );

    for doc in DOCS {
        let path = repo_root().join(doc);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {doc}: {e}"));
        for para in paragraphs(&text) {
            let hay = normalise(&para);
            for needle in UNIVERSAL {
                assert!(
                    !hay.contains(&normalise(needle)),
                    "XPILE-SHELLPASS-001: {doc} asserts that everything outside the \
                     enumerated shell surface refuses (\"{needle}\"). Measured on \
                     2026-07-29, twenty of twenty probed shapes are accepted via LitStr \
                     passthrough — background `&`, `&&`, `||`, `;`, the four \
                     redirection forms, subshells, brace groups, function \
                     definitions, `!` and `[ … ]` among them. State the boundary as \
                     two lists, not as a universal. Offending paragraph: {}",
                    &para[..para.len().min(200)]
                );
            }
        }
    }
}
