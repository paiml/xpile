//! Quarterly SOTA-gap dossier deadline gate (PMAT-016 / XPILE-SOTA-001).
//!
//! `docs/specifications/audit-design.md` carries a `Next Dossier
//! Deadline: YYYY-MM-DD` line in its §0 "Cadence" section. This test
//! parses that date and fails CI the moment current wall-clock time
//! reaches it — closing the procedural-stagnation hole from
//! `sub/provability-roadmap.md` §1.6 (falsifier F6 fires automatically
//! on missing dossier).
//!
//! When the deadline trips, the workflow is:
//!   1. Author the dossier (new §6+ subsection in audit-design.md).
//!   2. Bump the "Next Dossier Deadline:" line to the *next* quarter
//!      per the schedule in §0 (2026-Q3 → 2026-11-15, etc.).
//!   3. CI passes again because current time is now < new deadline.
//!
//! Missing the deadline ⇒ this test fails ⇒ release blocked ⇒
//! the slip is visible, not silent. That's the whole mechanism.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Parse the `Next Dossier Deadline: YYYY-MM-DD` line out of
/// audit-design.md. Returns `(year, month, day)`.
fn parse_next_deadline_date() -> (u32, u32, u32) {
    let audit = fs::read_to_string(workspace_root().join("docs/specifications/audit-design.md"))
        .expect("read audit-design.md");
    let prefix = "**Next Dossier Deadline: ";
    let idx = audit.find(prefix).unwrap_or_else(|| {
        panic!("audit-design.md must contain a `**Next Dossier Deadline: YYYY-MM-DD**` line in §0")
    });
    let rest = &audit[idx + prefix.len()..];
    let end = rest
        .find("**")
        .expect("deadline line malformed (no closing **)");
    let date_str = rest[..end].trim();
    let parts: Vec<&str> = date_str.split('-').collect();
    assert_eq!(
        parts.len(),
        3,
        "deadline must be YYYY-MM-DD, got `{date_str}`"
    );
    let y: u32 = parts[0].parse().expect("year");
    let m: u32 = parts[1].parse().expect("month");
    let d: u32 = parts[2].parse().expect("day");
    assert!((1..=12).contains(&m), "month out of range: {m}");
    assert!((1..=31).contains(&d), "day out of range: {d}");
    (y, m, d)
}

/// Convert a Gregorian (year, month, day) at 00:00:00 UTC to seconds
/// since the Unix epoch. Pure-Rust, no chrono dep — the algorithm is
/// straightforward day-counting since 1970-01-01.
fn date_to_unix_seconds(year: u32, month: u32, day: u32) -> u64 {
    fn is_leap(y: u32) -> bool {
        (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
    }
    let mut days: u64 = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    let dim = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 1..month {
        days += dim[(m - 1) as usize] as u64;
        if m == 2 && is_leap(year) {
            days += 1;
        }
    }
    days += (day - 1) as u64;
    days * 86_400
}

/// The live CI gate. If wall-clock time has reached the deadline, the
/// dossier hasn't been published on time — fail loudly with the
/// remediation workflow in the panic message.
#[test]
fn sota_dossier_deadline_not_passed() {
    let (y, m, d) = parse_next_deadline_date();
    let deadline = date_to_unix_seconds(y, m, d);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_secs();

    if now >= deadline {
        panic!(
            "Quarterly SOTA-gap dossier deadline ({y:04}-{m:02}-{d:02}) has passed without a new dossier.\n\
             \n\
             Per docs/specifications/sub/provability-roadmap.md §1.6, this fires falsifier F6\n\
             automatically. To resolve:\n\
             \n\
               1. Author the new dossier as a new §6+ subsection in audit-design.md.\n\
               2. Bump the `**Next Dossier Deadline: YYYY-MM-DD**` line in §0 to the next\n\
                  scheduled quarter (see the cadence table in §0).\n\
               3. CI passes when current time < new deadline.\n\
             \n\
             Hiding slippage is itself a falsification — bump the deadline publicly with a\n\
             reason in the commit message, do not just disable this test."
        );
    }
}

/// Sanity: the cadence table in §0 mentions all four quarters of the
/// initial rollout (2026-Q2, Q3, Q4, 2027-Q1) so the schedule is
/// visible even when the "Next Dossier Deadline" pointer has rolled
/// forward. Caught me deleting a quarter row once on a rewrite.
#[test]
fn cadence_table_lists_all_initial_quarters() {
    let audit = fs::read_to_string(workspace_root().join("docs/specifications/audit-design.md"))
        .expect("read audit-design.md");
    for tag in &["2026-Q2", "2026-Q3", "2026-Q4", "2027-Q1"] {
        assert!(
            audit.contains(tag),
            "audit-design.md §0 cadence table must list `{tag}`"
        );
    }
}

/// Self-test: the date arithmetic is correct on a few known points.
/// 1970-01-01 = 0, 2026-08-15 = 1786752000 (verified via
/// `datetime.datetime(2026, 8, 15, tzinfo=utc).timestamp()`).
#[test]
fn date_to_unix_seconds_known_points() {
    assert_eq!(date_to_unix_seconds(1970, 1, 1), 0);
    assert_eq!(date_to_unix_seconds(1970, 1, 2), 86_400);
    assert_eq!(date_to_unix_seconds(2026, 8, 15), 1_786_752_000);
    // 2000-01-01 = 946684800 — the canonical "we survived Y2K" sanity check.
    assert_eq!(date_to_unix_seconds(2000, 1, 1), 946_684_800);
}
