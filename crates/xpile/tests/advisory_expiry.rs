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

/// The longest an exemption may be dated out, in days. A CONSTANT here and not
/// a value read from the ledger: a review lane pointed out that when the window
/// lives in the same file as the dates it bounds, raising it is a one-line edit
/// in the very diff that needs it, and the gate has no opinion. The ledger still
/// declares `max_window_days` so a reader sees the rule beside the dates — and
/// the gate refuses a ledger whose declaration disagrees with this constant.
const MAX_WINDOW_DAYS: i64 = 90;

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
fn ignored_in_deny() -> Vec<String> {
    ids_in_advisories_ignore(&read(DENY))
}

/// There is no `toml` crate anywhere in this workspace — not even in the lock —
/// and this gate is not worth adding one for. So the array is scanned directly,
/// but ANCHORED and STRING-AWARE rather than by comment strip or first match.
///
/// Two defects this scan was measured against, one from each family of review:
///
/// * In `paiml/ruchy` the first version stripped `#` to end-of-line, and TOML
///   does not treat `#` inside a string as a comment: `{ reason = "tracked in
///   #1234", id = "RUSTSEC-…" }` hid a LIVE exemption. Here `"` toggles a string
///   state and `#` only opens a comment outside one.
/// * The first version HERE found the array with `text.find("ignore")` and the
///   next `[`. A review lane reproduced that a `# ignore` in any comment above
///   `[advisories]` makes that `[` the table header itself, the region ends at
///   its `]` at once, and the scan returns NOTHING — so every exemption is
///   "listed" (there are none to list) and the unlisted check cannot fail. Now
///   the `[advisories]` header is matched at line start, the region ends at the
///   next table header, and `ignore` must be a key at line start inside it.
///
/// `GHSA-` ids are kept as well as `RUSTSEC-`: cargo-deny accepts both, and a
/// filter that only knows one prefix is a filter that lets the other through.
fn ids_in_advisories_ignore(text: &str) -> Vec<String> {
    let array = advisories_ignore_array(text);
    let (mut ids, _) = scan_array(&array);
    ids.retain(|s| s.starts_with("RUSTSEC-") || s.starts_with("GHSA-"));
    ids
}

/// The text of `ignore = [ … ]` under `[advisories]`, from the `=` to the
/// closing bracket, or empty when there is no such key in that table.
fn advisories_ignore_array(text: &str) -> String {
    let mut in_advisories = false;
    let mut array = String::new();
    let mut collecting = false;
    for line in text.lines() {
        let t = line.trim_start();
        if !collecting && t.starts_with('[') {
            in_advisories = t.starts_with("[advisories]");
            continue;
        }
        if !in_advisories {
            continue;
        }
        if collecting {
            array.push_str(line);
        } else if let Some(rest) = ignore_key_value(t) {
            collecting = true;
            array.push_str(rest);
        } else {
            continue;
        }
        array.push('\n');
        if scan_array(&array).1 {
            break;
        }
    }
    array
}

/// `ignore = <rest>` at the start of a line, giving `<rest>`; None otherwise.
fn ignore_key_value(t: &str) -> Option<&str> {
    t.strip_prefix("ignore")?.trim_start().strip_prefix('=')
}

/// Every double-quoted string inside the first `[ … ]` of `array`, ignoring
/// `#` comments outside strings and nesting one level of `{ }` / `[ ]`, and
/// whether that outermost bracket has closed.
fn scan_array(array: &str) -> (Vec<String>, bool) {
    let Some(open) = array.find('[') else {
        return (Vec::new(), false);
    };
    let mut st = ArrayScan::default();
    for c in array[open + 1..].chars() {
        if st.feed(c) {
            return (st.strings, true);
        }
    }
    (st.strings, false)
}

#[derive(Default)]
struct ArrayScan {
    strings: Vec<String>,
    cur: String,
    in_str: bool,
    in_comment: bool,
    depth: i32,
}

impl ArrayScan {
    /// Feed one character; true when the outermost bracket closes.
    fn feed(&mut self, c: char) -> bool {
        if self.in_comment {
            self.in_comment = c != '\n';
            return false;
        }
        if self.in_str {
            self.in_string(c);
            return false;
        }
        match c {
            '#' => self.in_comment = true,
            '"' => self.in_str = true,
            '[' | '{' => self.depth += 1,
            ']' | '}' if self.depth == 0 => return true,
            ']' | '}' => self.depth -= 1,
            _ => {}
        }
        false
    }

    fn in_string(&mut self, c: char) {
        if c == '"' {
            self.strings.push(std::mem::take(&mut self.cur));
            self.in_str = false;
        } else {
            self.cur.push(c);
        }
    }
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

// ------------------------------------------------ the checks, over data
//
// Each rule is a function over (ledger entries, deny ids, today) so that it can
// be shown to FAIL on a synthetic input. A gate that has only ever been run on
// the live files — which pass — has never been seen to go red, and a rule that
// has never fired is a rule nobody knows is live. The `_refuses_` tests below are
// those negative controls; the live tests after them are the gate.

fn unlisted(deny_ids: &[String], entries: &[Entry]) -> Vec<String> {
    deny_ids
        .iter()
        .filter(|id| !entries.iter().any(|e| &e.id == *id))
        .cloned()
        .collect()
}

fn orphans(deny_ids: &[String], entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| !deny_ids.contains(&e.id))
        .map(|e| format!("{} ({})", e.id, e.krate))
        .collect()
}

fn thin(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| {
            e.krate.trim().is_empty()
                || e.owner.trim().is_empty()
                || e.owner_ticket.trim().is_empty()
                || e.reason.trim().len() < 20
        })
        .map(|e| format!("{} (crate {:?}, owner {:?})", e.id, e.krate, e.owner))
        .collect()
}

/// Expired, or unreadable — an unreadable date counts as expired: a promise
/// nobody can read is not one. `removed_by == today` is still valid; the ledger
/// says "the day one is past, the gate goes RED", and past means strictly.
fn expired(entries: &[Entry], today: i64) -> Vec<String> {
    entries
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
        .collect()
}

fn beyond_window(entries: &[Entry], today: i64) -> Vec<String> {
    let cap = today + MAX_WINDOW_DAYS;
    entries
        .iter()
        .filter_map(|e| parse_iso(&e.removed_by).map(|d| (e, d)))
        .filter(|(_, d)| *d > cap)
        .map(|(e, d)| format!("{} dated {} ({} days out)", e.id, e.removed_by, d - today))
        .collect()
}

fn entry(id: &str, removed_by: &str) -> Entry {
    Entry {
        id: id.to_string(),
        krate: "some-crate".to_string(),
        reason: "a reason long enough to pass the thinness floor".to_string(),
        owner: "someone".to_string(),
        owner_ticket: "#1".to_string(),
        removed_by: removed_by.to_string(),
    }
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// --------------------------------------------- negative controls (must fire)

#[test]
fn the_scan_refuses_to_be_blinded_by_a_comment_saying_ignore() {
    // The defect the first version of this scan had: `# ignore` above the table.
    let text = "[graph]\n# ignore this note\nall-features = true\n\n[advisories]\n\
                yanked = \"deny\"\nignore = [\n    \"RUSTSEC-2024-0001\",  # a\n\
                    { id = \"GHSA-xxxx-yyyy-zzzz\", reason = \"tracked in #12 [sic]\" },\n\
                    # \"RUSTSEC-2024-0002\",\n]\n\n[bans]\nskip = [ \"RUSTSEC-2024-0003\" ]\n";
    assert_eq!(
        ids_in_advisories_ignore(text),
        ids(&["RUSTSEC-2024-0001", "GHSA-xxxx-yyyy-zzzz"]),
        "the scan must find both ids under [advisories] ignore, skip the commented \
         one, and not read [bans] skip"
    );
}

#[test]
fn the_scan_returns_nothing_when_there_is_no_ignore_array_and_says_so() {
    assert!(ids_in_advisories_ignore("[advisories]\nyanked = \"deny\"\n").is_empty());
    assert!(ids_in_advisories_ignore("[bans]\nignore = [\"RUSTSEC-2024-0001\"]\n").is_empty());
}

#[test]
fn unlisted_refuses_an_exemption_the_ledger_does_not_carry() {
    let e = [entry("RUSTSEC-2024-0001", "2026-01-01")];
    assert_eq!(
        unlisted(&ids(&["RUSTSEC-2024-0001", "RUSTSEC-2024-0002"]), &e),
        ids(&["RUSTSEC-2024-0002"])
    );
}

#[test]
fn orphans_refuses_a_ledger_entry_deny_toml_no_longer_ignores() {
    let e = [
        entry("RUSTSEC-2024-0001", "2026-01-01"),
        entry("RUSTSEC-2024-0002", "2026-01-01"),
    ];
    assert_eq!(orphans(&ids(&["RUSTSEC-2024-0001"]), &e).len(), 1);
}

#[test]
fn thin_refuses_a_missing_owner_a_missing_crate_and_a_short_reason() {
    let mut no_owner = entry("RUSTSEC-2024-0001", "2026-01-01");
    no_owner.owner = "  ".into();
    let mut no_crate = entry("RUSTSEC-2024-0002", "2026-01-01");
    no_crate.krate = String::new();
    let mut short = entry("RUSTSEC-2024-0003", "2026-01-01");
    short.reason = "unmaintained".into();
    assert_eq!(thin(&[no_owner, no_crate, short]).len(), 3);
    assert!(thin(&[entry("RUSTSEC-2024-0004", "2026-01-01")]).is_empty());
}

#[test]
fn expired_refuses_yesterday_accepts_today_and_counts_unreadable_as_expired() {
    let today = days_from_civil(2026, 9, 20);
    let e = [
        entry("RUSTSEC-2024-0001", "2026-09-19"),
        entry("RUSTSEC-2024-0002", "2026-09-20"),
        entry("RUSTSEC-2024-0003", "soon"),
    ];
    let out = expired(&e, today);
    assert_eq!(out.len(), 2, "{out:#?}");
    assert!(out[0].contains("RUSTSEC-2024-0001") && out[1].contains("RUSTSEC-2024-0003"));
}

#[test]
fn beyond_window_refuses_day_91_and_accepts_day_90() {
    let today = days_from_civil(2026, 9, 20);
    let e = [
        entry("RUSTSEC-2024-0001", "2026-12-19"), // +90
        entry("RUSTSEC-2024-0002", "2026-12-20"), // +91
    ];
    let out = beyond_window(&e, today);
    assert_eq!(out.len(), 1, "{out:#?}");
    assert!(out[0].contains("RUSTSEC-2024-0002"));
}

#[test]
fn dates_cross_a_leap_day_and_a_year_boundary_exactly() {
    assert_eq!(
        days_from_civil(2028, 3, 1) - days_from_civil(2028, 2, 28),
        2
    );
    assert_eq!(
        days_from_civil(2027, 1, 1) - days_from_civil(2026, 12, 31),
        1
    );
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(parse_iso("2026-13-01"), None);
}

// ---------------------------------------------------------------- the gate

#[test]
fn every_deny_exemption_has_a_ledger_entry() {
    let unlisted = unlisted(&ignored_in_deny(), &ledger().ignores);
    assert!(
        unlisted.is_empty(),
        "PMAT-2120: {} exemption(s) in {DENY} have no entry in {LEDGER}: {unlisted:?}. \
         An exemption with no recorded expiry never expires.",
        unlisted.len()
    );
}

#[test]
fn the_scan_sees_the_exemptions_deny_toml_carries() {
    // The live half of the scan's control: a count floor, so an anchoring
    // regression that returns nothing cannot pass the unlisted check vacuously.
    let n = ignored_in_deny().len();
    assert!(
        n >= 1,
        "PMAT-2120: the scan read {n} ids from {DENY}; the ledger has {}",
        ledger().ignores.len()
    );
}

#[test]
fn no_ledger_entry_is_orphaned() {
    let orphans = orphans(&ignored_in_deny(), &ledger().ignores);
    assert!(
        orphans.is_empty(),
        "PMAT-2120: {} ledger entr(ies) name an advisory {DENY} no longer ignores: \
         {orphans:?}. The exemption was dropped and the ledger was not.",
        orphans.len()
    );
}

#[test]
fn every_entry_names_a_crate_an_owner_and_a_ticket() {
    let thin = thin(&ledger().ignores);
    assert!(
        thin.is_empty(),
        "PMAT-2120: {} ledger entr(ies) lack a crate, an owner, a ticket or a usable \
         reason: {thin:#?}. Five of these entries once read only \"unmaintained \
         ecosystem dep\" — an exemption nobody can identify is one nobody retires, \
         and one nobody owns is one nobody removes.",
        thin.len()
    );
}

#[test]
fn no_exemption_is_past_its_removed_by() {
    let expired = expired(&ledger().ignores, today_utc_days());
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
    assert_eq!(
        l.max_window_days, MAX_WINDOW_DAYS,
        "PMAT-2120: {LEDGER} declares max_window_days = {} but the gate holds {MAX_WINDOW_DAYS}. \
         The window is not a ledger knob; change the constant, in a commit that says why.",
        l.max_window_days
    );
    let far = beyond_window(&l.ignores, today_utc_days());
    assert!(
        far.is_empty(),
        "PMAT-2120: {} exemption(s) dated more than {MAX_WINDOW_DAYS} days out: {far:#?}. \
         A promise far enough away is not a promise.",
        far.len()
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
