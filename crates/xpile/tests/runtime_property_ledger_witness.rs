//! XPILE-RUNTIME-PROPERTY-001 (PMAT-2159, re-derives PMAT-468): which
//! contracts have a property-specific Runtime witness, derived from the tree.
//!
//! PMAT-468 was filed against "10 placeholder contracts" that reached quorum
//! on a single byte-identity demo fixture. That count was later re-scoped to
//! 4, but the 4 came from a different metric (contracts with no
//! `falsification_tests` key, fixed in #1875), so the question PMAT-468 asks
//! was never answered: for each `contracts/*.yaml`, is there a test that RUNS
//! emitted output and asserts on the construct that contract governs?
//!
//! [`LEDGER`] answers it, one row per contract id, and this file checks the
//! answer:
//!
//! 1. the ledger's ids are exactly the ids derived from `contracts/*.yaml`
//!    (a new contract with no row, or a row for a deleted contract, is red);
//! 2. every [`Kind::Executes`] row names a `#[test]` fn that exists and calls
//!    its execution probe on a code line, and the probe's home file spawns a
//!    process or a GPU instance on a code line;
//! 3. every [`Kind::InProcess`] row names a `#[test]` fn in a file that names
//!    the contract id (these contracts govern xpile's own trait API, so calling
//!    the trait IS the property);
//! 4. the residue count is derived from the ledger and must match the sentence
//!    `audit-design.md` publishes.
//!
//! What this does NOT check: that an executing test covers the WHOLE
//! contract. `C-C-FLOAT-ARITH`'s witness, for example, runs only two float
//! literal cases inside a mostly-int corpus. A row here means "some property
//! of this contract is executed", not "the contract is fully witnessed".

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// How a contract's governed property is exercised at runtime.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// A test runs emitted output. `probe` is the call in the test body that
    /// executes it; `runner` is the repo-relative file whose code spawns the
    /// process or GPU instance (usually the test file itself).
    Executes {
        file: &'static str,
        test: &'static str,
        probe: &'static str,
        runner: &'static str,
    },
    /// The contract governs xpile's own API; the test calls that API directly.
    InProcess {
        file: &'static str,
        test: &'static str,
    },
    /// No property-specific Runtime witness exists. The reason says why.
    Residue { reason: &'static str },
}

const E2E: &str = "crates/xpile/tests/transpile_e2e.rs";
const TRAITS: &str = "crates/xpile/tests/trait_runtime_properties.rs";

const fn e2e(test: &'static str) -> Kind {
    Kind::Executes {
        file: E2E,
        test,
        probe: "assert_rustc_runs",
        runner: E2E,
    }
}

const fn same_file(file: &'static str, test: &'static str, probe: &'static str) -> Kind {
    Kind::Executes {
        file,
        test,
        probe,
        runner: file,
    }
}

const C_TRUTH: &str = "crates/xpile/tests/c_truth_witness.rs";

const LEDGER: &[(&str, Kind)] = &[
    (
        "C-BASHRS-POSIX-IDEMPOTENCE",
        Kind::Residue {
            reason: "shell_diff_exec.rs runs each emitted script once and compares stdout to \
                     python3; idempotence (running it twice equals running it once) is never \
                     executed, only the re-emit fixed point is checked structurally",
        },
    ),
    (
        "C-C-FLOAT-ARITH",
        same_file(C_TRUTH, "c_truth_bridge_agrees_with_cc", "run"),
    ),
    (
        "C-C-INT-ARITH",
        same_file(C_TRUTH, "c_truth_bridge_agrees_with_cc", "run"),
    ),
    (
        "C-COMPILE-RUST-TO-PTX-MMA",
        Kind::Residue {
            reason: "gpu_witness.rs executes a saxpy kernel on the GPU; no test executes an \
                     mma (tensor-core) kernel",
        },
    ),
    (
        "C-COMPILE-RUST-TO-SPIRV",
        Kind::Executes {
            file: "crates/xpile-spirv-codegen/tests/gpu_witness.rs",
            test: "spirv_diffexec_executes_on_vulkan_and_matches",
            probe: "new_spirv_diffexec_witness",
            runner: "crates/xpile-spirv-codegen/src/spirv_diffexec.rs",
        },
    ),
    (
        "C-COMPILE-RUST-TO-WASM",
        same_file(
            "crates/xpile-wasm-codegen/tests/wasm_witness.rs",
            "wasm_list_index_executes_and_matches_cpython",
            "Command::new",
        ),
    ),
    (
        "C-COMPILE-RUST-TO-WGSL",
        same_file(
            "crates/xpile-wgsl-codegen/tests/gpu_real_kernel.rs",
            "clamp_scale_kernel_runs_on_vulkan_and_value_matches_cpython",
            "run_clamp_scale_on_gpu",
        ),
    ),
    (
        "C-COMPILE-SHELL-TO-FORJAR",
        Kind::Residue {
            reason: "forjar_validate_witness.rs runs `forjar validate` on the emitted YAML; \
                     nothing is applied, so no emitted resource is ever executed",
        },
    ),
    ("C-CONST-TRANSLATION", e2e("module_const")),
    ("C-ENUM-TRANSLATION", e2e("enum_basic")),
    (
        "C-FFI-CPYTHON-EXT",
        Kind::Residue {
            reason: "hybrid_verify.rs executes a ctypes boundary; no test builds or imports \
                     a CPython extension module",
        },
    ),
    (
        "C-FFI-SHELL-SUBPROCESS",
        same_file(
            "crates/xpile/tests/hybrid_verify.rs",
            "hybrid_verify_executes_the_flat_shell_boundary",
            "Command::new",
        ),
    ),
    (
        "C-NOTATION-LATEX-MATH-TO-EQUATION",
        Kind::Residue {
            reason: "notation tests compare parsed formulas in-process; no emitted output is run",
        },
    ),
    (
        "C-OLS-MODEL-UNIQUENESS",
        Kind::Residue {
            reason: "ols_recognition.rs checks recognition and citation in-process; no emitted \
                     `predict` is run",
        },
    ),
    ("C-PY-CONTEXT-MANAGER-EXIT", e2e("context_managers")),
    ("C-PY-EXCEPT-ALLOWLIST", e2e("multiple_except")),
    ("C-PY-FILE-IO-ROUNDTRIP", e2e("with_open")),
    ("C-PY-FLOAT-ARITH", e2e("float_floordiv_semantics")),
    ("C-PY-GENERATOR-EAGER", e2e("generators_eager")),
    (
        "C-PY-INT-ARITH",
        same_file(
            "crates/xpile/tests/diff_exec.rs",
            "differential_execution_cpython_vs_transpiled_rust",
            "run_rust",
        ),
    ),
    (
        "C-WASM-HEAP",
        same_file(
            "crates/xpile-wasm-codegen/tests/heap_dict_witness.rs",
            "dict_set_program_executes_in_wasm_and_matches_cpython",
            "assemble_and_run",
        ),
    ),
    (
        "C-XLATE-LEAN-TO-RUST",
        Kind::Residue {
            reason: "no frontend accepts .lean input, so there is nothing to execute",
        },
    ),
    ("C-XLATE-PY-BOOL-TO-RUST-BOOL", e2e("bool_cast")),
    (
        "C-XLATE-PY-CLASS-TO-STRUCT",
        e2e("dataclass_construction_and_field_access"),
    ),
    (
        "C-XLATE-PY-DICT-TO-HASHMAP",
        e2e("histogram_dict_ops_roundtrip"),
    ),
    (
        "C-XLATE-PY-LIST-TO-VEC",
        e2e("append_demo_emitted_rust_grows_list"),
    ),
    ("C-XLATE-PY-OPTIONAL-TO-OPTION", e2e("optional_return")),
    ("C-XLATE-PY-SET-TO-HASHSET", e2e("set_deterministic")),
    (
        "C-XLATE-PY-STR-TO-RUST-STRING",
        e2e("str_methods_emitted_rust_transforms_strings"),
    ),
    (
        "C-XLATE-PY-TUPLE-TO-RUST-TUPLE",
        e2e("tuples_emitted_rust_multiple_return"),
    ),
    (
        "C-XLATE-RUST-FN-TO-LEAN-THM",
        Kind::Residue {
            reason: "no Rust-to-Lean path exists (the Lean backend takes Python), so there is \
                     nothing to execute",
        },
    ),
    (
        "C-XPILE-BACKEND-TRAIT",
        Kind::InProcess {
            file: TRAITS,
            test: "every_backend_lower_is_deterministic_on_minimal_module",
        },
    ),
    (
        "C-XPILE-CONTRACT-BACKEND-TRAIT",
        Kind::InProcess {
            file: TRAITS,
            test: "every_contract_backend_render_is_deterministic_on_minimal_contract",
        },
    ),
    (
        "C-XPILE-CONTRACT-FRONTEND-TRAIT",
        Kind::InProcess {
            file: TRAITS,
            test: "every_contract_frontend_parse_is_deterministic_on_minimal_source",
        },
    ),
    (
        "C-XPILE-FRONTEND-TRAIT",
        Kind::InProcess {
            file: TRAITS,
            test: "frontend_extensions_are_disjoint_across_registered_impls",
        },
    ),
];

/// Code that starts a process or a GPU instance. A runner file must contain
/// one of these on a code line.
const RUNNERS: &[&str] = &["Command::new(", "wgpu::Instance::new("];

/// The sentence `audit-design.md` publishes; `{r}` and `{n}` are filled from
/// the ledger.
const AUDIT_DOC: &str = "docs/specifications/audit-design.md";

fn published_sentence(residue: usize, total: usize) -> String {
    format!(
        "{residue} of {total} contracts have no property-specific Runtime witness \
         (enumerated by `runtime_property_ledger_witness.rs`)"
    )
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn is_code_line(line: &str) -> bool {
    let t = line.trim_start();
    !t.is_empty() && !t.starts_with("//")
}

/// The contract ids declared by `contracts/*.yaml`: each file's
/// `metadata.id`, read as the first line whose trimmed text starts `id: C-`.
fn derived_contract_ids() -> BTreeSet<String> {
    let dir = repo_root().join("contracts");
    let mut ids = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("read contracts/") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read contract yaml");
        let id = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("id: "))
            .filter(|id| id.starts_with("C-"))
            .unwrap_or_else(|| panic!("{} declares no `id: C-…`", path.display()));
        ids.insert(id.trim().to_string());
    }
    ids
}

/// The body of `#[test] fn <name>()`, brace-matched. Strings, raw strings,
/// char literals and comments are skipped while matching, because test
/// drivers embed `fn main() { … }` with `}` at column 0 inside raw strings,
/// which ends a naive `^}` or next-`fn` scan early.
fn test_body<'a>(src: &'a str, name: &str) -> Result<&'a str, String> {
    let sig = format!("fn {name}()");
    let start = src
        .find(&sig)
        .ok_or_else(|| format!("no `{sig}` in the file"))?;
    let attrs: Vec<&str> = src[..start].lines().rev().take(4).collect();
    if !attrs.iter().any(|l| l.trim() == "#[test]") {
        return Err(format!("`{sig}` is not a #[test]"));
    }
    let end = body_end(src.as_bytes(), start).ok_or_else(|| format!("`{sig}` never closes"))?;
    Ok(&src[start..end])
}

/// The byte index just past the `}` closing the first `{` at or after `from`.
fn body_end(b: &[u8], from: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'r' if matches!(b.get(i + 1), Some(b'#' | b'"'))
                && !(i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) =>
            {
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                if b.get(i + 1 + hashes) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let mut close = vec![b'"'];
                close.extend(std::iter::repeat_n(b'#', hashes));
                i += 2 + hashes;
                while i < b.len() && !b[i..].starts_with(&close) {
                    i += 1;
                }
                i += close.len() - 1;
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'\'' if b.get(i + 2) == Some(&b'\'') => i += 2,
            b'\'' if b.get(i + 1) == Some(&b'\\') => {
                i += 2;
                while i < b.len() && b[i] != b'\'' {
                    i += 1;
                }
            }
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Whether `body` calls `probe` on a code line, as a whole identifier (so
/// `run` does not match inside `cc_run`).
fn calls_probe(body: &str, probe: &str) -> bool {
    let needle = format!("{probe}(");
    body.lines().filter(|l| is_code_line(l)).any(|l| {
        l.match_indices(&needle).any(|(i, _)| {
            !l[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    })
}

fn has_runner(src: &str) -> bool {
    src.lines()
        .filter(|l| is_code_line(l))
        .any(|l| RUNNERS.iter().any(|r| l.contains(r)))
}

fn check_executes(test_src: &str, test: &str, probe: &str, runner_src: &str) -> Result<(), String> {
    let body = test_body(test_src, test)?;
    if !calls_probe(body, probe) {
        return Err(format!("`{test}` never calls `{probe}(` on a code line"));
    }
    if !has_runner(runner_src) {
        return Err(format!("the runner file for `{test}` spawns nothing"));
    }
    Ok(())
}

fn residue_count() -> usize {
    LEDGER
        .iter()
        .filter(|(_, k)| matches!(k, Kind::Residue { .. }))
        .count()
}

#[test]
fn every_contract_is_classified_exactly_once() {
    let derived = derived_contract_ids();
    assert!(
        !derived.is_empty(),
        "no contract ids derived from contracts/"
    );
    let mut seen = BTreeSet::new();
    for (id, _) in LEDGER {
        assert!(seen.insert(id.to_string()), "{id} has two ledger rows");
    }
    let missing: Vec<_> = derived.difference(&seen).collect();
    let stale: Vec<_> = seen.difference(&derived).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "ledger drifted from contracts/*.yaml: no row for {missing:?}; row for a missing \
         contract {stale:?}"
    );
}

#[test]
fn every_executing_witness_reaches_a_process() {
    let rows: Vec<_> = LEDGER
        .iter()
        .filter_map(|(id, k)| match k {
            Kind::Executes {
                file,
                test,
                probe,
                runner,
            } => Some((id, file, test, probe, runner)),
            _ => None,
        })
        .collect();
    assert!(!rows.is_empty(), "no Executes rows");
    let failures: Vec<String> = rows
        .iter()
        .filter_map(|(id, file, test, probe, runner)| {
            check_executes(&read(file), test, probe, &read(runner))
                .err()
                .map(|e| format!("{id}: {e}"))
        })
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_in_process_witness_names_its_contract() {
    let rows: Vec<_> = LEDGER
        .iter()
        .filter_map(|(id, k)| match k {
            Kind::InProcess { file, test } => Some((id, file, test)),
            _ => None,
        })
        .collect();
    assert!(!rows.is_empty(), "no InProcess rows");
    for (id, file, test) in rows {
        let src = read(file);
        test_body(&src, test).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(src.contains(*id), "{id}: {file} never names the contract");
    }
}

#[test]
fn every_residue_row_gives_a_reason() {
    for (id, k) in LEDGER {
        if let Kind::Residue { reason } = k {
            assert!(reason.len() > 20, "{id}: residue reason is too short");
        }
    }
}

#[test]
fn audit_design_publishes_the_derived_residue_count() {
    let total = derived_contract_ids().len();
    let want = published_sentence(residue_count(), total);
    let doc = read(AUDIT_DOC)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        doc.contains(&want),
        "{AUDIT_DOC} must say: \"{want}\" (count derived from the ledger)"
    );
}

// ---- RED arms: each check refuses the shape it exists to catch. ----

const SPAWNING_FILE: &str = r##"
fn helper() { let _ = Command::new("rustc"); }

#[test]
fn good() {
    let driver = r#"
fn main() {
}
"#;
    helper(driver);
}

#[test]
fn commented() {
    // helper(x);
    let _ = 1;
}

fn not_a_test() {
    helper();
}
"##;

#[test]
fn red_arm_the_probe_must_be_called_on_a_code_line() {
    assert!(check_executes(SPAWNING_FILE, "good", "helper", SPAWNING_FILE).is_ok());
    let err = check_executes(SPAWNING_FILE, "commented", "helper", SPAWNING_FILE).unwrap_err();
    assert!(err.contains("never calls `helper(`"), "{err}");
}

#[test]
fn red_arm_the_probe_must_be_a_whole_identifier() {
    let err = check_executes(SPAWNING_FILE, "good", "per", SPAWNING_FILE).unwrap_err();
    assert!(err.contains("never calls `per(`"), "{err}");
}

#[test]
fn red_arm_the_test_must_exist_and_be_a_test() {
    let err = check_executes(SPAWNING_FILE, "absent", "helper", SPAWNING_FILE).unwrap_err();
    assert!(err.contains("no `fn absent()`"), "{err}");
    let err = check_executes(SPAWNING_FILE, "not_a_test", "helper", SPAWNING_FILE).unwrap_err();
    assert!(err.contains("is not a #[test]"), "{err}");
}

#[test]
fn red_arm_a_runner_that_spawns_only_in_a_comment_is_refused() {
    let runner = "// Command::new(\"rustc\")\nfn f() {}\n";
    let err = check_executes(SPAWNING_FILE, "good", "helper", runner).unwrap_err();
    assert!(err.contains("spawns nothing"), "{err}");
}

#[test]
fn red_arm_the_published_sentence_moves_with_the_count() {
    let total = derived_contract_ids().len();
    let now = published_sentence(residue_count(), total);
    let one_more = published_sentence(residue_count() + 1, total);
    assert_ne!(now, one_more);
    let doc = read(AUDIT_DOC)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !doc.contains(&one_more),
        "the doc also carries a stale count"
    );
}
