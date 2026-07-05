//! forjar.yaml IaC backend — the BACKEND-ONLY forjar integration
//! (PMAT-953).
//!
//! `/home/noah/src/forjar` is a paiml sibling: a single-binary pure-Rust
//! IaC execution engine (`forjar.yaml → DAG → plan → guarded bash → apply`).
//! It is a *consumer/runtime*, NOT a transpiler — so the integration is
//! **backend-only** (a new [`Target::ForjarYaml`] emitting forjar.yaml
//! text), exactly like bashrs-backend's Makefile/Dockerfile output lane,
//! and explicitly NOT a merge/federation (no IR to share; a runtime dep
//! would recreate the build/MSRV coupling the bashrs reversal killed). See
//! the `project-forjar-output-backend` memory for the full decision.
//!
//! ## What it lowers (clean cells only)
//!
//! The input must be a **SHELL-origin** [`Module`] (`SourceLang::Shell`) —
//! the `bashrs-frontend` lowers a `.sh` file to a synthetic `main`
//! [`Function`](xpile_meta_hir::Function) whose body is a sequence of
//! [`Stmt::Cmd`] / [`Stmt::Pipeline`] / [`Stmt::ShellAssign`]. From that
//! command sequence the backend emits:
//!
//! - **A single bare command** → a forjar `type: task` resource
//!   (`command: "<program args…>"`). forjar runs it (and, at apply time,
//!   wraps it in its own idempotence/convergence machinery).
//! - **A multi-command script body** → a forjar `type: file` resource
//!   (`path`, `mode: '0755'`, `content: |` the reconstructed POSIX script,
//!   one line per `Stmt`). forjar materialises the script file
//!   idempotently; a sibling `type: task` invokes it.
//!
//! Every manifest carries the canonical envelope: `version: "1.0"`,
//! `name`, `machines: { localhost: { hostname: localhost, addr: localhost } }`,
//! `resources:`.
//!
//! ## What it REFUSES (never wrong YAML — Lean-style honest refusal)
//!
//! Per the memory's lossy list, the backend returns a hard
//! [`BackendError::Lower`] (never emits speculative/incorrect YAML) for:
//!
//! - **Non-shell modules** (`SourceLang::Python`/`Rust`/… value-level
//!   functions that aren't a command sequence) — forjar.yaml is the
//!   ops/deployment lane only, like the Makefile/Dockerfile bashrs lane.
//! - **Any shell conditional / idempotence guard** — the meta-HIR shell
//!   lane has no `Stmt::ShellIf` / `Expr::ShellTest`, so forjar's
//!   convergence GUARDS can't be expressed. Only *unconditional*
//!   resources emit; forjar re-adds convergence at apply time.
//!   `Stmt::ShellLoop` / `Stmt::If` / `Stmt::While` are refused.
//! - **Non-renderable command args** (anything outside the shell `Expr`
//!   surface).
//!
//! ## Provability interlock
//!
//! xpile owns `C-BASHRS-POSIX-IDEMPOTENCE` (shell round-trip) and
//! `C-COMPILE-SHELL-TO-FORJAR` (this lowering, `contracts/
//! compile-shell-to-forjar-v1.yaml`); forjar owns `idempotent-apply` /
//! `plan-apply-equivalence` (apply-convergence). They hand off at the
//! YAML boundary — xpile emits correctly, forjar proves convergence.

use std::fmt::Write as _;

use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, QuorumStatus, Target};
use xpile_contracts::ContractId;
use xpile_meta_hir::{Expr, Function, Item, Module, QuotingStrategy, SourceLang, Stmt};

/// The Layer-5 compile contract every emitted forjar.yaml cites.
const CONTRACT_ID: &str = "C-COMPILE-SHELL-TO-FORJAR";

/// forjar.yaml backend. Single-emitter (no §29 quorum) — the executed
/// two-emitter forjar witness (emit + `forjar plan`/`apply`) is owned by
/// forjar itself at the YAML boundary, not by this backend.
#[derive(Default)]
pub struct ForjarBackend;

impl ForjarBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Backend for ForjarBackend {
    fn name(&self) -> &'static str {
        "forjar"
    }

    fn targets(&self) -> &[Target] {
        &[Target::ForjarYaml]
    }

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        if config.target != Target::ForjarYaml {
            return Err(BackendError::UnsupportedTarget(config.target));
        }
        let yaml = emit_manifest(module)?;
        Ok(Artifact {
            primary: yaml,
            sidecars: Vec::new(),
            citations: vec![ContractId::new(CONTRACT_ID)],
            quorum_status: QuorumStatus::Single {
                emitter: "xpile-forjar-codegen".to_string(),
            },
        }
        .with_citations(config.emit_contracts))
    }
}

fn unsupported(what: &str) -> BackendError {
    BackendError::Lower(format!("xpile-forjar-codegen: refused — {what}"))
}

/// Emit a full forjar.yaml manifest for `module`.
///
/// Refuses any non-shell module up front (forjar.yaml is the ops lane).
pub fn emit_manifest(module: &Module) -> Result<String, BackendError> {
    if module.source_lang != SourceLang::Shell {
        return Err(unsupported(&format!(
            "module `{}` is a {:?}-origin module, not a SHELL-origin command \
             sequence. forjar.yaml is the ops/deployment lane only (like the \
             Makefile/Dockerfile bashrs lane); value-level Rust/Python functions \
             have no forjar resource representation",
            module.name, module.source_lang
        )));
    }

    // A SHELL-origin module is a single synthetic `main` function holding
    // the command sequence (bashrs-frontend shape). An empty module (no
    // commands) has nothing to materialise.
    let func = single_shell_function(module)?;

    // Reconstruct each body statement into a POSIX shell command line,
    // refusing any control-flow / conditional / idempotence-guard shape
    // (the lossy cases the meta-HIR shell lane cannot express).
    let lines = render_command_sequence(&func.body.stmts)?;
    if lines.is_empty() {
        return Err(unsupported(&format!(
            "shell module `{}` lowered to zero commands — nothing to emit as a \
             forjar resource",
            module.name
        )));
    }

    let stack_name = sanitize_name(&module.name);
    let mut out = String::new();
    writeln!(out, "# xpile-forjar-codegen — generated forjar.yaml").expect("write");
    writeln!(
        out,
        "# source module: {} ({:?})",
        module.name, module.source_lang
    )
    .expect("write");
    writeln!(out, "# contract: {CONTRACT_ID}").expect("write");
    writeln!(out, "version: \"1.0\"").expect("write");
    writeln!(out, "name: {stack_name}").expect("write");
    writeln!(out, "machines:").expect("write");
    writeln!(out, "  localhost:").expect("write");
    // forjar's Machine schema REQUIRES `hostname` (verified via `forjar
    // validate`: emitting only `addr` fails with "missing field `hostname`").
    // Emit both, matching forjar's own examples (dist-forjar.yaml).
    writeln!(out, "    hostname: localhost").expect("write");
    writeln!(out, "    addr: localhost").expect("write");
    writeln!(out, "resources:").expect("write");

    if lines.len() == 1 {
        // Single bare command → a `type: task` resource. forjar runs the
        // command (wrapping it in its own convergence machinery at apply
        // time).
        let task_id = format!("{stack_name}-task");
        writeln!(out, "  {task_id}:").expect("write");
        writeln!(out, "    type: task").expect("write");
        writeln!(out, "    machine: localhost").expect("write");
        // forjar.yaml command is a scalar string; emit it as a
        // double-quoted YAML scalar with `"` escaped.
        writeln!(out, "    command: {}", yaml_scalar(&lines[0])).expect("write");
        writeln!(
            out,
            "    # xpile-contract: {CONTRACT_ID} (single bare command → task)"
        )
        .expect("write");
    } else {
        // Multi-command script body → a `type: file` resource holding the
        // reconstructed POSIX script, plus a `type: task` that runs it.
        let script_path = format!("/usr/local/bin/{stack_name}.sh");
        let file_id = format!("{stack_name}-script");
        let task_id = format!("{stack_name}-run");

        writeln!(out, "  {file_id}:").expect("write");
        writeln!(out, "    type: file").expect("write");
        writeln!(out, "    machine: localhost").expect("write");
        writeln!(out, "    path: {script_path}").expect("write");
        writeln!(out, "    mode: '0755'").expect("write");
        writeln!(out, "    content: |").expect("write");
        // YAML literal block scalar — indent each script line by 6 spaces
        // (4 for the mapping nesting + 2 for the block). A shebang makes
        // the materialised file directly executable.
        writeln!(out, "      #!/bin/sh").expect("write");
        for line in &lines {
            writeln!(out, "      {line}").expect("write");
        }
        writeln!(
            out,
            "    # xpile-contract: {CONTRACT_ID} (script body → file)"
        )
        .expect("write");

        writeln!(out, "  {task_id}:").expect("write");
        writeln!(out, "    type: task").expect("write");
        writeln!(out, "    machine: localhost").expect("write");
        writeln!(out, "    command: {}", yaml_scalar(&script_path)).expect("write");
        writeln!(out, "    depends_on: [{file_id}]").expect("write");
        writeln!(
            out,
            "    # xpile-contract: {CONTRACT_ID} (run the materialised script)"
        )
        .expect("write");
    }

    Ok(out)
}

/// Extract the single synthetic shell function (`main`) from a SHELL-origin
/// module. Refuses an empty module, or a module with a different item shape
/// than the bashrs-frontend `Item::Function` wrapper.
fn single_shell_function(module: &Module) -> Result<&Function, BackendError> {
    let mut funcs = module.items.iter().filter_map(|i| match i {
        Item::Function(f) => Some(f),
        _ => None,
    });
    let func = funcs.next().ok_or_else(|| {
        unsupported(&format!(
            "shell module `{}` has no function body (empty script) — nothing to \
             emit",
            module.name
        ))
    })?;
    if funcs.next().is_some() {
        return Err(unsupported(&format!(
            "shell module `{}` has multiple functions; the forjar lane expects the \
             single bashrs-frontend `main` command-sequence wrapper",
            module.name
        )));
    }
    Ok(func)
}

/// Reconstruct a body statement sequence into POSIX shell command lines,
/// refusing every control-flow / conditional / idempotence-guard shape.
fn render_command_sequence(stmts: &[Stmt]) -> Result<Vec<String>, BackendError> {
    let mut lines = Vec::new();
    for s in stmts {
        match s {
            Stmt::Cmd { program, args } => {
                lines.push(render_cmd(program, args)?);
            }
            Stmt::Pipeline { stages } => {
                lines.push(render_pipeline(stages)?);
            }
            Stmt::ShellAssign { name, value } => {
                lines.push(format!("{name}={}", render_arg(value)?));
            }
            // The lossy cases the memory names: the meta-HIR shell lane has
            // no `Stmt::ShellIf` / `Expr::ShellTest`, so forjar's
            // idempotence GUARDS cannot be expressed. A loop or conditional
            // is exactly such a guard — refuse rather than emit YAML that
            // silently drops the control structure.
            Stmt::ShellLoop { .. } => {
                return Err(unsupported(
                    "a shell loop (`for`/`while`/`until`) — control flow is an \
                     idempotence guard the meta-HIR shell lane cannot represent \
                     declaratively; forjar re-adds convergence at apply time, so \
                     only unconditional resources emit",
                ));
            }
            Stmt::If { .. } | Stmt::While { .. } => {
                return Err(unsupported(
                    "a shell conditional / loop — the meta-HIR shell lane has no \
                     `Stmt::ShellIf`/`Expr::ShellTest`, so an idempotence guard \
                     cannot be lowered to a forjar resource (never emit wrong YAML)",
                ));
            }
            other => {
                return Err(unsupported(&format!(
                    "non-command statement {other:?} in a shell module — the forjar \
                     lane lowers only `Stmt::Cmd`/`Stmt::Pipeline`/`Stmt::ShellAssign`"
                )));
            }
        }
    }
    Ok(lines)
}

/// Render a `Stmt::Cmd` to a `program arg1 arg2 …` shell line.
fn render_cmd(program: &str, args: &[Expr]) -> Result<String, BackendError> {
    if args.is_empty() {
        return Ok(program.to_string());
    }
    let rendered: Result<Vec<String>, BackendError> = args.iter().map(render_arg).collect();
    Ok(format!("{program} {}", rendered?.join(" ")))
}

/// Render a `Stmt::Pipeline` to a `cmd1 | cmd2 | …` shell line. Every
/// stage must be a `Stmt::Cmd` (the bashrs-frontend invariant).
fn render_pipeline(stages: &[Stmt]) -> Result<String, BackendError> {
    let mut rendered = Vec::with_capacity(stages.len());
    for stage in stages {
        let Stmt::Cmd { program, args } = stage else {
            return Err(unsupported(
                "a pipeline stage that is not a `Stmt::Cmd`; the forjar lane \
                 reconstructs only command pipelines",
            ));
        };
        rendered.push(render_cmd(program, args)?);
    }
    Ok(rendered.join(" | "))
}

/// Render a single command arg into its POSIX shell surface form. Mirrors
/// `bashrs-backend::render_arg` for the shell `Expr` surface; refuses any
/// value-level `Expr` (which a shell module never carries).
fn render_arg(e: &Expr) -> Result<String, BackendError> {
    match e {
        Expr::LitStr(s) => Ok(s.clone()),
        Expr::QuotedString { content, quoting } => Ok(match quoting {
            QuotingStrategy::Single => format!("'{content}'"),
            QuotingStrategy::Double => format!("\"{content}\""),
            QuotingStrategy::Backslash => content
                .chars()
                .map(|c| format!("\\{c}"))
                .collect::<String>(),
        }),
        Expr::ShellVar(name) => Ok(format!("${name}")),
        Expr::ShellSpecial(name) => Ok(format!("${name}")),
        Expr::CommandSubstitution(inner) => {
            let Stmt::Cmd { program, args } = inner.as_ref() else {
                return Err(unsupported(
                    "command substitution wrapping a non-`Stmt::Cmd` statement",
                ));
            };
            Ok(format!("$({})", render_cmd(program, args)?))
        }
        other => Err(unsupported(&format!(
            "non-shell command arg {other:?} — the forjar lane reconstructs only the \
             shell `Expr` surface (LitStr / QuotedString / ShellVar / ShellSpecial / \
             CommandSubstitution)"
        ))),
    }
}

/// Sanitize a module name into a forjar stack/resource id: lowercase, with
/// every non-`[a-z0-9-]` char collapsed to `-`. Guarantees a non-empty id.
fn sanitize_name(raw: &str) -> String {
    let collapsed: String = raw
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Trim leading/trailing dashes; fall back to a stable default.
    let trimmed = collapsed.trim_matches('-');
    if trimmed.is_empty() {
        "shell-stack".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Render a string as a double-quoted YAML scalar (escaping `\` and `"`).
/// forjar's `command:` field is a plain string; double-quoting keeps a
/// command containing spaces / special chars a single scalar.
fn yaml_scalar(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests;
