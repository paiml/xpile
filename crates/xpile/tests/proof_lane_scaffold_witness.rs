//! XPILE-PROOFLANE-001 (PMAT-1429) — the proof lane's `xpile info` listing
//! must be as honest as the code lane's.
//!
//! ## The defect this locks out
//!
//! PMAT-1346 established the rule for the CODE lane: a registered-but-hollow
//! entry "must not be silently counted among the languages xpile READS", so
//! `xpile info` prints `frontends (5 registered, 4 lowering)` and tags the
//! routing-only `ruchy` frontend with `[routing only — INPUT refuses, no
//! parser]`.
//!
//! The PROOF lane, printed by the *same function* fourteen lines below, never
//! got that sweep. It advertised
//!
//! ```text
//!   contract_backends (2):
//!     - lean-theorem → LeanTheorem
//!     - latex → LatexMath
//! ```
//!
//! at exit 0, at full parity with the nine real code-lane backends — while
//! BOTH implementations return a fixed payload:
//!
//! ```text
//! theorem _scaffold : True := True.intro
//! ```
//!
//! for every contract they are handed. `README.md` and
//! `book/src/reference/cli.md` both point at this listing as the live
//! capability surface ("Use this to confirm your install can see every lane"),
//! so the listing is a claim surface like any other.
//!
//! ## Why this gate MEASURES instead of asserting
//!
//! `ContractBackend::renders_contract_body()` is a SELF-REPORT, exactly like
//! `Frontend::lowers_input()`. A self-report that nothing checks is how the
//! stale `// current 444` comments got there. So this file does not take the
//! flag's word for it: it renders two contracts that differ in EVERY field
//! [`Contract`] carries, normalises each output against its own id, and
//! compares. It then fails in BOTH directions —
//!
//!   * a backend whose `primary` is contract-independent MUST report `false`
//!     (catches a scaffold that quietly claims to be real), and
//!   * a backend that reports `false` MUST actually be contract-independent
//!     (so when someone makes `latex` real, this gate forces the flag to
//!     flip rather than letting `xpile info` under-report a shipped feature).
//!
//! The scope of the claim is deliberately narrow and stated here rather than
//! implied: the flag is about `RenderedDoc.primary`, the rendered document
//! body — which is what a reader of `xpile info` is being told about. Both
//! scaffolds DO thread `contract.id` and `contract.depends_on` into
//! `RenderedDoc.citations`; the citation chain is not what is hollow.

use xpile_contract_backend::{ContractBackend, ContractRenderConfig};
use xpile_contracts::{
    Contract, ContractFormat, ContractId, XpileContractLane, XpileContractLayer,
};
use xpile_core::default_session;

/// Two contracts differing in every field `Contract` carries. A backend that
/// renders the contract at all has to produce different bodies for these.
fn probe_contracts() -> (Contract, Contract) {
    let a = Contract {
        id: ContractId::new("C-PROOFLANE-PROBE-ALPHA"),
        layer: XpileContractLayer::LanguageSemantics,
        lane: XpileContractLane::Code,
        depends_on: vec![ContractId::new("C-PROOFLANE-DEP-ONE")],
        references: vec!["alpha-reference-key".to_string()],
    };
    let b = Contract {
        id: ContractId::new("C-PROOFLANE-PROBE-BETA"),
        layer: XpileContractLayer::CompileTime,
        lane: XpileContractLane::Proof,
        depends_on: vec![
            ContractId::new("C-PROOFLANE-DEP-TWO"),
            ContractId::new("C-PROOFLANE-DEP-THREE"),
        ],
        references: vec!["beta-reference-key".to_string(), "beta-second".to_string()],
    };
    (a, b)
}

fn render_config(format: ContractFormat) -> ContractRenderConfig {
    ContractRenderConfig {
        format,
        embed_citation: true,
        include_falsification: true,
        lean_version: Some((4, 0)),
    }
}

/// Render `contract` and blank out its own id, so what remains is everything
/// the backend derived from the contract BEYOND a verbatim id substitution.
fn body_modulo_id(backend: &dyn ContractBackend, contract: &Contract) -> String {
    let format = backend.formats()[0];
    let doc = backend
        .render(contract, &render_config(format))
        .unwrap_or_else(|e| panic!("backend `{}` failed to render: {e}", backend.name()));
    doc.primary.replace(contract.id.as_str(), "<ID>")
}

/// Is this backend's rendered BODY actually a function of the contract?
fn measured_contract_dependence(backend: &dyn ContractBackend) -> bool {
    let (a, b) = probe_contracts();
    body_modulo_id(backend, &a) != body_modulo_id(backend, &b)
}

/// The load-bearing assertion: the self-report matches the measurement, in
/// BOTH directions, for every registered contract backend.
#[test]
fn every_contract_backend_self_report_matches_its_measured_behaviour() {
    let session = default_session();
    assert!(
        !session.contract_backends.is_empty(),
        "no contract backends registered — this gate would pass vacuously"
    );

    for cb in &session.contract_backends {
        let measured = measured_contract_dependence(cb.as_ref());
        let claimed = cb.renders_contract_body();
        assert_eq!(
            claimed,
            measured,
            "contract backend `{}` reports renders_contract_body() == {claimed}, but \
             rendering two contracts that differ in EVERY field of `Contract` \
             (id, layer, lane, depends_on, references) produced {} bodies. \
             {}",
            cb.name(),
            if measured { "different" } else { "IDENTICAL" },
            if measured {
                "It renders the contract — flip the flag to `true` so `xpile info` \
                 stops under-reporting a shipped capability."
            } else {
                "It is a scaffold — flip the flag to `false` so `xpile info` stops \
                 advertising it at parity with the real backends (PMAT-1429)."
            },
        );
    }
}

/// The CLI marker names a specific payload (`_scaffold`). Pin the marker's
/// WORDS to the artifact, so the tag cannot drift away from what is emitted.
#[test]
fn every_scaffold_backend_actually_emits_the_scaffold_marker_it_is_tagged_with() {
    let session = default_session();
    let (probe, _) = probe_contracts();

    let mut scaffolds = 0;
    for cb in &session.contract_backends {
        if cb.renders_contract_body() {
            continue;
        }
        scaffolds += 1;
        let doc = cb
            .render(&probe, &render_config(cb.formats()[0]))
            .expect("scaffold render");
        assert!(
            doc.primary.contains("_scaffold"),
            "contract backend `{}` is tagged `[scaffold — fixed `_scaffold` payload]` \
             in `xpile info`, but its output contains no `_scaffold` marker. Either \
             the tag or the payload is wrong:\n{}",
            cb.name(),
            doc.primary,
        );
    }
    assert!(
        scaffolds > 0,
        "no scaffold contract backends found — if the proof lane became real, delete \
         this test rather than letting it pass over an empty set"
    );
}

/// `xpile info` is the claim surface. Tie its rendered text to the MEASURED
/// registry, not to a hard-coded expectation.
#[test]
fn xpile_info_reports_the_measured_proof_lane_honestly() {
    let session = default_session();
    let registered = session.contract_backends.len();
    let rendering = session
        .contract_backends
        .iter()
        .filter(|b| b.renders_contract_body())
        .count();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("info")
        .output()
        .expect("running `xpile info`");
    assert!(out.status.success(), "`xpile info` must exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    // The count line must state BOTH numbers whenever they differ — the
    // PMAT-1346 shape. A bare `contract_backends (2):` over a lane where 0
    // render is the exact under-report this gate exists to stop.
    let expected_header = if rendering == registered {
        format!("  contract_backends ({registered}):")
    } else {
        format!("  contract_backends ({registered} registered, {rendering} rendering):")
    };
    assert!(
        stdout.contains(&expected_header),
        "`xpile info` must print `{expected_header}` for the measured registry \
         ({registered} registered, {rendering} rendering). Got:\n{stdout}"
    );

    // Every non-rendering backend's own line must carry the marker, and every
    // rendering one must NOT — so the tag cannot be sprayed over the lane.
    for cb in &session.contract_backends {
        let line = stdout
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("- {} →", cb.name())))
            .unwrap_or_else(|| {
                panic!(
                    "`xpile info` printed no line for contract backend `{}`:\n{stdout}",
                    cb.name()
                )
            });
        let tagged = line.contains("[scaffold");
        assert_eq!(
            tagged,
            !cb.renders_contract_body(),
            "`xpile info` line for contract backend `{}` is {}tagged as a scaffold, \
             but renders_contract_body() == {}. Line: {line}",
            cb.name(),
            if tagged { "" } else { "NOT " },
            cb.renders_contract_body(),
        );
    }
}
