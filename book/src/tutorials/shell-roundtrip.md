# Tutorial: POSIX shell round-trip

> **Governing contract:** [`C-BASHRS-POSIX-IDEMPOTENCE`](../reference/contracts.md#c-bashrs-posix-idempotence)
> — Layer 1 (semantics), code lane, kind: pattern.
> The invariants it pins: `subprocess_run_equals_shell_run`,
> `exit_code_consistency` —
> the supported POSIX-shell subset, and the cross-domain Python↔shell
> flow.

This tutorial walks through xpile's **same-language round-trip** for
POSIX shell: parse `.sh`, lower into meta-HIR, emit POSIX shell again.
It's the simplest demonstration of the bashrs merger that landed at
v0.1.0 (PMAT-037..119).

## 1. The source

```sh
#!/usr/bin/env sh
mkdir -p /tmp/out
echo "hello" > /tmp/out/file.txt
```

Save this as `script.sh`. Two POSIX commands, idempotent by virtue of
`mkdir -p` and the `>` redirect.

## 2. Round-trip

```bash
$ xpile transpile script.sh --target shell
#!/bin/sh
# xpile-bashrs-backend (v0.1.0 PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B)
# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE
# module: script
mkdir -p /tmp/out
echo "hello" > /tmp/out/file.txt
```

The emitted file:

- carries a `# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE` citation,
- preserves the shebang (normalised to `#!/bin/sh` — the supported
  POSIX dialect),
- emits each `Cmd` statement back in its original form, and
- adds a backend-identifier comment so users know which lane produced
  the file.

## 3. Cross-domain: Python → shell

The bashrs merger isn't just about same-language round-trip; it also
enables **cross-domain transpiles** like Python → shell, where a
Python `subprocess.run([...])` call lowers to a `Cmd` statement and
emits real POSIX shell. (Stricter Python subsets fall in scope here as
the work item series continues; see PMAT-040..052 for the lowering
roster.)

Conversely, asking the **Rust** backend to lower a shell-only construct
fails cleanly:

```bash
$ xpile transpile script.sh --target rust
Error: backend `rust` failed

Caused by:
    lowering error: unsupported item: Rust backend does not lower
    Stmt::Cmd (`mkdir` with 2 arg(s)) — contract
    C-BASHRS-POSIX-IDEMPOTENCE governs this construct; use
    `--target shell` to emit POSIX sh via bashrs-backend
```

This message **names the governing contract** and **suggests the
correct target**, and `rust` is one of the backends whose refusals do.
It is a good message, not an instance of a general rule: most probed
refusals name neither, and five of the nine backends name neither in
any of theirs. The measured counts live in exactly one place —
[Backends → Status](../reference/backends.md#status).

`C-XPILE-BACKEND-TRAIT` does not require either half; every one of its
equations is a property of the **emitted** artifact. Through v0.1.617
this paragraph called this message an instance of that contract's
"structural compile-contract citation" invariant — a name that is not
an equation key, for an equation whose formula ranges over
`Artifact.primary` (PMAT-1437, PMAT-1438).

## 4. The idempotence invariant

`C-BASHRS-POSIX-IDEMPOTENCE` does not say "the script is idempotent"
in the general sense (that's undecidable). It says the *supported
subset* of POSIX shell — the constructs the bashrs frontend
recognizes — round-trip without drift, and that the constructs we emit
are idempotent by their nature (`mkdir -p`, redirects, conditional
file creation, etc.).

The substrate enforces this at four strata:

| Stratum | What ratifies it |
|---|---|
| Semantic | `contracts/lean/Bashrs.lean` theorems on the supported AST |
| Symbolic | `contracts/kani/bashrs.rs` BMC harnesses |
| Runtime | `bashrs_realistic_demo.sh` fixture round-tripped on every CI cycle |
| Extrinsic | The PMAT-085..119 series in the roadmap |

— all four voices agree at v0.1.0; `xpile quorum` reports QUORUM.

## 5. Where this lives

| Concept | File |
|---|---|
| Contract YAML | [`contracts/bashrs-posix-idempotence-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/bashrs-posix-idempotence-v1.yaml) |
| Lean theorems | [`contracts/lean/Bashrs.lean`](https://github.com/paiml/xpile/blob/main/contracts/lean/Bashrs.lean) |
| Kani harness | [`contracts/kani/bashrs.rs`](https://github.com/paiml/xpile/blob/main/contracts/kani/bashrs.rs) |
| Frontend | `crates/bashrs-frontend/src/` |
| Backend | `crates/bashrs-backend/src/` |
| Fixture | `tests/fixtures/bashrs_realistic_demo.sh` |
| Sub-spec | [`docs/specifications/sub/bashrs-merger.md`](https://github.com/paiml/xpile/blob/main/docs/specifications/sub/bashrs-merger.md) |

## What's next

- [Reference: CLI commands](../reference/cli.md) — the full surface.
- [Reference: backends](../reference/backends.md) — what each backend
  emits and what it refuses to emit.
