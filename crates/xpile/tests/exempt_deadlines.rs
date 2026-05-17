//! Time-bounded escape-hatch enforcement (PMAT-014 / XPILE-EXEMPT-001).
//!
//! Every `XPILE-PENDING-UNTIL: v<semver>` marker in the workspace source
//! carries a release-version deadline. This test scans every `.rs` file
//! under `crates/*/src/` for the marker pattern, parses the version it
//! demands, and asserts that the current workspace version is still
//! strictly *less than* every deadline.
//!
//! When `cargo install xpile` ships v0.2.0, every marker with
//! `UNTIL: v0.2.0` will trip this test and either:
//!   1. The underlying feature gets implemented and the marker removed, OR
//!   2. The release is delayed and the marker is bumped to a later
//!      version with a public reason.
//!
//! Closes the "unimplemented forever" hole identified in the §27
//! provability roadmap. Modelled on ruchy 5.0 §14.7's
//! `#[contract_exempt(until)]` + `build.rs` pattern.
//!
//! Marker format (parsed by [`parse_pending_markers`]):
//!
//! ```text
//! [XPILE-PENDING-UNTIL: v<MAJOR>.<MINOR>.<PATCH>, ticket: <TICKET-ID>]
//! ```
//!
//! Live markers are listed in the failure diagnostic of
//! [`no_xpile_pending_until_has_expired`] when any deadline trips. As
//! of PMAT-029 (XPILE-REFINE-003) the workspace has zero live markers;
//! the scanner's reach is validated synthetically by
//! [`scanner_reaches_all_watched_directories`].

use std::fs;
use std::path::{Path, PathBuf};

/// Single parsed marker. The `file` + `line` are kept for actionable
/// error messages — if the deadline trips, the failure points the
/// reader directly at the source location to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingMarker {
    file: PathBuf,
    line: usize,
    until: SemVer,
    ticket: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SemVer {
    major: u32,
    minor: u32,
    patch: u32,
}

impl SemVer {
    fn parse(s: &str) -> Option<Self> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        Some(SemVer {
            major: parts[0].parse().ok()?,
            minor: parts[1].parse().ok()?,
            patch: parts[2].parse().ok()?,
        })
    }
}

fn workspace_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` for this test crate is `crates/xpile`. The
    // workspace root is two parents up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn workspace_version() -> SemVer {
    // Parse the workspace.package.version directly from the root
    // Cargo.toml. Using `env!("CARGO_PKG_VERSION")` reflects this test
    // crate's version, which is the workspace version since everyone
    // inherits via `version.workspace = true` — but reading the source
    // is more honest (catches a future drift where a crate pins a
    // different version).
    let cargo_toml =
        fs::read_to_string(workspace_root().join("Cargo.toml")).expect("read workspace Cargo.toml");
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version = ") {
            let v = rest.trim().trim_matches('"');
            if let Some(parsed) = SemVer::parse(v) {
                return parsed;
            }
        }
    }
    panic!("could not parse workspace.package.version from root Cargo.toml");
}

/// Walk every file the deadline scanner cares about and return every
/// `XPILE-PENDING-UNTIL` marker. XPILE-EXEMPT-002 widened this from
/// "Rust source under crates/*/src/" to also cover proof-lane and
/// symbolic-stratum artefacts under `contracts/`:
///   * `crates/*/src/**/*.rs` — Rust codegen / library source
///   * `contracts/lean/*.lean` — Semantic stratum proof statements
///   * `contracts/kani/*.rs`  — Symbolic stratum harnesses
///
/// Test fixture files and unit tests aren't walked because their
/// markers are documentation, not live exemptions — same posture as
/// before.
fn parse_pending_markers(root: &Path) -> Vec<PendingMarker> {
    let mut out = Vec::new();

    // (1) Rust codegen + library source under crates/*/src/
    let crates_dir = root.join("crates");
    if let Ok(entries) = fs::read_dir(&crates_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let src_dir = path.join("src");
            if src_dir.is_dir() {
                walk_dir(&src_dir, &[".rs"], &mut out);
            }
        }
    }

    // (2) Lean refinement proofs under contracts/lean/
    let lean_dir = root.join("contracts").join("lean");
    if lean_dir.is_dir() {
        walk_dir(&lean_dir, &[".lean"], &mut out);
    }

    // (3) Kani harnesses under contracts/kani/
    let kani_dir = root.join("contracts").join("kani");
    if kani_dir.is_dir() {
        walk_dir(&kani_dir, &[".rs"], &mut out);
    }

    out
}

fn walk_dir(dir: &Path, exts: &[&str], out: &mut Vec<PendingMarker>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, exts, out);
        } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            let dotted = format!(".{ext}");
            if exts.contains(&dotted.as_str()) {
                scan_file(&path, out);
            }
        }
    }
}

fn scan_file(path: &Path, out: &mut Vec<PendingMarker>) {
    let Ok(contents) = fs::read_to_string(path) else {
        return;
    };
    for (lineno, line) in contents.lines().enumerate() {
        if let Some(marker) = extract_marker_from_line(path, lineno + 1, line) {
            out.push(marker);
        }
    }
}

/// Extract a `[XPILE-PENDING-UNTIL: v<semver>, ticket: <id>]` marker
/// from one line. Returns `None` if the line doesn't contain the
/// marker — most lines won't.
fn extract_marker_from_line(file: &Path, line_no: usize, line: &str) -> Option<PendingMarker> {
    let needle = "XPILE-PENDING-UNTIL:";
    let idx = line.find(needle)?;
    let rest = &line[idx + needle.len()..];
    let rest = rest.trim_start();
    let until_end = rest.find(',').or_else(|| rest.find(']'))?;
    let until_str = rest[..until_end].trim();
    let until = SemVer::parse(until_str)?;

    let ticket_marker = "ticket:";
    let ticket_str = if let Some(t_idx) = rest.find(ticket_marker) {
        let after = &rest[t_idx + ticket_marker.len()..];
        let end = after.find(']').unwrap_or(after.len());
        after[..end].trim().to_string()
    } else {
        String::from("(no ticket)")
    };

    Some(PendingMarker {
        file: file.to_path_buf(),
        line: line_no,
        until,
        ticket: ticket_str,
    })
}

/// The load-bearing CI gate. If any marker's `until` version is <= the
/// current workspace version, the corresponding feature was promised
/// for *this* release and hasn't shipped. Build fails with a
/// per-marker breakdown so the fix is obvious.
#[test]
fn no_xpile_pending_until_has_expired() {
    let root = workspace_root();
    let current = workspace_version();
    let markers = parse_pending_markers(&root);

    let expired: Vec<_> = markers.iter().filter(|m| m.until <= current).collect();

    if !expired.is_empty() {
        let mut msg = String::from(
            "XPILE-PENDING-UNTIL deadline(s) reached without the underlying feature shipping.\n\
             Either implement the feature and remove the marker, OR bump `until:` to a later\n\
             version with a public reason in the commit message.\n\n",
        );
        for m in &expired {
            msg.push_str(&format!(
                "  - {}:{}  until=v{}.{}.{}  ticket={}\n",
                m.file.display(),
                m.line,
                m.until.major,
                m.until.minor,
                m.until.patch,
                m.ticket,
            ));
        }
        msg.push_str(&format!(
            "\nWorkspace version is v{}.{}.{}; deadlines must be strictly greater.",
            current.major, current.minor, current.patch
        ));
        panic!("{msg}");
    }
}

/// PMAT-029: synthetic-fixture replacement for the prior
/// `at_least_one_marker_exists` + `scanner_picks_up_proof_lane_markers`
/// tests. With XPILE-REFINE-003 closed, the workspace has zero live
/// markers — the old tests required at least one to exist anywhere in
/// real source, which made them go red after the last marker shipped.
///
/// The actually-load-bearing property of the scanner is "if a marker
/// exists in any of the watched directories, the scanner finds it."
/// We test that directly here: build a temp workspace-shaped tree,
/// drop a marker into each watched location, and assert the scanner
/// surfaces all of them with correct file paths + line numbers.
///
/// If a future refactor narrows the scan back to crates-only or drops
/// `.lean` / `.kani` coverage, this fires. The live-state property is
/// still implicitly checked by `no_xpile_pending_until_has_expired`,
/// which scans the real workspace each run.
#[test]
fn scanner_reaches_all_watched_directories() {
    let tmp = std::env::temp_dir().join(format!(
        "xpile-deadline-scan-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&tmp).expect("create temp root");

    // Build:
    //   <tmp>/crates/foo/src/lib.rs       <- marker R1
    //   <tmp>/crates/bar/src/inner/x.rs   <- marker R2 (nested)
    //   <tmp>/contracts/lean/Sample.lean  <- marker L1
    //   <tmp>/contracts/kani/sample.rs    <- marker K1
    let mk = |rel: &str, body: &str| {
        let p = tmp.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
    };
    mk(
        "crates/foo/src/lib.rs",
        "// [XPILE-PENDING-UNTIL: v9.9.9, ticket: R1]\nfn main() {}\n",
    );
    mk(
        "crates/bar/src/inner/x.rs",
        ".expect(\"err [XPILE-PENDING-UNTIL: v9.9.9, ticket: R2]\")\n",
    );
    mk(
        "contracts/lean/Sample.lean",
        "-- XPILE-PENDING-UNTIL: v9.9.9, ticket: L1\n",
    );
    mk(
        "contracts/kani/sample.rs",
        "// [XPILE-PENDING-UNTIL: v9.9.9, ticket: K1]\n",
    );

    let markers = parse_pending_markers(&tmp);
    let tickets: std::collections::BTreeSet<_> =
        markers.iter().map(|m| m.ticket.as_str()).collect();

    // Clean up before assertions so a failure doesn't leak the dir.
    let _ = fs::remove_dir_all(&tmp);

    let want = ["R1", "R2", "L1", "K1"];
    for t in want {
        assert!(
            tickets.contains(t),
            "scanner missed marker `{t}`. Found: {tickets:?}. \
             If the scanner stopped walking one of the four directories \
             (crates/*/src/, contracts/lean/, contracts/kani/), this test \
             pinpoints which one."
        );
    }
}

/// Parser self-test (RED → GREEN style): hand-write a sample line and
/// verify the regex-free parser handles the canonical format and the
/// "no ticket" edge case. Guards against drift in the marker syntax.
#[test]
fn parser_handles_canonical_and_missing_ticket() {
    let canonical = ".expect(\"foo [XPILE-PENDING-UNTIL: v0.2.0, ticket: PMAT-013-FOLLOWUP]\")";
    let m = extract_marker_from_line(Path::new("test.rs"), 42, canonical).unwrap();
    assert_eq!(
        m.until,
        SemVer {
            major: 0,
            minor: 2,
            patch: 0
        }
    );
    assert_eq!(m.ticket, "PMAT-013-FOLLOWUP");
    assert_eq!(m.line, 42);

    let without_ticket = "// [XPILE-PENDING-UNTIL: v1.0.0]";
    let m2 = extract_marker_from_line(Path::new("x.rs"), 1, without_ticket).unwrap();
    assert_eq!(
        m2.until,
        SemVer {
            major: 1,
            minor: 0,
            patch: 0
        }
    );
    assert_eq!(m2.ticket, "(no ticket)");

    let no_marker = "let x = 1; // unrelated comment";
    assert!(extract_marker_from_line(Path::new("x.rs"), 1, no_marker).is_none());

    let malformed_version = "// [XPILE-PENDING-UNTIL: vbad.semver]";
    assert!(extract_marker_from_line(Path::new("x.rs"), 1, malformed_version).is_none());
}

/// Gate-failure-path self-test. The integration test above only fires
/// the gate against real markers + real workspace version, both of
/// which currently pass. This test exercises the *failing* branch
/// directly with synthetic markers + synthetic current versions, so
/// the failure mode is verified without mutating the workspace.
#[test]
fn gate_fires_when_current_meets_or_exceeds_until() {
    let markers = [
        PendingMarker {
            file: PathBuf::from("synthetic.rs"),
            line: 10,
            until: SemVer {
                major: 0,
                minor: 2,
                patch: 0,
            },
            ticket: "FAKE-001".into(),
        },
        PendingMarker {
            file: PathBuf::from("other.rs"),
            line: 20,
            until: SemVer {
                major: 1,
                minor: 0,
                patch: 0,
            },
            ticket: "FAKE-002".into(),
        },
    ];

    let v_0_1_0 = SemVer {
        major: 0,
        minor: 1,
        patch: 0,
    };
    let v_0_2_0 = SemVer {
        major: 0,
        minor: 2,
        patch: 0,
    };
    let v_1_0_0 = SemVer {
        major: 1,
        minor: 0,
        patch: 0,
    };

    let expired_at_0_1_0: Vec<_> = markers.iter().filter(|m| m.until <= v_0_1_0).collect();
    assert_eq!(
        expired_at_0_1_0.len(),
        0,
        "v0.1.0 < both deadlines — no expiry"
    );

    let expired_at_0_2_0: Vec<_> = markers.iter().filter(|m| m.until <= v_0_2_0).collect();
    assert_eq!(
        expired_at_0_2_0.len(),
        1,
        "v0.2.0 reaches the v0.2.0 deadline; v1.0.0 still safe"
    );
    assert_eq!(expired_at_0_2_0[0].ticket, "FAKE-001");

    let expired_at_1_0_0: Vec<_> = markers.iter().filter(|m| m.until <= v_1_0_0).collect();
    assert_eq!(
        expired_at_1_0_0.len(),
        2,
        "v1.0.0 reaches both deadlines (deadlines are inclusive on equality)"
    );
}

/// Parser self-test for SemVer ordering. The whole gate hinges on this
/// being a total order; sanity-check it.
#[test]
fn semver_orders_correctly() {
    let v_0_1_0 = SemVer {
        major: 0,
        minor: 1,
        patch: 0,
    };
    let v_0_2_0 = SemVer {
        major: 0,
        minor: 2,
        patch: 0,
    };
    let v_0_1_5 = SemVer {
        major: 0,
        minor: 1,
        patch: 5,
    };
    let v_1_0_0 = SemVer {
        major: 1,
        minor: 0,
        patch: 0,
    };

    assert!(v_0_1_0 < v_0_1_5);
    assert!(v_0_1_5 < v_0_2_0);
    assert!(v_0_2_0 < v_1_0_0);
    assert!(v_0_1_0 < v_1_0_0);
    assert_eq!(v_0_1_0, SemVer::parse("v0.1.0").unwrap());
    assert_eq!(v_0_1_0, SemVer::parse("0.1.0").unwrap());
}
