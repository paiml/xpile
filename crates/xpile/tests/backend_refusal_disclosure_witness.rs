//! XPILE-BACKENDREFUSE-001 (PMAT-1437) — what a backend REFUSAL actually
//! discloses, measured by running the binary, and what the book is allowed to
//! say a CONTRACT pins down.
//!
//! ## The defect this locks out
//!
//! `book/src/reference/backends.md` published this, as the page's own
//! statement of the guarantee, at da411cef:
//!
//! ```text
//! each backend refuses constructs outside its subset with a message naming
//! the governing contract and, where one exists, a better `--target`.
//! That refusal *is* the guarantee
//! ```
//!
//! and, in the header blockquote, attributed it to the contract:
//!
//! ```text
//! The contract pins down structural emit invariants: every emitted artifact
//! must carry a `// xpile-contract: <ID>` citation, error paths must name the
//! governing contract, and unsupported constructs must fail cleanly with a
//! target-suggestion message.
//! ```
//!
//! Both halves were false, and the second was false TWICE OVER.
//!
//! **The contract does not pin either invariant.**
//! `contracts/xpile-backend-trait-v1.yaml` is 777 lines and 20 equations; the
//! strings `refus`, `suggest` and `error path` appear in it ZERO times. Its
//! equations are `target_ownership`, `lower_idempotency`, `target_consistency`,
//! `compile_contract_citation`, `frame_lower_is_pure` and thirteen Diamond
//! refinements — every one of them about the SUCCESS path. There is no
//! equation, no proof obligation and no falsification test about what a
//! refusal MESSAGE says.
//!
//! **The shipped backends do not satisfy it either.** Measured at da411cef
//! over the corpus below — 40 refusals that reached a backend's own
//! `lower()`, spanning all nine registered backends — 4 named a contract ID
//! and 7 named a better `--target`. Five of the nine (`ptx`, `wgsl`, `spirv`,
//! `bashrs`, `forjar`) did NEITHER, in any of their probed refusals.
//!
//! ## Why the wording mattered more than a doc row usually does
//!
//! `book/src/contributing/adding-a-backend.md` repeated the same list as
//! implementation instructions — "the trait's emit invariants (citation
//! requirement, error-path-names-the-contract requirement, target-suggestion
//! on unsupported constructs) are what your implementation must satisfy" — so
//! a contributor was told to satisfy a requirement that neither the contract
//! states nor five of the nine shipped backends meet. `cli.md` cited the same
//! contract "for emit-side error paths".
//!
//! ## Both directions are checked, and neither side is a hard-coded roster
//!
//! * The backend roster comes from `xpile info`; the probe results come from
//!   executing `xpile transpile`. A refusal counts only if it reached the
//!   BACKEND — the message must name the failing backend AND carry
//!   `lowering error:` (`BackendError::Lower`), so a frontend refusal or a
//!   `missing hardware profile` cannot stand in for one (PMAT-1432: a check
//!   that counts anything merely NEARBY certifies nothing).
//! * The counts are compared to a machine-readable table in `backends.md` by
//!   EQUALITY, not by `>=`. Improving a refusal message REDS this gate and
//!   forces the published disclosure to move with it — the PMAT-1431 §4
//!   `emit_surface` idiom.
//! * `every_invariant_the_book_attributes_to_a_contract_is_an_equation_key`
//!   closes the attribution half at its root: a page may only say a contract
//!   pins an invariant by naming an `equations:` KEY that exists in that
//!   contract's YAML. Prose cannot invent one.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn xpile(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("running `xpile {}`: {e}", args.join(" ")))
}

/// The registered backend names, read off the binary's own `xpile info`
/// listing (`    - <name> → <Target>[, …]`). Not a roster in this file:
/// a backend added tomorrow is covered the moment it registers.
fn registered_backends() -> BTreeSet<String> {
    let out = xpile(&["info"]);
    assert!(out.status.success(), "`xpile info` must succeed");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let block = stdout
        .split("  backends (")
        .nth(1)
        .unwrap_or_else(|| panic!("`xpile info` must print a `backends (N):` block:\n{stdout}"));
    let mut names = BTreeSet::new();
    for line in block.lines().skip(1) {
        let Some(rest) = line.trim_start().strip_prefix("- ") else {
            break; // the backends block ends at the first non-entry line
        };
        let name = rest.split('→').next().unwrap_or("").trim();
        assert!(
            !name.is_empty(),
            "malformed `xpile info` backend line: {line}"
        );
        names.insert(name.to_string());
    }
    names
}

/// Every canonical `--target` spelling, taken from the binary's own
/// `unknown target` refusal (the roster `parse_target` matches through).
/// Aliases are excluded — they resolve to a canonical spelling and would
/// double-count the same backend.
fn canonical_targets(probe_dir: &Path) -> Vec<String> {
    let src = probe_dir.join("vocab.py");
    std::fs::write(&src, "def g(a: int) -> int:\n    return a\n").expect("write vocab probe");
    let out = xpile(&[
        "transpile",
        &src.to_string_lossy(),
        "--target",
        "__no_such_target__",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let vocab = stderr.split("choose: ").nth(1).unwrap_or_else(|| {
        panic!("`--target __no_such_target__` must list the choices:\n{stderr}")
    });
    // `choose: a, b, c; aliases: …` — the canonical set is everything before
    // the alias clause.
    let list = vocab.split(';').next().unwrap_or("");
    let targets: Vec<String> = list
        .split(',')
        .map(|s| s.trim().trim_matches('`').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    assert!(
        targets.len() >= 2,
        "the `unknown target` refusal must enumerate the canonical spellings, got {targets:?}"
    );
    targets
}

/// The fixed probe corpus. Each entry is (file name, source). Every one is a
/// program some backend lowers and some backend refuses — the point is to
/// reach a `BackendError::Lower`, not to be exhaustive over any backend's
/// subset. A single probe samples ONE of a backend's many refusal messages
/// (PMAT-1433 lesson 3), which is exactly why the published table records
/// counts over THIS corpus and says so, rather than a per-backend verdict.
const CORPUS: &[(&str, &str)] = &[
    (
        "str_concat.py",
        "def g(s: str) -> str:\n    return s + \"x\"\n",
    ),
    (
        "dict_index.py",
        "def g(n: int) -> int:\n    d = {1: 2}\n    return d[n]\n",
    ),
    (
        "set_len.py",
        "def g(n: int) -> int:\n    s = {1, 2}\n    return len(s)\n",
    ),
    (
        "str_of_float.py",
        "def g(n: float) -> str:\n    return str(n)\n",
    ),
    (
        "list_index.py",
        "def g(n: int) -> int:\n    xs = [1, 2, 3]\n    return xs[n]\n",
    ),
    (
        "float_mul.py",
        "def g(n: float) -> float:\n    return n * 2.5\n",
    ),
    ("shell_echo.sh", "#!/bin/sh\necho hi\n"),
];

/// One executed refusal that reached a backend's `lower()`.
struct Refusal {
    backend: String,
    #[allow(dead_code)]
    probe: String,
    names_contract: bool,
    names_target_flag: bool,
}

/// Run the whole corpus against every canonical target and keep the failures
/// that reached the BACKEND. The stage is pinned twice: the message must name
/// the failing backend (`backend `<name>` failed`, the CLI's own wrapper) and
/// must carry `lowering error:` (`BackendError::Lower`). A frontend refusal
/// (`parse_and_lower failed`) and a `missing hardware profile` are excluded by
/// construction, not by a name filter.
fn measured_refusals(probe_dir: &Path) -> Vec<Refusal> {
    let mut out = Vec::new();
    for (name, body) in CORPUS {
        let src = probe_dir.join(name);
        std::fs::write(&src, body).unwrap_or_else(|e| panic!("write {name}: {e}"));
        let src = src.to_string_lossy().to_string();
        for target in canonical_targets(probe_dir) {
            let run = xpile(&["transpile", &src, "--target", &target]);
            let mut stderr = String::from_utf8_lossy(&run.stderr).to_string();
            // The PTX backend refuses without a compute capability before it
            // ever looks at the module (`BackendError::MissingHardware`).
            // Supply one and re-run — derived from the observed message, so no
            // target name is spelled here.
            if stderr.contains("missing hardware profile") {
                let run = xpile(&[
                    "transpile",
                    &src,
                    "--target",
                    &target,
                    "--hardware",
                    "ptx:sm_80",
                ]);
                stderr = String::from_utf8_lossy(&run.stderr).to_string();
            }
            let one_line = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
            if !one_line.contains("lowering error:") {
                continue;
            }
            let Some(backend) = one_line
                .split("backend `")
                .nth(1)
                .and_then(|s| s.split('`').next())
            else {
                continue; // not a backend-stage failure
            };
            out.push(Refusal {
                backend: backend.to_string(),
                probe: (*name).to_string(),
                // A contract ID is `C-` followed by an upper-case letter —
                // `C-PY-INT-ARITH`, `C-BASHRS-POSIX-IDEMPOTENCE`. The leading
                // space keeps `...C-` inside a word from matching.
                names_contract: one_line.match_indices("C-").any(|(i, _)| {
                    let before_ok = i == 0 || !one_line.as_bytes()[i - 1].is_ascii_alphanumeric();
                    let after_ok = one_line.as_bytes()[i + 2].is_ascii_uppercase();
                    before_ok && after_ok
                }),
                names_target_flag: one_line.contains("--target"),
            });
        }
    }
    out
}

/// The published table in `book/src/reference/backends.md`, keyed by backend:
/// `(probed, naming a contract, suggesting a --target)`.
///
/// Rows are read from the marked block, so the parser cannot wander into
/// another table on the page.
fn published_table() -> BTreeMap<String, (usize, usize, usize)> {
    const MARKER: &str = "<!-- XPILE-BACKENDREFUSE-001:BEGIN -->";
    const END: &str = "<!-- XPILE-BACKENDREFUSE-001:END -->";
    let page = workspace_root().join("book/src/reference/backends.md");
    let body =
        std::fs::read_to_string(&page).unwrap_or_else(|e| panic!("read {}: {e}", page.display()));
    let block = body
        .split(MARKER)
        .nth(1)
        .unwrap_or_else(|| panic!("backends.md must carry the `{MARKER}` block"))
        .split(END)
        .next()
        .unwrap_or_else(|| panic!("backends.md must close the block with `{END}`"));
    let mut rows = BTreeMap::new();
    for line in block.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().trim_matches('`'))
            .collect();
        if cells.len() != 4 {
            continue;
        }
        let (Ok(probed), Ok(contract), Ok(target)) = (
            cells[1].parse::<usize>(),
            cells[2].parse::<usize>(),
            cells[3].parse::<usize>(),
        ) else {
            continue; // header and separator rows
        };
        rows.insert(cells[0].to_string(), (probed, contract, target));
    }
    rows
}

/// A private probe directory per CALL, not per test — two tests running
/// concurrently must not share a namespace (PMAT-1436).
fn probe_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("xpile-backendrefuse-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create probe dir");
    dir
}

#[test]
fn refusal_disclosure_table_matches_the_running_binary() {
    let dir = probe_dir("table");
    let refusals = measured_refusals(&dir);
    let backends = registered_backends();

    let mut measured: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for r in &refusals {
        let e = measured.entry(r.backend.clone()).or_insert((0, 0, 0));
        e.0 += 1;
        e.1 += usize::from(r.names_contract);
        e.2 += usize::from(r.names_target_flag);
    }

    // NON-VACUITY, tied to an independently derived set: every backend
    // `xpile info` registers must have produced at least one measured
    // refusal. A corpus that stopped reaching a backend would otherwise
    // silently drop its row and the table would still "match".
    let uncovered: Vec<&String> = backends
        .iter()
        .filter(|b| !measured.contains_key(*b))
        .collect();
    assert!(
        uncovered.is_empty(),
        "the probe corpus reached no `BackendError::Lower` for {uncovered:?}; \
         every backend `xpile info` registers must be measured, or the table \
         below is a claim about a subset. Registered: {backends:?}"
    );
    assert!(
        measured.keys().all(|b| backends.contains(b)),
        "a refusal named a backend `xpile info` does not register: measured {:?} vs registered {backends:?}",
        measured.keys().collect::<Vec<_>>()
    );

    let published = published_table();
    assert_eq!(
        published, measured,
        "\nbook/src/reference/backends.md publishes a refusal-disclosure table \
         that no longer matches the binary.\n  published: {published:?}\n  measured:  {measured:?}\n\
         This is EQUALITY, not a floor: improving a refusal message is supposed \
         to red this gate so the published disclosure moves with it."
    );
}

#[test]
fn every_invariant_the_book_attributes_to_a_contract_is_an_equation_key() {
    // A page says what a contract pins by naming `equations:` KEYS after this
    // marker. Prose alone can no longer attribute an invariant to a contract.
    const MARKER: &str = "The invariants it pins:";
    let root = workspace_root();

    let mut pages_with_marker = 0usize;
    let mut keys_checked = 0usize;
    let mut offenders: Vec<String> = Vec::new();

    for (rel, body) in book_pages(&root) {
        // The page's header blockquote: the first run of `>` lines.
        let quote: Vec<&str> = body
            .lines()
            .skip_while(|l| !l.starts_with('>'))
            .take_while(|l| l.starts_with('>'))
            .collect();

        // The contract(s) it names, e.g.
        // `> **Governing contract:** [`C-XPILE-BACKEND-TRAIT`](…)`.
        let cited: BTreeSet<String> = quote
            .iter()
            .flat_map(|l| backticked(l))
            .filter(|t| {
                t.starts_with("C-") && t.chars().all(|c| c.is_ascii_uppercase() || c == '-')
            })
            .collect();
        if cited.is_empty() {
            continue; // no contract named — nothing is being attributed
        }

        // THE MARKER IS MANDATORY, and this is the half that reds the defect.
        // Every one of these nine blockquotes described what its contract
        // "pins down" / "governs" / "requires" in PROSE, and three of them
        // described things no equation in the YAML says. A gate that only
        // validated backticked keys would have passed the prose form
        // unchanged. Naming a contract in a header blockquote is therefore an
        // obligation to say which `equations:` keys you mean.
        assert!(
            body.contains(MARKER),
            "{rel} names {cited:?} in its header blockquote but never says \
             `{MARKER}`. A page that invokes a contract must name the \
             `equations:` keys it is invoking — prose describing what a \
             contract 'pins down' is exactly how PMAT-1437's falsehood got in."
        );
        pages_with_marker += 1;

        let known: BTreeSet<String> = cited
            .iter()
            .flat_map(|id| equation_keys(&root, id))
            .collect();

        // The claim runs from the marker to the end of the blockquote — every
        // backticked snake_case token on those lines is an attributed
        // invariant. The marker's OWN line counts: the list normally starts
        // on it.
        let start = quote
            .iter()
            .position(|l| l.contains(MARKER))
            .expect("the marker is inside the header blockquote");
        // An equation key is snake_case WITH an underscore. The underscore is
        // what separates a key from an incidental backticked word on the same
        // line (`int`, `i64`) — and it is checked, not assumed: every key of
        // every cited contract must have one, or this filter would be silently
        // skipping a real key.
        assert!(
            known.iter().all(|k| k.contains('_')),
            "{rel}: {cited:?} has an equation key with no underscore, which this \
             test's key filter would skip: {:?}",
            known
                .iter()
                .filter(|k| !k.contains('_'))
                .collect::<Vec<_>>()
        );
        for line in &quote[start..] {
            for tok in backticked(line) {
                if !tok.contains('_')
                    || !tok
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                {
                    continue; // prose, a flag, a path — not an equation key
                }
                keys_checked += 1;
                if !known.contains(&tok) {
                    offenders.push(format!(
                        "{rel}: `{tok}` is not an `equations:` key of {cited:?}"
                    ));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "\na book page attributes an invariant to a contract that has no such equation:\n  {}\n\
         Name an `equations:` key that exists, or stop saying the contract pins it.",
        offenders.join("\n  ")
    );
    // NON-VACUITY, both halves: the marker must be in use, and it must be
    // naming real keys rather than sitting over an empty list.
    assert!(
        pages_with_marker >= 9,
        "only {pages_with_marker} book page(s) declare which invariants their contract pins; \
         the three C-XPILE-BACKEND-TRAIT pages (backends.md, cli.md, adding-a-backend.md) \
         are the ones this gate exists for"
    );
    assert!(
        keys_checked >= pages_with_marker,
        "{keys_checked} equation key(s) checked across {pages_with_marker} page(s) — \
         a page carrying the marker with no key checks nothing"
    );
}

/// Every `*.md` under `book/src/`, recursively, as (repo-relative path, body).
fn book_pages(root: &Path) -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                walk(&p, root, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("md") {
                let rel = p
                    .strip_prefix(root)
                    .expect("book page under workspace root")
                    .to_string_lossy()
                    .into_owned();
                let body =
                    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                out.push((rel, body));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("book/src"), root, &mut out);
    out
}

/// The backticked tokens on one line.
fn backticked(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

/// The `equations:` keys of a contract, by ID. The YAML corpus is scanned for
/// the file whose `metadata.id` matches, so a rename cannot silently turn the
/// check into a no-op.
fn equation_keys(root: &Path, id: &str) -> BTreeSet<String> {
    #[derive(serde::Deserialize)]
    struct Doc {
        metadata: Meta,
        #[serde(default)]
        equations: BTreeMap<String, serde_yaml::Value>,
    }
    #[derive(serde::Deserialize)]
    struct Meta {
        id: String,
    }

    let dir = root.join("contracts");
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir contracts: {e}"));
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        let Ok(doc) = serde_yaml::from_str::<Doc>(&body) else {
            continue;
        };
        if doc.metadata.id == id {
            return doc.equations.into_keys().collect();
        }
    }
    panic!("no contract in contracts/ declares `metadata.id: {id}`");
}
