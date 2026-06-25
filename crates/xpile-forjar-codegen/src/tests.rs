//! Unit + structural-golden tests for the forjar.yaml backend (PMAT-953).
//!
//! Covers the supported meta-HIR→forjar-resource mappings (single bare
//! command → `type: task`; multi-command script body → `type: file` +
//! `type: task`; `ShellAssign` rendering), the honest refusals (non-shell
//! modules, shell conditionals / loops, empty modules), and a STRUCTURAL
//! golden validation that round-trips the emitted forjar.yaml back through
//! a YAML parser and asserts the resource shape.
//!
//! The forjar-crate golden validation (forjar's own `parse_config` /
//! `validate_config`) is evaluated separately and DEFERRED — the shipping
//! crate takes NO runtime forjar dependency (backend-only), so this module
//! validates the emitted YAML structurally instead.

use super::*;
use serde_yaml::Value;
use xpile_backend::{BackendConfig, Profile};
use xpile_meta_hir::{
    Block, Expr, Function, Item, Module, QuotingStrategy, SourceLang, Stmt, Type,
};

// ─── builders ───────────────────────────────────────────────────────

fn shell_module(name: &str, stmts: Vec<Stmt>) -> Module {
    let func = Function {
        name: "main".into(),
        params: Vec::new(),
        return_type: Type::I64,
        body: Block {
            stmts,
            trailing_return: Expr::LitInt(0),
        },
    };
    Module {
        name: name.into(),
        source_lang: SourceLang::Shell,
        items: vec![Item::Function(func)],
        ffi_boundaries: Vec::new(),
    }
}

fn cmd(program: &str, args: Vec<Expr>) -> Stmt {
    Stmt::Cmd {
        program: program.into(),
        args,
    }
}

fn lit(s: &str) -> Expr {
    Expr::LitStr(s.into())
}

fn forjar_config() -> BackendConfig {
    BackendConfig {
        target: Target::ForjarYaml,
        profile: Profile::RustOut,
        hardware: None,
    }
}

fn parse_yaml(text: &str) -> Value {
    serde_yaml::from_str(text).expect("emitted forjar.yaml must parse as YAML")
}

// ─── supported mappings ─────────────────────────────────────────────

#[test]
fn single_bare_command_lowers_to_a_task_resource() {
    let m = shell_module(
        "deploy",
        vec![cmd("systemctl", vec![lit("restart"), lit("nginx")])],
    );
    let yaml = emit_manifest(&m).expect("single command lowers cleanly");

    // Envelope.
    assert!(yaml.contains("version: \"1.0\""));
    assert!(yaml.contains("name: deploy"));
    assert!(yaml.contains("machines:"));
    assert!(yaml.contains("addr: localhost"));

    // Structural golden: parse it back and assert the resource shape.
    let v = parse_yaml(&yaml);
    assert_eq!(v["version"].as_str(), Some("1.0"));
    assert_eq!(v["name"].as_str(), Some("deploy"));
    let resources = v["resources"].as_mapping().expect("resources mapping");
    assert_eq!(resources.len(), 1, "one bare command → one task resource");
    let task = &v["resources"]["deploy-task"];
    assert_eq!(task["type"].as_str(), Some("task"));
    assert_eq!(task["machine"].as_str(), Some("localhost"));
    assert_eq!(
        task["command"].as_str(),
        Some("systemctl restart nginx"),
        "command scalar reconstructs the shell line"
    );
}

#[test]
fn multi_command_body_lowers_to_a_file_plus_task() {
    let m = shell_module(
        "provision",
        vec![
            cmd("mkdir", vec![lit("-p"), lit("/opt/app")]),
            cmd("chown", vec![lit("app:app"), lit("/opt/app")]),
            cmd("systemctl", vec![lit("enable"), lit("app")]),
        ],
    );
    let yaml = emit_manifest(&m).expect("multi-command body lowers cleanly");

    let v = parse_yaml(&yaml);
    let resources = v["resources"].as_mapping().expect("resources mapping");
    assert_eq!(resources.len(), 2, "script body → file + task");

    let file = &v["resources"]["provision-script"];
    assert_eq!(file["type"].as_str(), Some("file"));
    assert_eq!(file["mode"].as_str(), Some("0755"));
    assert_eq!(file["path"].as_str(), Some("/usr/local/bin/provision.sh"));
    let content = file["content"].as_str().expect("content block scalar");
    assert!(content.starts_with("#!/bin/sh"), "script has a shebang");
    assert!(content.contains("mkdir -p /opt/app"));
    assert!(content.contains("chown app:app /opt/app"));
    assert!(content.contains("systemctl enable app"));

    let task = &v["resources"]["provision-run"];
    assert_eq!(task["type"].as_str(), Some("task"));
    assert_eq!(
        task["command"].as_str(),
        Some("/usr/local/bin/provision.sh")
    );
    // depends_on the file resource (forjar materialises it first).
    let deps = task["depends_on"].as_sequence().expect("depends_on list");
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].as_str(), Some("provision-script"));
}

#[test]
fn shell_assign_renders_in_a_script_body() {
    let m = shell_module(
        "envcfg",
        vec![
            Stmt::ShellAssign {
                name: "TARGET".into(),
                value: lit("/srv/www"),
            },
            cmd("install", vec![lit("-d"), Expr::ShellVar("TARGET".into())]),
        ],
    );
    let yaml = emit_manifest(&m).expect("assign + cmd lowers cleanly");
    let v = parse_yaml(&yaml);
    let content = v["resources"]["envcfg-script"]["content"]
        .as_str()
        .expect("content");
    assert!(content.contains("TARGET=/srv/www"));
    assert!(
        content.contains("install -d $TARGET"),
        "ShellVar arg renders as $NAME"
    );
}

#[test]
fn pipeline_reconstructs_as_a_piped_command_line() {
    // A single pipeline statement is "one line" → a task resource.
    let m = shell_module(
        "logscan",
        vec![Stmt::Pipeline {
            stages: vec![
                cmd("cat", vec![lit("/var/log/app.log")]),
                cmd("grep", vec![lit("ERROR")]),
            ],
        }],
    );
    let yaml = emit_manifest(&m).expect("pipeline lowers cleanly");
    let v = parse_yaml(&yaml);
    assert_eq!(
        v["resources"]["logscan-task"]["command"].as_str(),
        Some("cat /var/log/app.log | grep ERROR")
    );
}

#[test]
fn quoted_arg_renders_with_its_quoting_strategy() {
    let m = shell_module(
        "msg",
        vec![cmd(
            "echo",
            vec![Expr::QuotedString {
                content: "hello world".into(),
                quoting: QuotingStrategy::Double,
            }],
        )],
    );
    let yaml = emit_manifest(&m).expect("quoted arg lowers");
    let v = parse_yaml(&yaml);
    assert_eq!(
        v["resources"]["msg-task"]["command"].as_str(),
        Some("echo \"hello world\"")
    );
}

#[test]
fn backend_trait_lower_emits_citation_and_quorum() {
    let m = shell_module("svc", vec![cmd("true", vec![])]);
    let artifact = ForjarBackend::new()
        .lower(&m, &forjar_config())
        .expect("backend lowers");
    assert_eq!(artifact.citations.len(), 1);
    assert_eq!(artifact.citations[0].as_str(), CONTRACT_ID);
    assert!(matches!(
        artifact.quorum_status,
        QuorumStatus::Single { .. }
    ));
    assert!(artifact.primary.contains("type: task"));
}

#[test]
fn backend_targets_only_forjar_yaml() {
    assert_eq!(ForjarBackend::new().targets(), &[Target::ForjarYaml]);
}

#[test]
fn module_name_is_sanitized_into_a_forjar_id() {
    let m = shell_module("My App!!", vec![cmd("true", vec![])]);
    let yaml = emit_manifest(&m).expect("lowers");
    let v = parse_yaml(&yaml);
    // "My App!!" → "my-app" (lowercased, non-alnum collapsed, trimmed).
    assert_eq!(v["name"].as_str(), Some("my-app"));
    assert!(v["resources"]["my-app-task"]["type"].as_str() == Some("task"));
}

// ─── honest refusals (never wrong YAML) ─────────────────────────────

#[test]
fn refuses_a_non_shell_module() {
    // A Python/Rust value-level module is NOT an ops command sequence.
    let m = Module {
        name: "calc".into(),
        source_lang: SourceLang::Python,
        items: vec![Item::Function(Function {
            name: "add".into(),
            params: Vec::new(),
            return_type: Type::I64,
            body: Block {
                stmts: Vec::new(),
                trailing_return: Expr::LitInt(0),
            },
        })],
        ffi_boundaries: Vec::new(),
    };
    let err = emit_manifest(&m).expect_err("non-shell module must be refused");
    let msg = err.to_string();
    assert!(msg.contains("ops/deployment lane"), "got: {msg}");
}

#[test]
fn refuses_a_shell_loop_idempotence_guard() {
    use xpile_meta_hir::LoopKind;
    let m = shell_module(
        "looped",
        vec![Stmt::ShellLoop {
            kind: LoopKind::For {
                var: "f".into(),
                items: vec![lit("a"), lit("b")],
            },
            body: vec![cmd("process", vec![Expr::ShellVar("f".into())])],
        }],
    );
    let err = emit_manifest(&m).expect_err("a shell loop must be refused");
    assert!(err.to_string().contains("idempotence guard"));
}

#[test]
fn refuses_a_shell_conditional() {
    let m = shell_module(
        "guarded",
        vec![Stmt::If {
            cond: Expr::LitBool(true),
            then_body: vec![cmd("touch", vec![lit("/tmp/x")])],
            else_body: vec![],
        }],
    );
    let err = emit_manifest(&m).expect_err("a conditional must be refused");
    assert!(err.to_string().contains("never emit wrong YAML"));
}

#[test]
fn refuses_an_empty_shell_module() {
    let m = Module {
        name: "empty".into(),
        source_lang: SourceLang::Shell,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    };
    let err = emit_manifest(&m).expect_err("empty module must be refused");
    assert!(err.to_string().contains("empty script"));
}

#[test]
fn backend_refuses_wrong_target() {
    let m = shell_module("svc", vec![cmd("true", vec![])]);
    let cfg = BackendConfig {
        target: Target::Rust,
        profile: Profile::RustOut,
        hardware: None,
    };
    let err = ForjarBackend::new()
        .lower(&m, &cfg)
        .expect_err("wrong target must be refused");
    assert!(matches!(err, BackendError::UnsupportedTarget(Target::Rust)));
}

// ─── structural golden: full round-trip validation ──────────────────

#[test]
fn structural_golden_full_round_trip_shape() {
    // The cheapest real proof the schema is plausible WITHOUT a runtime
    // forjar dependency: emit → parse-as-YAML → assert the top-level
    // envelope keys + machine + every resource carries a `type`. (The
    // forjar-crate `validate_config` golden — semantic validation against
    // forjar's own ForjarConfig — is deferred; see the report.)
    let m = shell_module(
        "stack",
        vec![
            cmd("apt-get", vec![lit("update")]),
            cmd("apt-get", vec![lit("install"), lit("-y"), lit("nginx")]),
        ],
    );
    let yaml = emit_manifest(&m).expect("lowers");
    let v = parse_yaml(&yaml);

    // Envelope keys forjar requires.
    assert!(v.get("version").is_some(), "version present");
    assert!(v.get("name").is_some(), "name present");
    assert!(v.get("machines").is_some(), "machines present");
    assert!(v.get("resources").is_some(), "resources present");

    // localhost machine with an addr.
    assert_eq!(
        v["machines"]["localhost"]["addr"].as_str(),
        Some("localhost")
    );

    // Every resource value is a mapping carrying a `type`.
    for (id, res) in v["resources"].as_mapping().expect("resources mapping") {
        let ty = res
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("resource {id:?} missing `type`"));
        assert!(
            matches!(ty, "file" | "task" | "cron"),
            "resource {id:?} has an unexpected forjar type {ty:?}"
        );
        assert_eq!(
            res["machine"].as_str(),
            Some("localhost"),
            "resource {id:?} pins the localhost machine"
        );
    }
}
