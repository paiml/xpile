//! XPILE-CRATEMETA-001 (PMAT-1465) — the 31 crates' PUBLISHED metadata is a
//! claim surface, and nothing had ever read it.
//!
//! Every workspace member carries a `description` in its `Cargo.toml` and a
//! crate-root `//!` block at the top of its `src/lib.rs`. Those two strings are
//! the crates.io headline and the docs.rs summary for that crate — for 30 of the
//! 31 members they are the ONLY prose a reader who arrives from the registry
//! ever sees, because only `crates/xpile` ships a `readme`. Both are re-uploaded,
//! verbatim, on every Friday publish.
//!
//! At `18d91af8` **no test in the workspace read either one.** The only gate
//! over any `Cargo.toml` is `publish_manifest_integrity.rs`, and it reads
//! `version =` on path-deps and nothing else (`grep -c description` over it: 0).
//! This is PMAT-1459's rule — *ask which published field has no consumer* — one
//! surface further out than PMAT-1464 reached: that slice found the phantom lanes
//! in `docs/assets/hero.svg`, the image README embeds, and wrote down that the
//! next candidates are "any file kind absent from every `walk_*`". `Cargo.toml`
//! was one.
//!
//! ## What was wrong
//!
//! Five distinct falsehoods, each contradicted by a signal this repo already
//! computes:
//!
//! | site | claim | live |
//! |------|-------|------|
//! | `xpile-contract-backend` desc + `//!` | renders contracts as "LaTeX / Lean theorems / **mdBook**" | no ContractBackend claims `ContractFormat::MdBook` |
//! | `xpile-contract-frontend` desc + `//!` | parses "LaTeX / Lean theorem text / **mdBook**" | no ContractFrontend claims it either |
//! | `xpile-backend` desc + `//!` | "**every** target language (Rust, Ruchy, PTX, WGSL, SPIR-V, Lean)" | `Target` has **nine** variants; Wasm, Shell and ForjarYaml are omitted from a sentence that quantifies universally |
//! | `bashrs-frontend` desc | "POSIX shell frontend (sh/bash/zsh **+ Makefile/Dockerfile**)" | both are in that frontend's own `refused_claims()` — `parse_and_lower` REFUSES them (PMAT-1420) |
//! | `bashrs-frontend` `//!` | "the real parser / lowering pipeline is **deferred to v0.2.0**"; "`parse_and_lower` returns a **structurally empty `Module`**" | `lowers_input()` is `true` and the file is a 1 900-line POSIX parser with loops, `if`/`elif`, and `case` (PMAT-085..092, 1268, 1276, 1281, 1283..1285) |
//! | `xpile-backend` `Target::Shell` | shell emit is a "scaffold at v0.1.0; full emit at v0.2.0" | lowering a shell `Module` through the REGISTERED Shell backend emits the commands (executed below) |
//! | `xpile-backend` `Target::ForjarYaml` + `xpile-forjar-codegen` `//!` + its **runtime refusal message** | "the meta-HIR shell lane has no `Stmt::ShellIf`" | `Stmt::ShellIf` exists; this file CONSTRUCTS one |
//!
//! The mdBook pair is the sharpest: `lane_roster_witness.rs:16` states, in as
//! many words, that **"no mdBook contract frontend or backend exists"**, and
//! `mdBook` is one of the four spellings on that gate's own phantom list. Its
//! corpus is `book/src` + `README.md` + `docs/assets/*.svg`. Two crates were
//! shipping the banned claim to crates.io the whole time, outside its walk.
//! Third recurrence of PMAT-1464's lesson — *a corpus of the FILES that mention
//! a lane is not a corpus of the ARTIFACTS that present one* — and here the
//! artifact is the registry page.
//!
//! The `bashrs-frontend` pair is the same shape against a different signal.
//! PMAT-1433 added `Frontend::refused_claims()` precisely so `xpile info` and
//! `book/src/reference/frontends.md` would stop advertising `Makefile` /
//! `Dockerfile` / `*.mk` as things the shell lane handles. It wired the new
//! signal into both of those readers and left the crate's own crates.io
//! description saying `sh/bash/zsh + Makefile/Dockerfile`. **The sweep was
//! narrower than the rule it wrote**, and the surface it missed was the
//! published one.
//!
//! ## Every arm is derived, and three of them PASS on the unmodified corpus
//!
//! No arm here bans a spelling. Each computes its population from a live signal
//! and the corpus supplies its own passing controls — which is what makes the
//! rule a measurement rather than an argument:
//!
//!   * `ContractFormat::Coq` / `Agda` / `Isabelle` are unrenderable exactly like
//!     `MdBook`, and **no** crate metadata presents them. The arm is not simply
//!     "ban mdbook".
//!   * `ruchy-frontend` declares `lowers_input() == false` and its `//!` says
//!     *"routing only; `.ruchy` INPUT refuses"* — a capability denial that is
//!     TRUE, and it stays green.
//!   * `xpile-latex-contract-backend` and `xpile-lean-contract-backend` both
//!     open with *"scaffold stub"* and both report
//!     `renders_contract_body() == false`. Honest, and outside the population
//!     the deferral arm scores.
//!
//! ## What is OUT of subject, and why — measured, not assumed
//!
//!   * **The source-language roster.** The backend arm requires a universal over
//!     target languages to enumerate all nine `Target` variants. The frontend
//!     analogue is not gated: the C frontend's registry name is the single
//!     character `c`, which cannot be matched in prose without fabricating hits
//!     in every other word. `xpile-frontend`'s `//!` names "Python, C, Ruchy,
//!     ..." with an explicit ellipsis, so it is non-exhaustive by construction
//!     rather than false; it is left alone.
//!   * **Backend maturity adjectives.** `xpile-{ptx,ruchy,lean}-codegen` all
//!     describe themselves as a "backend stub". The proof lane got a
//!     machine-readable answer for this in PMAT-1429
//!     (`ContractBackend::renders_contract_body()`, MEASURED by
//!     `proof_lane_scaffold_witness.rs`); the code-lane `Backend` trait has no
//!     twin, so "stub" on a code backend is not decidable here. That asymmetry
//!     is the standing lead this slice files, not a finding it makes.
//!   * **Field-level doc comments.** `ContractFormat::MdBook` is a live enum
//!     variant, and five `///` comments in the contract-lane traits describe the
//!     marker syntax that variant would use. Describing a format the type system
//!     knows about is not the same claim as a crate saying it RENDERS it, so the
//!     subject here is the crate-ROOT `//!` block and the `description` field.
//!     The one exception is the `Target` enum's own doc block, which is a
//!     published per-lane roster of what each backend does and is scored.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use xpile_backend::Target;
use xpile_contracts::ContractFormat;
use xpile_meta_hir::{Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

/// One published metadata string, with enough provenance to name it in a
/// failure. `kind` distinguishes the crates.io headline from the docs.rs one so
/// a report says which registry page carries the claim.
#[derive(Debug, Clone)]
struct Site {
    krate: String,
    kind: &'static str,
    text: String,
}

impl Site {
    /// Lowercased, with every non-alphanumeric run collapsed away. `SPIR-V`,
    /// `SPIR V` and `spirv` all become `spirv`; `forjar.yaml` becomes
    /// `forjaryaml`. Used for token containment so a check never depends on how
    /// prose happens to punctuate a lane name.
    fn squashed(&self) -> String {
        self.text
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    }
}

/// The `description = "…"` value of a manifest, if it declares one.
fn manifest_description(manifest: &str) -> Option<String> {
    for line in manifest.lines() {
        let Some(rest) = line.strip_prefix("description") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim();
        let Some(inner) = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')) else {
            continue;
        };
        return Some(inner.to_string());
    }
    None
}

/// The contiguous `//!` block at the top of a source file, `//!` markers
/// stripped. Blank lines and attributes before it are tolerated; the first
/// non-`//!` code line ends it.
fn crate_root_doc(src: &str) -> String {
    let mut out = String::new();
    let mut started = false;
    for line in src.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("//!") {
            started = true;
            out.push_str(rest.trim_start());
            out.push('\n');
        } else if started || (!t.is_empty() && !t.starts_with("#!")) {
            // Either the block has ended, or a non-`//!`, non-attribute line
            // came first and there is no crate-root doc to collect.
            break;
        }
    }
    out
}

/// Every published metadata string in the workspace: one `description` and one
/// crate-root `//!` per member.
fn corpus() -> Vec<Site> {
    let root = workspace_root();
    let mut sites = Vec::new();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join("crates"))
        .expect("crates/ exists")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    for dir in dirs {
        let krate = dir
            .file_name()
            .and_then(|n| n.to_str())
            .expect("crate dir name is utf-8")
            .to_string();
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).expect("manifest reads");
        if let Some(desc) = manifest_description(&manifest) {
            sites.push(Site {
                krate: krate.clone(),
                kind: "Cargo.toml description (the crates.io headline)",
                text: desc,
            });
        }
        for entry in ["src/lib.rs", "src/main.rs"] {
            let path = dir.join(entry);
            if !path.is_file() {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("source reads");
            let doc = crate_root_doc(&src);
            if !doc.trim().is_empty() {
                sites.push(Site {
                    krate: krate.clone(),
                    kind: "crate-root `//!` (the docs.rs summary)",
                    text: doc,
                });
            }
        }
    }
    sites
}

/// The corpus has to be non-empty and reach both kinds, or every negative below
/// passes over nothing (PMAT-1396: a negative over an empty enumeration is free).
#[test]
fn the_published_metadata_corpus_reaches_every_member_both_ways() {
    let sites = corpus();
    let with_desc: BTreeSet<&str> = sites
        .iter()
        .filter(|s| s.kind.starts_with("Cargo.toml"))
        .map(|s| s.krate.as_str())
        .collect();
    let with_doc: BTreeSet<&str> = sites
        .iter()
        .filter(|s| s.kind.starts_with("crate-root"))
        .map(|s| s.krate.as_str())
        .collect();
    assert!(
        with_desc.len() >= 31,
        "only {} workspace members declare a `description`; every published crate must carry \
         one, and a member missing from this walk is a claim surface this gate cannot see. \
         Reached: {with_desc:?}",
        with_desc.len()
    );
    assert!(
        with_doc.len() >= 30,
        "only {} workspace members expose a crate-root `//!`; that block is the docs.rs \
         summary and a member without one publishes an empty front page. Reached: {with_doc:?}",
        with_doc.len()
    );
    // Two independent spot-anchors, one per kind, so a walk that silently
    // started collecting the wrong thing (an empty string, a `///` block) is
    // caught rather than counted.
    let backend_desc = sites
        .iter()
        .find(|s| s.krate == "xpile-backend" && s.kind.starts_with("Cargo.toml"))
        .expect("xpile-backend declares a description");
    assert!(
        backend_desc.text.contains("Backend trait"),
        "the manifest walk did not read xpile-backend's description; got {:?}",
        backend_desc.text
    );
    let frontend_doc = sites
        .iter()
        .find(|s| s.krate == "xpile-frontend" && s.kind.starts_with("crate-root"))
        .expect("xpile-frontend has a crate-root doc");
    assert!(
        frontend_doc.text.contains("Frontend trait"),
        "the `//!` walk did not read xpile-frontend's crate-root doc; got {:?}",
        frontend_doc.text
    );
}

/// Every `ContractFormat`, pinned exhaustively: the wildcard-free match below
/// fails to COMPILE when a variant is added, so this list cannot silently go
/// stale the way a hand-maintained spelling list does.
fn all_contract_formats() -> Vec<ContractFormat> {
    let all = vec![
        ContractFormat::LatexMath,
        ContractFormat::LeanTheorem,
        ContractFormat::MdBook,
        ContractFormat::Coq,
        ContractFormat::Agda,
        ContractFormat::Isabelle,
    ];
    for f in &all {
        match f {
            ContractFormat::LatexMath
            | ContractFormat::LeanTheorem
            | ContractFormat::MdBook
            | ContractFormat::Coq
            | ContractFormat::Agda
            | ContractFormat::Isabelle => {}
        }
    }
    all
}

/// A format's token as prose would spell it, squashed: `MdBook` → `mdbook`.
fn format_token(f: ContractFormat) -> String {
    format!("{f:?}").to_ascii_lowercase()
}

#[test]
fn no_published_crate_metadata_presents_an_unrenderable_contract_format() {
    let session = xpile_core::default_session();
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    for b in &session.contract_backends {
        for f in b.formats() {
            claimed.insert(format_token(*f));
        }
    }
    for f in &session.contract_frontends {
        for fmt in f.formats() {
            claimed.insert(format_token(*fmt));
        }
    }
    // The population is the complement, derived — NOT a list of names someone
    // decided were phantoms. `lane_roster_witness.rs` bans four spellings it
    // validates against the registry; this arm needs no list at all, so an
    // mdBook backend landing tomorrow lifts the ban by itself.
    let unrenderable: Vec<String> = all_contract_formats()
        .into_iter()
        .map(format_token)
        .filter(|t| !claimed.contains(t))
        .collect();
    assert!(
        claimed.len() >= 2,
        "fewer than two contract formats are claimed by any registered impl ({claimed:?}); the \
         registry walk is not reaching the proof lane and the complement below would accuse \
         everything"
    );
    assert!(
        unrenderable.len() >= 3,
        "every contract format is claimed by some impl, so this arm scores nothing. Live \
         claimed: {claimed:?}"
    );

    let mut offenders = Vec::new();
    for site in corpus() {
        let squashed = site.squashed();
        for token in &unrenderable {
            if squashed.contains(token.as_str()) {
                offenders.push(format!(
                    "{} — {}: presents `{token}`, which no registered ContractBackend or \
                     ContractFrontend claims (live formats: {claimed:?})",
                    site.krate, site.kind
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\na published crate page presents a contract format nothing can render or parse:\n  \
         {}\n\
         `lane_roster_witness.rs` already bans this claim across `book/src`, `README.md` and \
         `docs/assets/*.svg`; a crates.io description is the same claim on the page most \
         readers actually land on.",
        offenders.join("\n  ")
    );
}

/// Every `Target`, pinned exhaustively the same way as the formats above.
fn all_targets() -> Vec<Target> {
    let all = vec![
        Target::Rust,
        Target::Ruchy,
        Target::Ptx,
        Target::Wgsl,
        Target::Spirv,
        Target::Wasm,
        Target::Lean,
        Target::Shell,
        Target::ForjarYaml,
    ];
    for t in &all {
        match t {
            Target::Rust
            | Target::Ruchy
            | Target::Ptx
            | Target::Wgsl
            | Target::Spirv
            | Target::Wasm
            | Target::Lean
            | Target::Shell
            | Target::ForjarYaml => {}
        }
    }
    all
}

fn target_token(t: Target) -> String {
    format!("{t:?}").to_ascii_lowercase()
}

#[test]
fn a_universal_over_target_languages_enumerates_the_whole_registry() {
    // A sentence that says "every target language" and then opens a parenthesis
    // has made two claims: a universal and a census. PMAT-1458's shape — only
    // the census ages, and the universal half is what a reader checks.
    let targets = all_targets();
    assert_eq!(
        targets.len(),
        9,
        "the Target roster changed; re-derive the expectation below rather than editing this \
         number in isolation"
    );
    let mut offenders = Vec::new();
    let mut scored = 0usize;
    for site in corpus() {
        let squashed = site.squashed();
        if !squashed.contains("everytargetlanguage") {
            continue;
        }
        scored += 1;
        let missing: Vec<String> = targets
            .iter()
            .map(|t| target_token(*t))
            .filter(|tok| !squashed.contains(tok.as_str()))
            .collect();
        if !missing.is_empty() {
            offenders.push(format!(
                "{} — {}: quantifies over EVERY target language but omits {missing:?} from its \
                 enumeration",
                site.krate, site.kind
            ));
        }
    }
    assert!(
        scored >= 2,
        "no published metadata quantifies over the target languages, so this arm scored nothing \
         ({scored} site(s) seen). The claim lived in xpile-backend's description AND its \
         crate-root doc; if both were reworded away, drop this arm deliberately rather than \
         letting it pass empty."
    );
    assert!(
        offenders.is_empty(),
        "\na published crate page claims to cover EVERY target language and then lists a \
         subset:\n  {}\n\
         The live roster is {:?}. Name all of them, or stop quantifying.",
        offenders.join("\n  "),
        targets.iter().map(|t| target_token(*t)).collect::<Vec<_>>()
    );
}

/// Registry name → the crate that ships that frontend. Nothing in the tree links
/// them (`python` ships from `depyler-frontend`, `c` from `decy-frontend`), so
/// the map is explicit — and asserted exhaustive, so a new frontend reds here
/// until it is listed rather than silently escaping the two arms below.
fn frontend_crate(name: &str) -> Option<&'static str> {
    match name {
        "python" => Some("depyler-frontend"),
        "c" => Some("decy-frontend"),
        "ruchy" => Some("ruchy-frontend"),
        "bashrs" => Some("bashrs-frontend"),
        "wasm" => Some("xpile-wasm-frontend"),
        _ => None,
    }
}

#[test]
fn no_crate_page_advertises_an_input_its_own_frontend_refuses() {
    let session = xpile_core::default_session();
    let sites = corpus();
    let mut offenders = Vec::new();
    let mut scored = 0usize;
    for f in &session.frontends {
        let krate = frontend_crate(f.name()).unwrap_or_else(|| {
            panic!(
                "frontend `{}` is registered but not mapped to a crate — add it to \
                 `frontend_crate` so its published page is scored",
                f.name()
            )
        });
        // `refused_claims()` (PMAT-1433) is the frontend's own statement of the
        // spellings it ROUTES and then REFUSES. It exists because `xpile info`
        // was printing them flush with the ones that work.
        for claim in f.refused_claims() {
            let token: String = claim
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            if token.is_empty() {
                continue;
            }
            scored += 1;
            for site in sites.iter().filter(|s| s.krate == krate) {
                if !site.squashed().contains(token.as_str()) {
                    continue;
                }
                // A page may NAME a refused spelling to say it refuses. It may
                // not present it as something the crate handles. The distinction
                // is the denial, and it has to be in the same string.
                let lower = site.text.to_ascii_lowercase();
                if lower.contains("refus") || lower.contains("not handled") {
                    continue;
                }
                offenders.push(format!(
                    "{krate} — {}: advertises `{claim}`, which this frontend's own \
                     `refused_claims()` lists and `parse_and_lower` REFUSES",
                    site.kind
                ));
            }
        }
    }
    assert!(
        scored >= 3,
        "no registered frontend declares a `refused_claims()` entry, so this arm scored nothing \
         ({scored}). PMAT-1433 put three there (`*.mk`, `Makefile`, `Dockerfile`); if the \
         Makefile dialect landed, this arm needs re-aiming, not deleting."
    );
    assert!(
        offenders.is_empty(),
        "\na published crate page advertises an input the crate refuses:\n  {}\n\
         `refused_claims()` was added (PMAT-1433) so `xpile info` and \
         `book/src/reference/frontends.md` would stop doing exactly this. The registry page is \
         the third reader.",
        offenders.join("\n  ")
    );
}

/// Spellings that defer a capability to a later release. Scored ONLY against
/// crates whose live signal says the capability is already there, so an honest
/// deferral (`ruchy-frontend`, the two contract-backend scaffolds) is outside
/// the population rather than exempted by a branch.
const DEFERRALS: &[&str] = &[
    "structurally empty",
    "deferred to v0.2.0",
    "scaffold",
    "placeholder",
];

#[test]
fn no_crate_page_defers_a_capability_its_frontend_already_has() {
    let session = xpile_core::default_session();
    let sites = corpus();
    let mut offenders = Vec::new();
    let mut capable = 0usize;
    let mut denying = 0usize;
    for f in &session.frontends {
        let krate = frontend_crate(f.name()).expect("every frontend is mapped");
        // PMAT-1346's signal, MEASURED by `claims_drift.rs`, which runs each
        // frontend against a real program in its own language and reds if what
        // it did disagrees with what it declared here.
        if !f.lowers_input() {
            denying += 1;
            continue;
        }
        capable += 1;
        for site in sites.iter().filter(|s| s.krate == krate) {
            let lower = site.text.to_ascii_lowercase();
            for d in DEFERRALS {
                if lower.contains(d) {
                    offenders.push(format!(
                        "{krate} — {}: says {d:?} while `lowers_input()` is true",
                        site.kind
                    ));
                }
            }
        }
    }
    assert!(
        capable >= 3 && denying >= 1,
        "the population split is degenerate ({capable} capable / {denying} denying). This arm is \
         only a measurement while BOTH sides are occupied — `ruchy-frontend` is the denying side \
         and its `//!` really does say `routing only`, which is what proves the needle is not \
         simply banning the word."
    );
    assert!(
        offenders.is_empty(),
        "\na published crate page defers a capability the crate already has:\n  {}\n\
         `lowers_input()` is the live answer and `claims_drift.rs` measures it by RUNNING the \
         frontend. A registry page that says the parser is still coming is the one claim a \
         reader cannot check.",
        offenders.join("\n  ")
    );
}

#[test]
fn the_shell_target_doc_does_not_defer_an_emit_the_backend_performs() {
    // EXECUTED, not asserted: lower a shell module through whatever backend the
    // live session registers for `Target::Shell` and look at what comes out.
    let session = xpile_core::default_session();
    let backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&Target::Shell))
        .expect("a backend is registered for Target::Shell");
    let module = Module {
        name: "cratemeta_probe".into(),
        source_lang: SourceLang::Shell,
        items: vec![Item::Function(Function {
            name: "main".into(),
            params: vec![],
            return_type: Type::I64,
            body: Block {
                stmts: vec![Stmt::Cmd {
                    program: "echo".into(),
                    args: vec![Expr::LitStr("xpile-cratemeta-probe".into())],
                }],
                trailing_return: Expr::LitInt(0),
            },
        })],
        ffi_boundaries: vec![],
    };
    let cfg = xpile_backend::BackendConfig {
        target: Target::Shell,
        profile: xpile_backend::Profile::RustOut,
        hardware: None,
        emit_contracts: false,
    };
    let artifact = backend
        .lower(&module, &cfg)
        .expect("the registered shell backend lowers a shell module");
    let emitted = artifact.primary.clone();
    // Input-dependence, PMAT-1388's rule: the probe token must survive into the
    // artifact, so a backend that returned a fixed placeholder comment cannot
    // satisfy this by accident.
    assert!(
        emitted.contains("echo") && emitted.contains("xpile-cratemeta-probe"),
        "the registered Target::Shell backend did not emit the probe command; if the shell \
         emit really did regress to a placeholder, the deferral prose below becomes true again \
         and this whole test should be re-derived, not deleted.\n---\n{emitted}\n---"
    );

    // It emits. So no published page may say the emit is still to come.
    let root = workspace_root();
    let backend_src = std::fs::read_to_string(root.join("crates/xpile-backend/src/lib.rs"))
        .expect("xpile-backend source reads");
    let shell_doc = doc_block_for_variant(&backend_src, "Shell");
    assert!(
        !shell_doc.trim().is_empty(),
        "no doc block was extracted for `Target::Shell` — the variant was renamed or the \
         extractor broke, and the loop below would pass over an empty string"
    );
    for d in DEFERRALS {
        assert!(
            !shell_doc.to_ascii_lowercase().contains(d),
            "the `Target::Shell` doc says {d:?}, but the registered backend just emitted the \
             probe command. That doc block is the published per-lane roster on docs.rs.\n---\n\
             {shell_doc}\n---"
        );
    }
}

/// The `///` lines immediately preceding `    <Variant>,` in an enum body.
fn doc_block_for_variant(src: &str, variant: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let needle = format!("{variant},");
    let Some(idx) = lines
        .iter()
        .position(|l| l.trim() == needle && !l.starts_with("//"))
    else {
        return String::new();
    };
    let mut doc: Vec<&str> = Vec::new();
    for i in (0..idx).rev() {
        let t = lines[i].trim_start();
        match t.strip_prefix("///") {
            Some(rest) => doc.push(rest.trim_start()),
            None => break,
        }
    }
    doc.reverse();
    doc.join("\n")
}

#[test]
fn nothing_published_denies_a_meta_hir_variant_that_exists() {
    // The existence half is proven by the COMPILER: this value only builds
    // while `Stmt::ShellIf` is a live variant. Nothing to keep in sync.
    let live = Stmt::ShellIf {
        cond: Expr::LitStr("[ -f x ]".into()),
        then_body: vec![],
        else_body: vec![],
    };
    assert!(
        matches!(live, Stmt::ShellIf { .. }),
        "constructed a Stmt::ShellIf that does not match itself"
    );

    // Three sites said the shell lane HAS no `Stmt::ShellIf`: the
    // `Target::ForjarYaml` doc, the forjar crate's `//!`, and — worst — the
    // refusal message the binary PRINTS when it declines a conditional. The
    // refusal itself is correct (forjar has no conditional resource); the reason
    // it gave was a fact about meta-HIR, and that fact was false.
    let root = workspace_root();
    let denial = "no `Stmt::ShellIf`";
    let mut offenders = Vec::new();
    let mut scanned = 0usize;
    for rel in [
        "crates/xpile-backend/src/lib.rs",
        "crates/xpile-forjar-codegen/src/lib.rs",
    ] {
        let body = std::fs::read_to_string(root.join(rel)).expect("source reads");
        scanned += 1;
        // Whitespace-insensitive: the message is wrapped across a string
        // continuation, so a line-local scan misses it entirely.
        let flat: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
        let flat_needle: String = denial.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.contains(&flat_needle) {
            offenders.push(format!(
                "{rel}: states the shell lane has no `Stmt::ShellIf`, a variant this test just \
                 constructed"
            ));
        }
    }
    assert_eq!(scanned, 2, "the two-file scan did not open both files");
    assert!(
        offenders.is_empty(),
        "\na published page (and a runtime refusal message) denies a meta-HIR variant that \
         exists:\n  {}\n\
         `Stmt::ShellIf` landed with PMAT-1283/1284. Say what forjar cannot represent, not what \
         meta-HIR does not have.",
        offenders.join("\n  ")
    );
}
