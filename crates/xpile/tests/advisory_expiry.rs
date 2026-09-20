//! PMAT-2120 — every advisory exemption is an owned, dated promise, and none of
//! them is dead.
//!
//! `deny.toml` carries seven `[advisories] ignore` entries. Neither `cargo deny`
//! nor `cargo audit` lets an ignore say *until when*, which is how a standing
//! exemption becomes permanent: it outlives the reason for it, nobody re-reads
//! it, and the repository reports green about something it stopped looking at.
//!
//! `docs/audits/advisory-ignores.yaml` gives each one a crate, a reason, a named
//! owner, an owner ticket and a `removed_by`. This test refuses a missing entry,
//! a missing owner, an expired date, a date beyond the window — and a DEAD
//! exemption.
//!
//! # Why liveness is checked by removing the ignore
//!
//! "Is this exemption still doing anything" cannot be answered from `Cargo.lock`.
//! MEASURED 2026-09-20: `RUSTSEC-2024-0436` (`paste`) is **dead** in `paiml/ruchy`
//! — removing its ignore leaves `cargo deny` at exit 0 — and **live** here. Same
//! crate, same advisory, opposite answer, because this repo sets
//! `[graph] all-features = true` and ruchy does not. A crate in the lock is not a
//! crate in the advisory's reachable graph.
//!
//! And it is done PER EXEMPTION. Removing all seven at once and seeing
//! `advisories FAILED` proves at least one is live, not that each one is — a test
//! over a set does not license a claim about its members. That mistake was made
//! on this very repository first, and corrected.

use std::path::{Path, PathBuf};
use std::process::Command;

const LEDGER: &str = "docs/audits/advisory-ignores.yaml";
const DENY: &str = "deny.toml";

/// Declare that this host MUST be able to run the liveness check. Same contract
/// as `XPILE_REQUIRE_WASM_RUNTIME` and `XPILE_REQUIRE_FORJAR`: set in CI, it
/// turns a missing `cargo-deny` into a RED rather than a skip, so deleting the
/// install step cannot silently retire this half of the gate.
const REQUIRE: &str = "XPILE_REQUIRE_CARGO_DENY";

#[derive(Debug, serde::Deserialize)]
struct Ledger {
    max_window_days: i64,
    ignores: Vec<Entry>,
}

#[derive(Debug, serde::Deserialize)]
struct Entry {
    id: String,
    #[serde(rename = "crate")]
    krate: String,
    reason: String,
    #[serde(default)]
    owner: String,
    #[serde(default)]
    owner_ticket: String,
    removed_by: String,
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/xpile; the workspace root is two up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("PMAT-2120: cannot resolve the workspace root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("PMAT-2120: cannot read {}: {e}", p.display()))
}

fn ledger() -> Ledger {
    serde_yaml::from_str(&read(LEDGER))
        .unwrap_or_else(|e| panic!("PMAT-2120: {LEDGER} is not valid YAML: {e}"))
}

/// Every advisory id `deny.toml` ignores, read from its `[advisories] ignore`
/// array.
///
/// There is no `toml` crate anywhere in this workspace — not even in the lock —
/// and this gate is not worth adding one for. So the array is scanned directly,
/// but with a STRING-AWARE scan rather than a comment strip.
///
/// The distinction is not pedantry: in `paiml/ruchy` the first version of this
/// gate stripped `#` to end-of-line, and TOML does not treat `#` inside a string
/// as a comment. `{ reason = "tracked in #1234", id = "RUSTSEC-…" }` hid a LIVE
/// exemption, and it took a second-family review with a control against the
/// previous commit to find it. Here the scan tracks whether it is inside quotes,
/// so a `#` in a reason is literal and a `#` outside one ends a comment.
fn ignored_in_deny() -> Vec<String> {
    let text = read(DENY);
    let Some(start) = text.find("ignore") else {
        return Vec::new();
    };
    let Some(open) = text[start..].find('[') else {
        return Vec::new();
    };
    let region = &text[start + open + 1..];

    let mut ids = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut in_comment = false;
    for c in region.chars() {
        match c {
            '\n' if in_comment => in_comment = false,
            _ if in_comment => {}
            '#' if !in_str => in_comment = true,
            '"' => {
                if in_str {
                    ids.push(std::mem::take(&mut cur));
                }
                in_str = !in_str;
            }
            ']' if !in_str => break,
            _ if in_str => cur.push(c),
            _ => {}
        }
    }
    ids.retain(|s| s.starts_with("RUSTSEC-"));
    ids
}

// ---------------------------------------------------------------- dates
// No date crate in this workspace, and one is not worth adding for this. Days
// since the civil epoch, Howard Hinnant's algorithm — exact for any proleptic
// Gregorian date, no leap-second or timezone subtleties, because both sides are
// whole days in UTC.

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn parse_iso(s: &str) -> Option<i64> {
    let mut it = s.trim().splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    (1..=12).contains(&m).then_some(())?;
    (1..=31).contains(&d).then_some(())?;
    Some(days_from_civil(y, m, d))
}

/// Today, in whole UTC days. UTC and not local: a gate whose verdict depends on
/// where it runs is a gate you eventually argue with.
fn today_utc_days() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("PMAT-2120: system clock is before the epoch")
        .as_secs() as i64;
    secs / 86_400
}

// ---------------------------------------------------------------- the gate

#[test]
fn every_deny_exemption_has_a_ledger_entry() {
    let listed: Vec<String> = ledger().ignores.iter().map(|e| e.id.clone()).collect();
    let unlisted: Vec<String> = ignored_in_deny()
        .into_iter()
        .filter(|id| !listed.contains(id))
        .collect();
    assert!(
        unlisted.is_empty(),
        "PMAT-2120: {} exemption(s) in {DENY} have no entry in {LEDGER}: {unlisted:?}. \
         An exemption with no recorded expiry never expires.",
        unlisted.len()
    );
}

#[test]
fn no_ledger_entry_is_orphaned() {
    let ignored = ignored_in_deny();
    let orphans: Vec<String> = ledger()
        .ignores
        .iter()
        .filter(|e| !ignored.contains(&e.id))
        .map(|e| format!("{} ({})", e.id, e.krate))
        .collect();
    assert!(
        orphans.is_empty(),
        "PMAT-2120: {} ledger entr(ies) name an advisory {DENY} no longer ignores: \
         {orphans:?}. The exemption was dropped and the ledger was not.",
        orphans.len()
    );
}

#[test]
fn every_entry_names_a_crate_an_owner_and_a_ticket() {
    let thin: Vec<String> = ledger()
        .ignores
        .iter()
        .filter(|e| {
            e.krate.trim().is_empty()
                || e.owner.trim().is_empty()
                || e.owner_ticket.trim().is_empty()
                || e.reason.trim().len() < 20
        })
        .map(|e| format!("{} (crate {:?}, owner {:?})", e.id, e.krate, e.owner))
        .collect();
    assert!(
        thin.is_empty(),
        "PMAT-2120: {} ledger entr(ies) lack a crate, an owner, a ticket or a usable \
         reason: {thin:#?}. Three of these entries once read only \"unmaintained \
         ecosystem dep\" — an exemption nobody can identify is one nobody retires, \
         and one nobody owns is one nobody removes.",
        thin.len()
    );
}

#[test]
fn no_exemption_is_past_its_removed_by() {
    let today = today_utc_days();
    let expired: Vec<String> = ledger()
        .ignores
        .iter()
        .filter_map(|e| match parse_iso(&e.removed_by) {
            Some(d) if d >= today => None,
            Some(d) => Some(format!(
                "{} ({}) expired {} — {} day(s) ago, owner {}",
                e.id,
                e.krate,
                e.removed_by,
                today - d,
                e.owner
            )),
            None => Some(format!(
                "{} ({}) has an unreadable removed_by {:?}",
                e.id, e.krate, e.removed_by
            )),
        })
        .collect();
    assert!(
        expired.is_empty(),
        "PMAT-2120 EXEMPTION EXPIRED: {} past their removed_by: {expired:#?}\n\n\
         This is the promise coming due, not a flaky gate. Either the advisory was \
         resolved — drop the ignore and the entry — or it was not, in which case \
         re-review it and move the date DELIBERATELY, naming what changed. An \
         unreadable date counts as expired: a promise nobody can read is not one.",
        expired.len()
    );
}

#[test]
fn no_exemption_is_dated_beyond_the_window() {
    let l = ledger();
    let cap = today_utc_days() + l.max_window_days;
    let far: Vec<String> = l
        .ignores
        .iter()
        .filter_map(|e| parse_iso(&e.removed_by).map(|d| (e, d)))
        .filter(|(_, d)| *d > cap)
        .map(|(e, d)| {
            format!(
                "{} dated {} ({} days out)",
                e.id,
                e.removed_by,
                d - today_utc_days()
            )
        })
        .collect();
    assert!(
        far.is_empty(),
        "PMAT-2120: {} exemption(s) dated more than {} days out: {far:#?}. A promise \
         far enough away is not a promise.",
        far.len(),
        l.max_window_days
    );
}

// ------------------------------------------------- liveness: the removal test

/// A DEAD exemption — one whose removal changes nothing — is a scheduled
/// obligation manufactured out of a removable line.
///
/// This is the checker the operator ruled for: **the removal test, never lock
/// presence**. For each exemption, write a `deny.toml` with that one id deleted
/// and nothing else changed, run `cargo deny check advisories` against it, and
/// require the verdict to flip to non-zero. If it does not, the ignore was
/// suppressing nothing and should be deleted rather than dated.
///
/// It runs one `cargo deny` per exemption — seven here, a few seconds each. The
/// batched form is cheaper and does not answer the question: removing all seven
/// at once and seeing `advisories FAILED` proves at least one is live, not that
/// each one is. The reason is here, in the gate, and not only in the ledger,
/// because this is the point at which someone with thirty exemptions will want
/// to batch it.
#[test]
fn no_exemption_is_dead() {
    let Some(deny_bin) = cargo_deny() else {
        assert!(
            std::env::var(REQUIRE).is_err(),
            "PMAT-2120: {REQUIRE} is set, so this host must be able to run \
             `cargo deny` — but it is not installed. Declaring the requirement and \
             then skipping is how a gate retires itself quietly."
        );
        eprintln!("PMAT-2120: cargo-deny not installed; liveness unchecked. Set {REQUIRE}=1 to make this a failure.");
        return;
    };

    let root = repo_root();
    let deny_text = read(DENY);
    let tmp = std::env::temp_dir().join(format!("pmat2120-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("PMAT-2120: cannot create a temp dir");

    let mut dead = Vec::new();
    for e in ledger().ignores {
        let trimmed: String = deny_text
            .lines()
            .filter(|l| !l.contains(&e.id))
            .collect::<Vec<_>>()
            .join("\n");
        let cfg = tmp.join(format!("deny-{}.toml", e.id));
        std::fs::write(&cfg, &trimmed).expect("PMAT-2120: cannot write the probe config");
        let status = Command::new(&deny_bin)
            .args(["--manifest-path"])
            .arg(root.join("Cargo.toml"))
            .args(["check", "-c"])
            .arg(&cfg)
            .arg("advisories")
            .current_dir(&root)
            .output()
            .expect("PMAT-2120: cannot run cargo deny");
        if status.status.success() {
            dead.push(format!("{} ({})", e.id, e.krate));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);

    assert!(
        dead.is_empty(),
        "PMAT-2120: {} exemption(s) are DEAD — removing the ignore leaves \
         `cargo deny check advisories` at exit 0, so it suppresses nothing: \
         {dead:#?}.\n\n\
         Delete the line and its ledger entry rather than dating it. Giving a dead \
         exemption an owner and a date manufactures an obligation out of a \
         removable line, which is the failure this ledger exists to prevent.",
        dead.len()
    );
}

fn cargo_deny() -> Option<PathBuf> {
    let probe = Command::new("cargo-deny").arg("--version").output();
    if probe.map(|o| o.status.success()).unwrap_or(false) {
        return Some(PathBuf::from("cargo-deny"));
    }
    None
}
