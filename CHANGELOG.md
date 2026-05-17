# Changelog

All notable changes to xpile are recorded here. The project follows
[Semantic Versioning](https://semver.org/) once it stabilizes; while in
pre-1.0 development each minor version may include breaking changes to
meta-HIR and the trait surfaces.

## [Unreleased]

### Python subset (live, runtime-verified)

This list is the **canonical source of truth** for the supported subset.
The depyler-frontend module docstring points here. When extending the
subset, update this section first.

- Top-level `def name(p: int, q: int) -> int:` with optional type
  annotations for `int` and `bool`
- Multi-statement body: zero or more `let` assignments + final `return`
- Identifiers, integer literals
- Binary arithmetic: `+ - * // %` (floor div / mod use Euclidean
  semantics, matching Python on negative operands — not Rust/Lean's
  default truncate-toward-zero). Rust + Ruchy emission uses
  `.checked_*().expect(...)` so i64 overflow panics with a message
  pointing at the unimplemented bigint promotion slow path in contract
  `C-PY-INT-ARITH` (see `contracts/py-int-arith-v1.yaml`). Lean's `Int`
  is unbounded, so the same contract is satisfied by construction.
- Bitwise: `& | ^ << >>`. `& | ^` lower to plain infix in Rust/Ruchy
  (no overflow risk per-bit). Shifts use `checked_shl` / `checked_shr`
  with `u32::try_from(rhs)` so out-of-range shift amounts panic naming
  the same contract. Lean uses `Int.land` / `Int.lor` / `Int.xor` for
  `& | ^` and `<<<` / `>>>` with `.toNat` coercion for shifts.
- Power: `**`. Rust/Ruchy emit `checked_pow(u32::try_from(rhs).expect(...))`;
  negative exponents (which Python would promote to Float) panic naming
  `C-PY-INT-ARITH`. Lean uses `^` with `.toNat` (same fidelity gap as
  shifts on negative rhs).
- Comparisons: `== != < <= > >=`
- Logical: `and or` (short-circuit, Bool)
- Unary: `-x` (checked_neg, same overflow contract), `not x`
- Ternary: `x if cond else y`
- **Statement-level `if/else`** with single- *or multi-* assignment
  branches. Each assigned name is lifted to its own
  `let name: T = if cond { ... } else { ... }` (PMAT-005). Both
  branches must assign the same *set* of names; assignments can be in
  any order within each branch.
- **`if / elif* / else` chains** recursively lowered to nested
  `IfExpr`; pretty-printed as flat `else if` in Rust / Ruchy
- Function calls: `f(args)` (including self-recursion — `factorial`,
  `fib`-style)
- **`while` loops + mutable rebinding** (PMAT-006). A name that's
  reassigned anywhere in the function (including inside a loop body)
  gets `let mut`; subsequent assignments emit `name = value;`. The
  frontend infers mutability via a pre-walk that takes the max of
  if-branch counts (alternatives) and doubles inside loop bodies
  (repetition). Lean is unsupported for `while` — a follow-up will
  encode it as `partial def` with tail recursion.
- **`for target in range(...)`** desugaring (PMAT-007 + PMAT-008).
  Supports `range(stop)`, `range(start, stop)`, and `range(start, stop, step)`
  where `step` is any non-zero integer literal (positive *or* negative).
  Lowers to a `Let` init + `While target <cmp> stop` + `target = target + step`
  tail. Loop direction is decided at lower time from the literal's
  sign: positive step uses `<`, negative step uses `>`. Non-range
  iterables and non-literal / zero steps still error with a clear message.
- **`assert cond`** (PMAT-009). No-message form only. Rust/Ruchy emit
  `assert!(cond);`. Lean is skipped (requires Decidable instances +
  a propositional formulation; deferred).
- **`BigInt` slow-path scaffold** (PMAT-012). Annotate a function with
  `BigInt` (`def big_sum(a: BigInt, b: BigInt) -> BigInt`) and the
  Rust backend emits `xpile_bigint::BigInt` with plain infix arithmetic
  (no `.checked_*().expect()` — BigInt never overflows). Lean's `Int`
  is unbounded, so the same Python source produces the same Lean
  output regardless of `int` vs `BigInt`. Ruchy defers — emits a
  clear PMAT-012 error pointing at the Rust backend. Bitwise / shift
  / power on BigInt are still a follow-up.
- **Implicit BigInt promotion via return type** (PMAT-013). Annotate
  only the *return* as `BigInt` and the frontend auto-promotes every
  `int`-typed param to BigInt: `def factorial(n: int) -> BigInt:` reads
  naturally and produces a BigInt-mode function end-to-end. Codegen
  appends `.clone()` to BigInt Ident references (BigInt isn't `Copy`)
  so a name referenced in cond + branches + recursive call compiles
  cleanly.

### Backends (real emission)

- Rust target: `pub fn name(...) -> T { ... }`
- Ruchy target: `fun name(...) -> T { ... }`
- Lean 4 target: `def name (...) : T := ...` (uses `Int.fdiv` /
  `Int.fmod` to preserve Python floor semantics). Functions with a
  `while` loop emit a companion `partial def <fn>_loop_0` helper that
  threads loop-state variables as parameters and recurses with their
  updated values (PMAT-010). For-in-range, while + mutable rebinding,
  countdown loops — all transpile cleanly to Lean.

**Contract citations** (PMAT-011): every function whose body uses an
op governed by a Layer-1 contract carries a citation in the emitted
source — `// xpile-contract: C-PY-INT-ARITH` in Rust/Ruchy,
`@[xpile_contract "C-PY-INT-ARITH"]` in Lean. The applicability is
data-driven: comparison- or logical-only functions get no citation;
arithmetic / bitwise / shift / power / unary-neg functions do. The
Lean partial-def helper for a while-loop function carries the same
citation as the outer function.

Same Python source transpiles to all three via `xpile transpile <file.py> --target <t>`.

### Quality gates (on every PR via `.github/workflows/ci.yml`)

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `pv lint contracts/`
- `cargo deny check advisories`
- `cargo test --workspace`

### Shell variable assignment — `Stmt::ShellAssign` (PMAT-051)

POSIX shell `VAR=value` is now a first-class IR construct. Real
build scripts can be transpiled end-to-end:

\`\`\`
$ cat <<'EOF' > /tmp/build.sh
LOG=/tmp/build.log
TODAY=\$(date)
NAME="Noah Gift"
echo \$LOG and \$TODAY for \$NAME
EOF

$ xpile transpile /tmp/build.sh --target shell
#!/bin/sh
# xpile-bashrs-backend (...)
# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE
# module: build
LOG=/tmp/build.log
TODAY=\$(date)
NAME="Noah Gift"
echo \$LOG and \$TODAY for \$NAME
\`\`\`

**This is the first xpile demo of a complete realistic shell
script transpiling round-trip end-to-end** — every line uses a
different Layer B construct (LitStr / CommandSubstitution /
QuotedString / ShellVar) and they all compose.

Implementation:
- **xpile-meta-hir** — new \`Stmt::ShellAssign { name: String, value: Expr }\`.
  Same cross-cutting Unsupported arm pattern as every other
  bashrs-domain variant.
- **bashrs-frontend** — parser detects \`NAME=value\` at line start
  when NAME is a POSIX-legal identifier. Uses the quoting-aware
  tokenizer (PMAT-049/050) to parse the value, so RHS can be
  \`LitStr\` / \`QuotedString\` / \`ShellVar\` / \`CommandSubstitution\`.
  Multi-token RHS (POSIX's \`VAR=val cmd args\` export-for-next-cmd
  form) explicitly rejected at v0.1.0.
- **bashrs-backend** — emits \`NAME=value\` on its own line using
  the existing \`render_arg\` helper for the value, so all four
  Expr variants render correctly in the value position.

What's NOT yet here:
- POSIX \`VAR=val cmd args\` (temporary-export) form — rejected
  explicitly. Modelling this requires the export-for-next-cmd
  semantics which is a separate Stmt variant.
- \`export VAR=value\` — semantically different (sets in the
  environment, not just the shell). Separate variant.
- \`unset VAR\` — separate variant.
- Compound assignment (\`+=\`, \`-=\` etc.) — bash-only, not POSIX.

Test coverage:
- 4 new bashrs-frontend tests:
  - \`parse_and_lower_simple_shell_assign\` — \`LOG=/tmp/foo\` →
    ShellAssign with LitStr value
  - \`parse_and_lower_shell_assign_with_command_substitution_value\` —
    \`TODAY=\$(date)\` composes with CommandSubstitution
  - \`parse_and_lower_shell_assign_with_quoted_value\` — \`NAME="Noah Gift"\`
    composes with QuotedString
  - \`parse_and_lower_rejects_var_eq_val_cmd_args_form\` — negative

### Command substitution `$(cmd)` parser (PMAT-050)

**\`Expr::CommandSubstitution\` is now produced end-to-end.** Same
pattern as PMAT-049 (quoted strings): extends the tokenizer to
recognise \`\$(cmd args)\` as an atomic token, then recursively
lowers the inner content into \`Stmt::Cmd\`.

\`\`\`
$ echo 'echo today is \$(date)' > /tmp/cs.sh
$ xpile transpile /tmp/cs.sh --target shell
#!/bin/sh
# xpile-bashrs-backend (...)
# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE
# module: cs
echo today is \$(date)

$ echo 'echo \$(date +%Y) and \$(uname -a) end' > /tmp/cs2.sh
$ xpile transpile /tmp/cs2.sh --target shell
...
echo \$(date +%Y) and \$(uname -a) end
\`\`\`

Implementation:
- **bashrs-frontend** — new \`RawToken::CommandSubst(String)\` variant
  carrying the inner content. Tokenizer recognises \`\$(\` when not
  adjacent to a bareword; reads until matching \`)\`; rejects
  nested \`\$(\$(cmd))\` (v0.1.0 supports one level only); rejects
  unterminated \`\$(\` with a precise diagnostic.
- **\`lower_raw_token\`** — now returns \`Result<Expr, FrontendError>\`
  (was \`Expr\`) since CommandSubst lowering can fail on malformed
  inner content. Recursively tokenizes the inner content and lowers
  to \`Expr::CommandSubstitution(Box<Stmt::Cmd>)\`.
- Both Cmd-construction sites updated to use the fallible variant
  via \`.collect::<Result<Vec<_>, _>>()?\`.

What's NOT yet here:
- **Nested substitution** (\`\$(\$(cmd))\`) — v0.1.0 explicitly rejects.
- **Backtick substitution** (\`\`\`cmd\`\`\`) — POSIX's older syntax;
  same semantic, but the v0.1.0 tokenizer doesn't recognise.
- **Pipelines inside \`\$(...)\`** — bashrs-backend's
  \`render_substituted_stmt\` rejects them defensively; the parser
  doesn't produce them.
- **Substitution inside double quotes** — \`"today is \$(date)"\` is
  parsed as one DoubleQuoted token with literal \`\$(date)\` content;
  variable / substitution expansion inside double quotes is v0.2.0.

Test coverage:
- 3 new bashrs-frontend tokenizer unit tests:
  - \`tokenize_line_recognises_command_substitution\` — single + multi-substitution lines
  - \`tokenize_line_rejects_unterminated_command_substitution\` — \`\$(cmd\` without \`)\`
  - \`tokenize_line_rejects_nested_command_substitution\` — \`\$(\$(date))\`
- 1 new lower-side unit test \`lower_raw_token_command_substitution_produces_expr\` — verifies the recursive Cmd construction.
- 1 new parse-side end-to-end test \`parse_and_lower_with_command_substitution\`.

### Quoting-aware tokenizer in bashrs-frontend (PMAT-049)

**`Expr::QuotedString` is now produced end-to-end.** Before this PR
the tokenizer was \`split_whitespace\`-based, so \`echo "hello world"\`
parsed as three barewords (\`echo\`, \`"hello\`, \`world"\`). Post-this-
PR it parses as two tokens: \`echo\` (bareword) + \`"hello world"\`
(\`Expr::QuotedString { quoting: Double }\`).

\`\`\`
$ echo "echo 'single quotes here' and \"double\" yo" > /tmp/q2.sh
$ xpile transpile /tmp/q2.sh --target shell
#!/bin/sh
# xpile-bashrs-backend (v0.1.0 PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B)
# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE
# module: q2
echo 'single quotes here' and "double" yo
\`\`\`

Both single-quoted and double-quoted regions survive the round-trip
with their quoting strategy intact.

Implementation:
- **bashrs-frontend** — new \`RawToken\` enum (\`Bare\` /
  \`SingleQuoted\` / \`DoubleQuoted\`) + \`tokenize_line\` state-machine
  tokenizer that recognises single and double quotes; bareword
  regions split on whitespace.
- New \`lower_raw_token\` helper dispatches \`RawToken\` to the right
  \`Expr\` variant (Bare via existing \`lower_token\`, quoted regions
  to \`Expr::QuotedString\` with the corresponding \`QuotingStrategy\`).
- Both Cmd-construction sites (top-level + Pipeline stage) switch
  from \`split_whitespace\` to the new tokenizer.

Error cases caught:
- Unterminated quotes (\`echo "hi\` / \`echo 'still hanging\`) reject
  with a precise diagnostic.
- Adjacent-to-bareword quotes (\`foo"bar"\`, \`foo'bar'\`) reject —
  string concatenation isn't supported at v0.1.0 (POSIX sh would
  treat this as one token).

Test coverage:
- 4 new bashrs-frontend tokenizer unit tests:
  - \`tokenize_line_handles_quoted_strings\` — single / double /
    mixed quoting cases
  - \`tokenize_line_rejects_unterminated_quotes\` — three negative
    cases
  - \`tokenize_line_rejects_adjacent_quotes\` — string-concat
    negative
  - \`tokenize_line_plain_words_match_split_whitespace\` —
    pre-PMAT-049 behaviour preserved on quote-free input
- 1 new parse-side unit test \`parse_and_lower_with_quoted_string_arg\`
  — end-to-end through \`parse_and_lower\`.

What's still v0.2.0 (source fold):
- Escape sequences (\`\\"\` / \`\\'\` / \`\\\\\` / \`\\$\`).
- String concatenation (\`foo"bar"\` → \`foobar\` per POSIX).
- Variable expansion inside double quotes (\`"hi \$USER"\` — content
  is preserved at v0.1.0 but not yet typed as a template).
- Inline \`#\` comments inside command lines.

### Layer B IR shape complete — `Stmt::ShellLoop` + `LoopKind` (PMAT-048)

**Last variant from the `sub/bashrs-merger.md` Layer B table lands.**
Shell control-flow loops (\`for x in …; do … done\`, \`while [ … ]\`,
\`until [ … ]\`) are now first-class IR. The meta-HIR Layer B shape
is **complete**:

| Surface | Variant | PR |
|---|---|---|
| Stmt | Cmd | PMAT-039 |
| Stmt | Pipeline | PMAT-041 |
| Stmt | **ShellLoop** | **PMAT-048 (this PR)** |
| Expr | LitStr | PMAT-042 |
| Expr | QuotedString | PMAT-042 |
| Expr | ShellVar | PMAT-045 |
| Expr | CommandSubstitution | PMAT-047 |
| Type | ShellString | PMAT-046 |
| Type | ExitCode | PMAT-046 |
| enum | QuotingStrategy | PMAT-042 |
| enum | **LoopKind** | **PMAT-048 (this PR)** |

Implementation:
- **xpile-meta-hir** — new \`Stmt::ShellLoop { kind: LoopKind, body }\`
  + new enum \`LoopKind { For { var, items }, While { cond }, Until { cond } }\`.
  \`stmt_has_int_arith\` extended (recurses into items / cond / body).
- **Codegens** — \`Stmt::ShellLoop\` arms in rust / ruchy / lean
  emit + \`stmt_has_bigint\` helpers. lean has two sites (while-body
  walker + emit_stmt). All Unsupported with the bashrs contract.
- **bashrs-backend** — new \`render_shell_loop\` helper renders the
  loop *header* (\`for var in items;\`, \`while cond;\`, \`until cond;\`)
  with a placeholder body (\`do : # body: <pending v0.2.0 expansion>; done\`).
  Multi-line body rendering needs a recursive Stmt renderer the
  v0.1.0 backend doesn't carry; future PR plugs it in.

What's NOT yet here (same posture as PMAT-046/047):
- **Parser support** — bashrs-frontend's hand-rolled parser doesn't
  recognise multi-line \`for / do / done\` syntax. v0.2.0 source
  fold's real bashrs parser produces this variant.
- **Body rendering** — placeholder \`do : # body: <pending>\` at v0.1.0;
  full recursive body rendering is XPILE-BASHRS-MERGER-***+.

Test coverage:
- 2 new bashrs-backend unit tests: \`render_shell_loop_for_kind\`
  (for-loop header) and \`render_shell_loop_while_and_until\`
  (both predicate-driven dialects).

**The Layer B IR is now structurally complete** per the spec
table. The remaining bashrs merger work shifts from "add variants"
to (a) bashrs source fold (v0.2.0), (b) producer-side parser
extensions for the new variants, (c) refinement of the C-BASHRS-
POSIX-IDEMPOTENCE contract from Bronze to Silver tier in Lean.

### Layer B variant — `Expr::CommandSubstitution(Box<Stmt>)` (PMAT-047)

Shell command substitution (\`$(cmd)\`) is now a first-class IR
variant. **Stmt nests inside Expr** — the first compositional
Layer B variant that crosses the Stmt/Expr boundary.

\`\`\`rust
// IR shape:
Stmt::Cmd {
    program: "echo".into(),
    args: vec![
        Expr::LitStr("today is".into()),
        Expr::CommandSubstitution(Box::new(Stmt::Cmd {
            program: "date".into(),
            args: vec![Expr::LitStr("+%Y".into())],
        })),
    ],
}
// renders as: echo today is $(date +%Y)
\`\`\`

Implementation:
- **xpile-meta-hir** — new \`Expr::CommandSubstitution(Box<Stmt>)\`.
  Stmt gained \`PartialEq\` derive so the recursive Expr can stay
  \`PartialEq\`-able (every Stmt field is itself \`PartialEq\`, so the
  derive is mechanical). \`expr_has_int_arith\` extended (recurses
  into the inner Stmt).
- **Codegens** — \`Expr::CommandSubstitution(_)\` arms in rust /
  ruchy / lean \`emit_expr\` returning \`Unsupported(...)\` naming the
  bashrs contract. depyler-frontend's type-inference helpers +
  lean's \`collect_idents\` get defensive arms.
- **bashrs-backend** — new \`render_substituted_stmt\` helper renders
  \`$(program args)\`. Only \`Stmt::Cmd\` is supported inside \`$(...)\`
  at v0.1.0; nested pipelines / control flow are XPILE-BASHRS-MERGER-***+.
  \`render_arg\` recurses through the new variant via the helper.

What's NOT yet here:
- **Parser support** — bashrs-frontend's hand-rolled parser doesn't
  recognise \`$(...)\` syntax yet. The variant is *IR-shape ready*;
  the v0.2.0 source fold's real bashrs parser produces it from
  real shell input. Same scaffold-only posture as PMAT-046's
  \`Type::ShellString\` / \`Type::ExitCode\`.
- Nested pipelines / control flow inside \`$(...)\` — defensive
  arm in \`render_substituted_stmt\` covers the case explicitly.

Test coverage:
- 2 new bashrs-backend unit tests: \`render_arg_command_substitution\`
  (zero-arg / one-arg / mixed-with-ShellVar) and
  \`render_arg_command_substitution_with_non_cmd_inner_errors\`
  (defensive).

### Layer B type variants — `Type::ShellString` + `Type::ExitCode` (PMAT-046)

Two pure-additive type variants the spec calls out for the bashrs
domain. Unused at the v0.1.0 surface but **load-bearing for the
Bronze→Silver refinement of `C-BASHRS-POSIX-IDEMPOTENCE`** — the
Silver-tier Lean model will type the POSIX shell state explicitly
(env vars carry \`Type::ShellString\`, exit statuses carry
\`Type::ExitCode\`) instead of the v0.1.0 Bronze model's abstract
\`Outcome\` wrapper.

Implementation:
- **xpile-meta-hir** — new \`Type::ShellString\` + \`Type::ExitCode\`
  variants. Both \`Copy\` (same as the existing \`I64\`/\`Bool\`/\`BigInt\`).
- **xpile-rust-codegen** — \`Type::ShellString | Type::ExitCode\` arm
  in \`emit_type\` returning \`Unsupported(...)\` naming the bashrs
  contract. (No Rust mapping at v0.1.0; future bashrs runtime crate
  will export the quoting-aware wrapper + \`std::process::ExitStatus\`
  alias.)
- **xpile-ruchy-codegen** — symmetric Unsupported arm.
- **xpile-lean-codegen** — Unsupported arm in code-lane \`emit_type\`.
  Silver-tier refinement of \`Bashrs.lean\` will model these
  directly in the proof lane (typed POSIX shell state), not via the
  code-lane emit.

Why ship now even though no producer uses them: same rationale as
PMAT-042 landed \`Vec<Expr>\` before any quoted-arg producer existed
— the IR shape is the load-bearing change. Future Silver-tier
refinement work plugs into the existing variants rather than
needing a refactor.

What's NOT here yet:
- A frontend that types shell variables as \`ShellString\` —
  bashrs-frontend treats all args as \`Expr::ShellVar(String)\` at
  the IR level; the *type* of those refs is implicit.
- A Lean refinement that uses these types — Silver-tier
  \`Bashrs.lean\` is XPILE-BASHRS-MERGER-***+.
- A meta-HIR function returning \`Type::ExitCode\` — the synthesised
  bashrs-frontend \`main\` returns \`Type::I64\` today; flipping it to
  \`ExitCode\` is a separate decision that affects how the audit
  pipeline classifies shell-domain functions.

### Layer B third Expr variant — `Expr::ShellVar` (PMAT-045)

Shell variable references (`$NAME` / `${NAME}`) are now a
first-class IR construct. Builds directly on PMAT-042's
\`Vec<Expr>\` foundation — a pure additive variant, no refactor.

\`\`\`
$ echo 'echo $HOME and ${USER}' > /tmp/v.sh
$ xpile transpile /tmp/v.sh --target shell
#!/bin/sh
# xpile-bashrs-backend (v0.1.0 PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B)
# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE
# module: v
echo $HOME and $USER
\`\`\`

Implementation:
1. **xpile-meta-hir** — new \`Expr::ShellVar(String)\`. The carried
   name omits the leading \`$\` and any optional braces;
   bashrs-frontend validates it's a POSIX-legal identifier before
   constructing the variant. \`expr_has_int_arith\` extended (returns
   false — different contract).
2. **Codegens** — \`Expr::ShellVar\` arms in rust / ruchy / lean
   \`emit_expr\` returning \`Unsupported(...)\` naming the bashrs
   contract. depyler-frontend's \`infer_type\` / \`infer_type_in_ctx\`
   and lean-codegen's \`collect_idents\` extended with defensive
   arms.
3. **bashrs-frontend** — new \`lower_token\` helper recognises
   \`$NAME\` and \`${NAME}\` where NAME is POSIX-legal (letters /
   digits / underscore, not starting with digit). Special params
   like \`$1\`, \`$@\`, \`$?\` fall through to \`LitStr\` (deferred to
   future Layer B PR).
4. **bashrs-backend** — \`render_arg\` extended; \`ShellVar(name)\`
   renders as bareword \`$NAME\` (canonical output form; brace form
   is input-side only).

Test coverage:
- 6 new bashrs-frontend unit tests:
  - \`lower_token_recognises_dollar_name\` — \`$HOME\` / \`$USER\` etc.
  - \`lower_token_recognises_dollar_brace_name\` — \`${HOME}\` etc.
  - \`lower_token_rejects_special_params_as_litstr\` — \`$1\`, \`$@\`, \`$?\`, \`$*\`, \`$0\`, \`$-\` fall through.
  - \`lower_token_rejects_malformed_brace_as_litstr\` — \`${HOME\`, \`${1}\`, \`${has-hyphen}\` fall through.
  - \`lower_token_plain_strings_pass_through_as_litstr\` — regression on PMAT-042.
  - \`parse_and_lower_with_shell_var_arg\` — end-to-end through the frontend.
- 1 new bashrs-backend unit test: \`render_arg_shell_var\` — verifies bareword output.
- 1 new xpile-core integration test: \`layer_b_shell_var_end_to_end\` — full bashrs-frontend → bashrs-backend pipeline.

What's NOT covered yet:
- Special parameters (\`$1\`, \`$@\`, \`$*\`, \`$?\`, \`$0\`) — needs
  \`Expr::ShellPosParam\` / \`Expr::ShellSpecial\` variants.
- Variable interpolation inside QuotedString (\`"Hello, \$USER"\`)
  — needs string-template AST.
- Command substitution (\`$(date)\`) — needs
  \`Expr::CommandSubstitution\`.
- Variable assignment (\`VAR=value\`) — needs \`Stmt::ShellAssign\`.

### Lean refinement theorem — C-BASHRS-POSIX-IDEMPOTENCE reaches QUORUM (PMAT-044)

**Second contract to reach full §14.4 N-of-M oracle quorum.** New
\`contracts/lean/Bashrs.lean\` carries the refinement theorem
\`subprocess_run_eq_shell_run\`, which proves that CPython's
\`subprocess.run([program, args...])\` and bashrs-backend's emitted
shell command produce identical observable Outcomes on string-
literal inputs. Proof is \`rfl\` by our modelling choice (Bronze
tier per ruchy 5.0 §14.10.5).

\`\`\`
$ xpile quorum
  contract                                  Sem  Sym  Run  Ext  status
  C-PY-INT-ARITH                              8    1    4    5  QUORUM
  C-BASHRS-POSIX-IDEMPOTENCE                  1    0    1    4  QUORUM   ← new
  ... (10 more)
  totals: 2 QUORUM, 0 PARTIAL, 10 UNVERIFIED (12 contracts total)
\`\`\`

Implementation:
- **\`contracts/lean/Bashrs.lean\`** — new file with the
  \`XpileContracts.CBashrsPosixIdempotence\` namespace.
  \`subprocess_run_eq_shell_run\` is the load-bearing theorem.
  \`Outcome\` is an abstract observable-equivalence wrapper —
  v0.1.0's Bronze model; Silver/Gold/Platinum tiers refine it as
  the spec's POSIX-sh semantic interpreter ships in future PRs.
- **\`contracts/bashrs-posix-idempotence-v1.yaml\`** — equation
  \`subprocess_run_equals_shell_run\` with \`lean_theorem\` +
  \`lean_file\` refs so \`refinement_proofs.rs\` validates the
  citation pipeline.
- **Quorum test** \`c_bashrs_posix_idempotence_has_runtime_witness\`
  tightened to require \`status == QUORUM\` (was
  \`PARTIAL || QUORUM\`). Locks in the v0.1.0 milestone — second
  contract at full QUORUM.

Documentary value: any future change to bashrs-backend's emit that
breaks the observable equivalence with CPython's subprocess.run
must either (a) preserve \`rfl\`-equivalence in the Lean model
(Semantic stratum keeps holding) OR (b) invalidate the theorem (the
\`refinement_proofs.rs\` citation gate fires). The two strata
(Semantic + Runtime) reinforce each other: a real-input divergence
caught by \`shell_diff_exec.rs\` would not be silenced by Lean's
\`rfl\`, and a model that drifts from the Lean theorem cannot
quietly pass the citation gate.

Tier roadmap for \`C-BASHRS-POSIX-IDEMPOTENCE\`:
- v0.1.0: **Bronze** — model commitment, theorem reduces to \`rfl\`.
- Future (Silver): typed POSIX-sh state (env vars, redirections,
  exit codes) + refinement under it.
- Future (Gold): adversarial verification by external semantic
  model.
- Future (Platinum): full shellcheck-equivalence proof.

### Shell-side diff_exec gate — C-BASHRS-POSIX-IDEMPOTENCE reaches PARTIAL (PMAT-043)

**Second contract reaches non-UNVERIFIED quorum status.** New
\`tests/shell_diff_exec.rs\` runs each fixture two ways:

1. CPython: \`exec(open(file).read()); demo()\` — the function's
   \`subprocess.run(...)\` calls fire and their stdout flows.
2. Shell: \`xpile transpile file --target shell | /bin/sh\` — the
   bashrs-backend-emitted shell executes the equivalent commands.

Both must produce **byte-identical stdout**. The test fails loudly
if depyler-frontend's subprocess.run lowering or bashrs-backend's
emit diverges from CPython observable behaviour.

\`\`\`
$ xpile quorum
  contract                                  Sem  Sym  Run  Ext  status
  C-PY-INT-ARITH                              8    1    4    5  QUORUM
  C-BASHRS-POSIX-IDEMPOTENCE                  0    0    1    3  PARTIAL   ← new
  ... (10 more)
  totals: 1 QUORUM, 1 PARTIAL, 10 UNVERIFIED (12 contracts total)
\`\`\`

Architectural significance: **pre-PMAT-043 nothing actually executed
the bashrs-emitted shell**. PMAT-040's \`subprocess.run\` cross-
domain test only verified the string output matches a pattern, not
that the emitted shell would run successfully. This PR closes that
gap — the v0.3.0 falsifier evidence (PMAT-040) is now backed by a
Runtime stratum witness, not just static-string assertion.

What ships:
- New fixture \`tests/fixtures/bashrs_diff_demo.py\` — three
  deterministic \`subprocess.run(["echo", ...])\` calls that
  produce predictable stdout (no \`pwd\` etc. that varies by cwd).
- New test file \`tests/shell_diff_exec.rs\` (replaces no existing
  file) with one test that runs the diff and one helper trio
  (have_python_and_sh / run_cpython / run_shell). Skip-gracefully
  if \`python3\` or \`/bin/sh\` is missing from PATH.
- New quorum-gate test in \`tests/quorum.rs\`:
  \`c_bashrs_posix_idempotence_has_runtime_witness\` — asserts the
  Runtime count for the contract is ≥1 and status is PARTIAL or
  QUORUM. Locks in the v0.1.0 milestone.

Quorum reporter impact: \`C-BASHRS-POSIX-IDEMPOTENCE\` jumps from
\`0/0/0/0 UNVERIFIED\` to \`0/0/1/3 PARTIAL\` — Runtime stratum
gains the new fixture witness, Extrinsic stratum reflects the
PMAT-037 through 043 roadmap mentions.

How \`C-BASHRS-POSIX-IDEMPOTENCE\` reaches QUORUM next: ship a Lean
refinement theorem about shell idempotence (Sem ≥1, contract gains
3rd stratum) or a Kani harness (Sym ≥1). Either takes it to QUORUM
on the §14.4 N-of-M rule.

### Layer B Expr-side foundation — quoting-aware string args (PMAT-042)

Refactors `Stmt::Cmd::args` from `Vec<String>` to `Vec<Expr>` and
introduces the Layer B Expr-side variants the rest of the merger
spec layers on top of:

- **`Expr::LitStr(String)`** — the unquoted / raw-token form. What
  bashrs-frontend produces for every arg at v0.1.0; what
  depyler-frontend's `subprocess.run` lowering produces.
- **`Expr::QuotedString { content, quoting: QuotingStrategy }`** —
  the typed counterpart for args that need shell-level quoting.
- **`QuotingStrategy::{Single, Double, Backslash}`** — the three
  POSIX-relevant quoting forms the spec calls out.

\`\`\`rust
// PMAT-042 in action: a hand-built Cmd with a single-quoted arg
Stmt::Cmd {
    program: "echo".into(),
    args: vec![Expr::QuotedString {
        content: "hello world".into(),
        quoting: QuotingStrategy::Single,
    }],
}
// emits:  echo 'hello world'
\`\`\`

Why now: the v0.1.0 hand-rolled bashrs-frontend doesn't produce
quoting metadata yet (every arg is `Expr::LitStr`). But landing the
`Vec<Expr>` shape now means every subsequent Layer B Expr-side
variant (`ShellVar`, `CommandSubstitution`) is an additive
pattern-match rather than a refactor of every Cmd-construction site.

Implementation (cross-cutting, ~7 sites):

1. **xpile-meta-hir** — new `Expr::LitStr` + `Expr::QuotedString` +
   `QuotingStrategy`. `Stmt::Cmd::args` changed from `Vec<String>`
   to `Vec<Expr>`. `expr_has_int_arith` extended (both new variants
   return false — they're under `C-BASHRS-POSIX-IDEMPOTENCE`, not
   `C-PY-INT-ARITH`).

2. **xpile-rust-codegen, xpile-ruchy-codegen, xpile-lean-codegen** —
   new `Expr::LitStr | Expr::QuotedString` arms in each emit_expr
   that return `Unsupported(...)` naming the bashrs contract.
   Symmetric with PMAT-039/041's Cmd/Pipeline disposition.

3. **xpile-lean-codegen** — `collect_idents` extended (defensive
   arm; never reached because Lean modules don't carry shell-string
   exprs).

4. **bashrs-frontend** — parser now produces `Vec<Expr::LitStr>`
   for args (both top-level Cmd and Pipeline stages). Behaviour
   unchanged at the surface — the change is purely IR-shape.

5. **bashrs-backend** — new `render_arg(Expr) -> Result<String>`
   helper renders each arg per its quoting strategy:
   * `LitStr` → bareword
   * `QuotedString::Single` → `'content'`
   * `QuotedString::Double` → `"content"`
   * `QuotedString::Backslash` → `\c1\c2\c3…`
   Used by both Cmd and Pipeline emit sites. Non-string Expr args
   refused with a clear error (defensive).

6. **depyler-frontend** — `subprocess.run` lowering produces
   `Vec<Expr::LitStr>` instead of `Vec<String>`. Behaviour
   unchanged for Python sources. `infer_type` / `infer_type_in_ctx`
   extended with defensive arms for the new variants (they're
   never reached on Python-frontend inputs).

7. **Tests** — bashrs-frontend / bashrs-backend / xpile-core tests
   updated to construct args as `Vec<Expr>`. New tests:
   `render_arg_uses_quoting_strategy` (3 strategies + LitStr) and
   `lower_cmd_with_quoted_string_arg_renders_with_quotes` (full
   end-to-end through bashrs-backend).

What's NOT here yet (Layer B follow-ups):

- `Expr::ShellVar(String)` — `$NAME` / `${NAME}` references.
- `Expr::CommandSubstitution(Box<Stmt>)` — `$(cmd)` inline.
- `Type::ShellString` / `Type::ExitCode` — typed shell-domain
  values for Lean refinement proofs.
- Quoting-detection in bashrs-frontend's parser (currently every
  arg is `LitStr`; the v0.2.0 source fold's real bashrs parser
  produces `QuotedString` where appropriate).

### Layer B second variant — `Stmt::Pipeline` end-to-end (PMAT-041)

Multi-stage shell pipelines (`cmd1 | cmd2 | cmd3 …`) flow through
the bashrs lane end-to-end. Same compositional shape as PMAT-039's
`Stmt::Cmd`: produced only by bashrs-frontend, consumed only by
bashrs-backend, refused by every other backend via explicit
`Unsupported` arms naming `C-BASHRS-POSIX-IDEMPOTENCE`.

\`\`\`
$ echo 'ls /tmp | wc -l' > /tmp/pipe.sh
$ xpile transpile /tmp/pipe.sh --target shell
#!/bin/sh
# xpile-bashrs-backend (v0.1.0 PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B)
# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE
# module: pipe
ls /tmp | wc -l
\`\`\`

Six small changes that compose:

1. **xpile-meta-hir** — new `Stmt::Pipeline { stages: Vec<Stmt> }`.
   Stages typed as `Stmt` for future composition with control-flow
   variants; at v0.1.0 every stage is a `Stmt::Cmd` (enforced by
   the frontend parser). `stmt_has_int_arith` recurses into stages
   for symmetry with the other compound variants.

2. **xpile-rust-codegen** — Pipeline arm in `emit_stmt_indented`
   returning `Unsupported(...)` with the stage count; companion
   arm in `stmt_has_bigint` (recurses).

3. **xpile-ruchy-codegen** — symmetric Unsupported arms.

4. **xpile-lean-codegen** — Pipeline arms in both match sites
   (while-loop body walker + `emit_stmt`).

5. **bashrs-frontend** — parser splits any line containing `|`
   into N stages, each tokenised like a Cmd; wraps as
   `Stmt::Pipeline`. Single-token lines (no `|`) continue producing
   `Stmt::Cmd` (PMAT-039 unchanged). Rejects empty stages
   (`cmd | | cmd`, `| cmd`, `cmd |`) with a clear diagnostic —
   POSIX sh rejects them too.

6. **bashrs-backend** — emit walks Cmd AND Pipeline. Each Pipeline
   renders each stage as `program args…` and joins with ` | ` on
   a single line. Non-Cmd stages are refused with an error
   pointing at the v0.1.0 stage-shape constraint (defensive arm
   for future frontends).

Test coverage:
- 4 new bashrs-frontend parser unit tests (2-stage / 3-stage /
  empty-stage rejection / single-stage stays Cmd regression).
- 2 new bashrs-backend emit tests (pipeline-renders / non-Cmd-
  stage refuses).
- 1 new xpile-core integration test
  (`layer_b_pipeline_end_to_end`).

What's NOT covered yet (each is its own additive PR):
- Quoted args (`echo "hello world"`) — needs `Expr::QuotedString`.
- Shell variables (`echo $HOME`) — needs `Expr::ShellVar`.
- Command substitution (`x=$(date)`) — needs
  `Expr::CommandSubstitution`.
- Embedded `|` inside quoted strings (`echo "a|b" | cat`) —
  v0.1.0 parser is naive; the v0.2.0 source fold's real bashrs
  parser fixes it.

### Cross-domain Python → bashrs via `subprocess.run` recognition (PMAT-040)

**The v0.3.0 falsifier evidence ships at v0.1.0.** depyler-frontend
now recognises `subprocess.run([str-literal, ...])` and lowers each
call to a `Stmt::Cmd` in meta-HIR. bashrs-backend walks any function's
Cmd statements (PMAT-039's `main`-only filter relaxed) and emits real
POSIX shell.

\`\`\`
$ cat /tmp/build_script.py
def build() -> int:
    subprocess.run(["echo", "starting"])
    subprocess.run(["ls", "/tmp"])
    subprocess.run(["pwd"])
    subprocess.run(["echo", "done"])
    return 0

$ xpile transpile /tmp/build_script.py --target shell
#!/bin/sh
# xpile-bashrs-backend (v0.1.0 PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B)
# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE
# module: build_script
# function: build
echo starting
ls /tmp
pwd
echo done
\`\`\`

Architectural significance: `sub/bashrs-merger.md`'s v0.3.0
check-back demanded that "at least one cross-domain consumer of
shell variants ships by v0.3.0 or `XPILE-UNMERGE-001` reverts the
IR merge." This PR satisfies that precondition at v0.1.0 — the IR
merge is no longer load-bearing on a future hypothesis, it has
shipped evidence. The acceptance set was:

  (a) Python `subprocess.run` recognition  ← THIS PR
  (b) Rust `Command::new` recognition       (still future)
  (c) Lean theorem about shell composition  (still future)

Implementation:

1. **depyler-frontend** — new `lower_expr_stmt_as_cmd` recogniser.
   Accepts `subprocess.run([str-lit, ...])` (positional arg = list
   literal of string literals; keyword args like `check=True`
   accepted-and-ignored). Rejects every other call shape with a
   precise diagnostic. The narrow match keeps future widening
   (e.g. `subprocess.check_call`, `os.system`) as additive
   pattern-matches rather than a refactor of a general
   expression-statement handler.

2. **bashrs-backend** — emit loop's `f.name == "main"` filter
   relaxed. Now walks every function's body for `Stmt::Cmd`. Emits
   `# function: <name>` divider before each non-`main` function's
   Cmd block so the source-to-shell mapping stays legible. The
   PMAT-039 synthesised-`main` shape continues to work (no divider
   emitted for it, since the name is structural rather than
   semantic).

3. **New fixture** `tests/fixtures/subprocess_demo.py` is the
   load-bearing demonstration. It carries an in-file doc-comment
   explaining its role as v0.3.0 falsifier evidence so future
   contributors understand why removing it triggers
   `XPILE-UNMERGE-001`.

Test coverage:
- 2 new transpile_e2e tests:
  - \`transpile_python_subprocess_run_to_shell_via_bashrs_backend\`
    — the load-bearing positive: Python → bashrs end-to-end.
  - \`transpile_python_subprocess_run_with_non_list_arg_fails_with_clear_error\`
    — negative; non-list arg yields an error mentioning both
    "subprocess.run" and "list literal".

What this PR explicitly does NOT cover (additive future work):
- `subprocess.check_call`, `subprocess.check_output`, `os.system`
  recognition.
- `subprocess.run(...)` with non-literal args (variables, format
  strings) — needs Layer B `Expr::ShellVar` / `Expr::QuotedString`.
- Capturing `subprocess.run`'s return value into a Python variable
  (needs `Expr::ExitCode` / sidecar handling for `CompletedProcess`).

### Layer B minimum viable demo — `Stmt::Cmd` end-to-end (PMAT-039)

First meta-HIR shell variant lands. `bashrs-frontend` parses a real
(if minimal) shell script and `bashrs-backend` emits real (if
minimal) POSIX shell — proving the §27 Layer B architectural premise
that the shared IR can carry shell semantics. Other backends
(rust / ruchy / lean) refuse `Stmt::Cmd` via explicit `Unsupported`
arms naming `C-BASHRS-POSIX-IDEMPOTENCE`.

Before / after (`xpile transpile demo.sh --target shell`):

\`\`\`
# Before (PMAT-037/038 scaffold)
#!/bin/sh
# xpile-bashrs-backend scaffold (...)
# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE
# module: demo
# source_lang: Shell
# TODO: lower meta-HIR shell variants to ShellCheck-clean POSIX sh
# via the bashrs runtime, landing at v0.2.0 with the source fold.

# After (this PR)
#!/bin/sh
# xpile-bashrs-backend (v0.1.0 PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B)
# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE
# module: demo
echo starting build
ls /tmp
pwd
echo done
\`\`\`

And `xpile transpile demo.sh --target rust` now fails fast with:

\`\`\`
Error: backend `rust` failed
Caused by:
    lowering error: unsupported item: Rust backend does not lower
    Stmt::Cmd (`echo` with 2 arg(s)) — contract
    C-BASHRS-POSIX-IDEMPOTENCE governs this construct; use
    `--target shell` to emit POSIX sh via bashrs-backend
\`\`\`

That refusal is the **load-bearing cross-domain dispatch boundary**
the Layer B falsifier (`sub/bashrs-merger.md` v0.3.0 check-back)
implicitly depends on: if any backend silently swallowed `Stmt::Cmd`
the bashrs domain's contract wouldn't be enforceable.

What ships (six small changes that compose):

1. **`xpile-meta-hir`**: new `Stmt::Cmd { program: String, args: Vec<String> }`.
   `Vec<String>` (not `Vec<Expr>`) for args because the hand-rolled
   parser doesn't produce variables / substitution yet — the
   expression-level shape (`Expr::ShellVar` / `Expr::QuotedString`
   / `Expr::CommandSubstitution`) ships with the v0.2.0 source fold.
   `stmt_has_int_arith` helper extended (returns false for Cmd —
   different contract domain).

2. **`xpile-rust-codegen`**: explicit `Stmt::Cmd` arm in
   `emit_stmt_indented` returning `CodegenError::Unsupported`;
   companion arm in `stmt_has_bigint`.

3. **`xpile-ruchy-codegen`**: symmetric Unsupported arm (Ruchy
   compiles to Rust, inherits the disposition).

4. **`xpile-lean-codegen`**: two arms — one in the while-loop body
   walker, one in `emit_stmt`. Both Unsupported, citing the bashrs
   contract.

5. **`bashrs-frontend`**: line-based parser. Each non-empty,
   non-comment line → one `Stmt::Cmd`. Shebang and `#`-comment
   lines stripped. The parsed command sequence is wrapped in a
   synthesised `main` function (`return_type: I64`,
   `trailing_return: LitInt(0)` — script exits 0 by default) so
   shell scripts coexist with the existing function-centric Module
   structure. If Layer B grows a richer `Item` taxonomy
   (`Item::ShellScript`), the wrapper goes away.

6. **`bashrs-backend`**: walks `module.items[].body.stmts`, emits
   one shell-line per `Stmt::Cmd`. Header / shebang / citation
   shape unchanged from PMAT-037 scaffold. Empty input still
   produces a well-formed POSIX file with the
   `# (no commands ...)` diagnostic comment.

Test coverage:
- 3 new `bashrs-frontend` parser unit tests (empty input, real
  three-command script, comments-only input).
- 1 new `bashrs-backend` test for synthesised-main emission;
  1 updated test for empty-module emission.
- 2 new `xpile-core` integration tests:
  `layer_b_end_to_end_bashrs_frontend_to_bashrs_backend` — full
  pipeline produces real shell; `layer_b_rust_backend_refuses_shell_module_with_cmd`
  — locks in the cross-domain refusal with the contract citation
  in the error message.

What's deliberately NOT yet here (each is its own future PR):
- Pipelines (`cmd1 | cmd2`) → `Stmt::Pipeline { stages: Vec<Stmt::Cmd> }`
- Variables / quoting / substitution → Layer B Expr-side variants
- Real ShellCheck-clean output → v0.2.0 source fold with the
  bashrs corpus + verifier
- Inline `# comment` token handling inside command lines

### Frontend::matches_path trait method (PMAT-038)

Extends the `Frontend` trait with a `matches_path(path) -> bool`
method, defaulting to extension-based matching so all existing
frontends (python / c / ruchy) behave unchanged. `BashrsFrontend`
overrides it to additionally claim the extensionless canonical
filenames `Makefile` and `Dockerfile` — closing the second item
on the `sub/bashrs-merger.md` Layer A backlog.

End-to-end behaviour change:

\`\`\`
$ echo "all:" > /tmp/Makefile && echo -e "\techo hi" >> /tmp/Makefile
$ xpile transpile /tmp/Makefile --target shell
#!/bin/sh
# xpile-bashrs-backend scaffold (...)
# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE
# module: Makefile
# source_lang: Shell
...

$ xpile transpile /tmp/Dockerfile --target shell
# ... same shape, module: Dockerfile
\`\`\`

Pre-PMAT-038 both invocations errored with "no frontend handles
extension `.`" because the dispatch logic was a raw
`extensions().contains()` check.

Dispatch sites switched to `matches_path`:
  - `xpile transpile` (main.rs `transpile` fn)
  - `xpile audit` per-file lookup (main.rs `audit` fn)

The audit walker (`collect_source_files` / `walk_dir`) stays
extension-only at v0.1.0; expanding it to walk canonical-filename
artifacts can land when the audit pipeline grows shell-target
support (XPILE-FALSIFY-003+).

Test coverage:
  - 3 new bashrs-frontend unit tests:
    `matches_path_accepts_dotted_extensions`,
    `matches_path_accepts_extensionless_makefile_and_dockerfile`,
    `matches_path_rejects_unrelated_files` (negative — must NOT
    grab `.py` / `.c` / `Makefile.in` / `Dockerfile.dev`).
  - 2 new xpile-core integration tests:
    `matches_path_dispatch_is_unique_per_file` (asserts exactly
    one frontend claims each known path),
    `matches_path_default_impl_is_extension_only_for_non_overriding_frontends`
    (catches regressions that widen the trait default).

### bashrs merger Layer A scaffold (PMAT-037 / XPILE-BASHRS-MERGER-001)

First concrete step on the `sub/bashrs-merger.md` Layer A path:
the shell domain is now a first-class registered transpile target.
v0.1.0 scaffold-stage: no actual shell parsing or ShellIR yet — the
real source folding from `paiml/bashrs` lands at v0.2.0 (the
"weeks 1-6 extract" phase). What this PR delivers:

- **Two new workspace crates**:
  - `crates/bashrs-frontend/` — implements `Frontend`, recognises
    `.sh` / `.bash` / `.zsh` / `.mk` extensions, `parse_and_lower`
    returns a structurally empty `Module` tagged
    `SourceLang::Shell`. Special-file matching (`Makefile`,
    `Dockerfile`) is deferred to v0.2.0 with a richer matcher.
  - `crates/bashrs-backend/` — implements `Backend`, targets
    `Target::Shell`. `lower` emits a placeholder POSIX-shell
    comment carrying the `C-BASHRS-POSIX-IDEMPOTENCE` citation, so
    the citation pipeline is exercised end-to-end on day one.

- **Two new enum variants** (the load-bearing IR change):
  - `xpile_meta_hir::SourceLang::Shell`
  - `xpile_backend::Target::Shell`
  No `Stmt::Cmd` / `Stmt::Pipeline` / `ShellVar` etc. yet — those
  ship with the v0.2.0 source folding per `bashrs-merger.md` Layer B.

- **Dispatch wiring**: `xpile-core::default_session` now registers
  bashrs-frontend + bashrs-backend. `xpile info` lists them as
  the 4th frontend + 6th backend.

- **CLI**: `xpile transpile foo.sh --target shell` works end-to-end
  (returns the scaffold POSIX comment). `parse_target` accepts
  `shell`, `sh`, `bash` as aliases.

- **Contract**: new `contracts/bashrs-posix-idempotence-v1.yaml`
  (`C-BASHRS-POSIX-IDEMPOTENCE`, kind: pattern). Pattern scope
  rather than kernel while the equations / falsification_tests /
  kani_harnesses sections are unpopulated — same posture as
  `compile-rust-to-ptx-mma-v1.yaml`'s scaffold.

- **Quorum reporter impact**: `xpile quorum` now walks 12 contracts
  (was 11). C-BASHRS-POSIX-IDEMPOTENCE shows as UNVERIFIED, which
  is the accurate scaffold-stage state. Promoting it to PARTIAL
  or QUORUM is v0.2.0 work and beyond.

- **Tests**: 5 new unit tests (3 on bashrs-frontend, 2 on
  bashrs-backend). 2 new integration tests in `xpile-core` assert
  the dispatch table includes bashrs's shell extensions and that
  the backend emits the contract citation. Total workspace tests
  pass: 0 failures across the workspace, including all existing
  diff_exec / quorum / attestations gates.

Architectural significance: this PR makes the bashrs merger no
longer purely aspirational — every dispatch surface, contract
substrate, audit pipeline, and quorum reporter now recognises the
shell domain. The remaining v0.2.0 work (real ShellIR emit,
17,882-pattern corpus integration, `paiml/bashrs` repo becoming a
re-export shim) plugs into already-wired infrastructure rather
than adding new lanes. Falsifier: the existing v0.3.0 check-back
in `sub/bashrs-merger.md` ("at least one cross-domain consumer of
shell variants must ship by v0.3.0 or `XPILE-UNMERGE-001` reverts
the IR merge") is unchanged.

### BigInt auto-promotion closes DIFF-003 documented gaps (PMAT-036)

Converts the 20 documented promotion gaps in the differential-exec
gate from panics into successful BigInt-equivalent outputs. Headline:

\`\`\`
XPILE-DIFF-001/002: 100 fast-path differential checks across 10 fixtures — all green.
XPILE-DIFF-003: 20 overflow-phase checks across 2 fixture(s) — 0 documented promotion gaps, 20 promoted-and-agreed.
\`\`\`

Mechanism (no new codegen — just exercising existing PMAT-013 / -025
infrastructure on the overflow-prone fixtures):

1. **`factorial.py` and `countdown.py` annotated `-> BigInt`.** PMAT-013's
   implicit promotion lifts `n: int` → BigInt and every int literal
   in the body → `xpile_bigint::BigInt::from(...)`, so the whole
   function runs in BigInt mode end-to-end. Recursive multiplication
   for n=21..30 now never overflows.

2. **`depyler-frontend` extends BigInt propagation to for-range loop
   targets.** Before this PR, `for i in range(n, 0, -1)` lowered to
   `let mut i: i64 = n` even when `n` was BigInt — a type error
   under PMAT-013. Now the for-target's binding type follows
   `ctx.fn_return_type`: BigInt-mode functions get BigInt loop
   variables, so countdown.py compiles cleanly.

3. **`depyler-frontend` accepts `from __future__ import annotations`
   as a no-op preamble.** Required for CPython to `exec` the fixture
   without `NameError: BigInt` (xpile's metadata-only type alias for
   Python's unbounded int).

4. **`diff_exec.rs` dual-mode build pipeline.** When the transpile
   output uses `xpile_bigint::BigInt`, the runner materialises a
   one-shot Cargo project that depends on the in-workspace
   `xpile-bigint` crate (path dep) so the produced binary has the
   real `num_bigint::BigInt` + `Display` available. Non-BigInt
   fixtures keep the existing standalone-rustc fast path.

5. **`--target-dir` pinning** so the binary lands at a predictable
   path regardless of any global `CARGO_TARGET_DIR` env or
   workspace `.cargo/config.toml` setting (the local dev env sets
   `target-dir` globally; CI doesn't).

E2E test updates: 3 transpile_e2e tests that hard-asserted i64
emission for factorial/countdown were updated to assert BigInt
emission. Drivers now use inline `mod xpile_bigint { ... }` shims
matching the existing PMAT-013 BigInt fixture tests.

Architectural payoff: this PR proves the §27 type lattice handles
dynamic size escalation through a complete fixture lifecycle —
frontend lowering, codegen, and the differential-exec gate all
participate in the BigInt-mode path. The 20-gaps-to-20-successes
flip in the gate output is the user-visible metric.

### Additive slow-path soundness theorem (PMAT-034 / XPILE-REFINE-006)

Closes the last fast/slow-path refinement gap for `C-PY-INT-ARITH`'s
additive operation. New theorem `add_slow_path_eq_python`:

\`\`\`lean
theorem add_slow_path_eq_python
    (a b : Int)
    (_h : ¬ fits_i64 (a + b)) :
    bigint_add a b = a + b := by
  rfl
\`\`\`

The proof is `rfl` by our modelling choice (`bigint_add a b := a + b`).
The artifact's value is *documentary*: the equation
`addition_overflow_promotion` in `py-int-arith-v1.yaml` now carries a
`lean_theorem:` ref, so `refinement_proofs.rs` validates the citation
on every test run. Any future change to `bigint_add`'s definition
would have to either retain `rfl`-equality with `+` or invalidate
this theorem (and fail the gate).

The `¬ fits_i64 (a + b)` hypothesis is the *operational* trigger
(when the i64 fast path would panic and emission switches to BigInt
mode), not a mathematical precondition. The slow-path equality holds
for all `a, b`; keeping the hypothesis in the signature documents
which YAML equation this theorem refines.

Quorum impact: `xpile quorum` now reports C-PY-INT-ARITH at Sem=8
(up from 7), Sym=1, Run=3, Ext=5 — still QUORUM status, but with
more Semantic-stratum coverage.

Bitwise (XPILE-REFINE-005) remains the only refinement gap on
C-PY-INT-ARITH: core Lean lacks `Int.land/lor/xor`. Needs mathlib
dep or hand-rolled cast-through-Nat — design decision deferred.

### Unified §14.4 quorum reporter (PMAT-033)

New `xpile quorum` subcommand consolidates the four §14.4 strata into
a single CLI table. It's a *reporter*, not a gate — the constituent
CI gates (`refinement_proofs.rs`, `kani_verify.rs`, `diff_exec.rs`,
`attestations.rs`) remain authoritative; this command visualises what
they've collectively established.

\`\`\`
xpile quorum [--contracts-dir <p>] [--fixtures-dir <p>] [--roadmap <p>] [--json]
\`\`\`

Per-contract tally:
| Stratum | Vote source |
|---|---|
| Semantic | `lean_theorem:` refs in the contract's own YAML |
| Symbolic | `kani_harness:` refs in the contract's own YAML |
| Runtime | fixture files under `tests/fixtures/` mentioning the contract ID |
| Extrinsic | roadmap work-item mentions (reuses PMAT-032's scanner) |

Quorum status per ruchy 5.0 §14.4: `QUORUM` (≥1 vote in ≥3 strata),
`PARTIAL` (1-2 strata), `UNVERIFIED` (0 strata).

v0.1.0 live state:

\`\`\`
  contract                                  Sem  Sym  Run  Ext  status
  C-PY-INT-ARITH                              7    1    3    5  QUORUM
  C-COMPILE-RUST-TO-PTX-MMA                   0    0    0    0  UNVERIFIED
  ... (9 more, all UNVERIFIED)

totals: 1 QUORUM, 0 PARTIAL, 10 UNVERIFIED (11 contracts total)
\`\`\`

The QUORUM count == 1 number is the headline: at v0.1.0, exactly one
contract has full four-stratum coverage. The 10 UNVERIFIED contracts
are the actionable backlog.

Test coverage:
- 2 unit tests on the threshold logic + field counter
- 2 integration tests: `C-PY-INT-ARITH` has full quorum in live state;
  reporter walks every contracts/*.yaml file (no silent misses).

### Extrinsic-stratum attestations via pmat work items (PMAT-032 / XPILE-QUORUM-005)

Closes the Extrinsic-stratum side of the ruchy 5.0 §14.4 N-of-M
oracle quorum. The three formal strata (Semantic / Symbolic /
Runtime) are CI-gated since QUORUM-001-003 + DIFF-001-003; the
Extrinsic stratum (human review) is now sourced from `roadmap.yaml`
work-item references to contract IDs.

New CLI subcommand:

\`\`\`
xpile attestations [--roadmap <path>] [--contracts-dir <path>] [--json]
\`\`\`

Walks `contracts/*.yaml` for the contract ID universe (lightweight
`metadata.id:` scan), then scans the roadmap log for occurrences of
each ID. Each occurrence is one human attestation; attestations are
attributed to the enclosing work item's `id:` (e.g. `PMAT-029`).

v0.1.0 live state:
- 11 contracts scanned.
- **`C-PY-INT-ARITH`**: 5 attestations across 5 work items
  (PMAT-002 / 011 / 017 / 019 / 030).
- 10 unattested contracts (defined under contracts/ but never
  referenced in any work-item): surfaced as a "zombie contract"
  candidate list so a future audit can decide which to retire vs.
  promote to first-class.

Integration tests assert C-PY-INT-ARITH has ≥1 attestation in the
live roadmap and that the text-mode output carries its landmarks
(QUORUM ticket, stratum identifier). Unit tests cover the YAML
`metadata.id` parser and the per-work-item attribution logic. JSON
output is a single-line, hand-rolled payload (same posture as
`xpile audit --json`) so CI dashboards can ingest it without
serde_yaml/serde_json pulled into the xpile bin.

### Overflow-prone ranges + panic-as-BigInt interpretation (PMAT-031 / XPILE-DIFF-003)

Extends `diff_exec.rs` from "only test fast-path inputs" to also
exercise inputs that *must* overflow i64. New `overflow_args` field
on `FixtureCfg` declares a per-fixture overflow domain. The runner:

1. Runs CPython on the overflow inputs — always succeeds (Python
   promotes to BigInt).
2. Runs the transpiled Rust binary — expected to panic.
3. Classifies the outcome:
   - **`DocumentedGap`**: Rust panicked AND the panic message cites
     `C-PY-INT-ARITH`. This is the *expected* behaviour per Layer-1
     `C-PY-INT-ARITH` slow-path-not-yet-implemented. Counted under
     `promotion_gaps`. NOT a test failure.
   - **`Promoted`**: Rust exited zero with a value. Either the
     function is in BigInt mode (a pleasant surprise — full
     promotion is the long-term goal), or this specific input
     didn't actually overflow. We compare against Python; agreement
     counts under `overflow_promoted_ok`, divergence is a silent
     miscompile and hard-fails.
   - **`OffContractCrash`**: Rust panicked but the message did NOT
     cite `C-PY-INT-ARITH`. Either codegen regressed (lost the
     citation) or it's an unrelated crash. Hard-fails.

Two fixtures now have overflow demos: `factorial.py` (n ≥ 21
overflows recursively) and `countdown.py::factorial_iter` (same
domain, iterative shape). At v0.1.0, all 20 overflow-phase
checks land in `DocumentedGap` — the citation trail is intact, the
gap is named, the test surfaces a number ("20 documented promotion
gaps") that will drop to zero once XPILE-REFINE-006 ships BigInt
mode for these signatures.

Why the third outcome bucket is load-bearing: it catches the
regression where someone removes `C-PY-INT-ARITH` from the panic
literal in `emit_checked` / `emit_checked_pow` / `emit_checked_shift`.
Pre-003 such a regression was invisible to the differential gate.

### Complete C-PY-INT-ARITH refinement corpus: shift + power theorems (PMAT-030 / XPILE-REFINE-004)

Three more theorems join the four already discharged for `+`, `*`,
`//`, `%`. The full in-domain arithmetic + shift + power surface of
`C-PY-INT-ARITH` is now machine-checked by Lean 4.15.

| Theorem | Discharge technique |
|---|---|
| `shl_fast_path_eq_slow_path` (`<<`) | `bmod_fits_i64` lemma (modelled as `a * 2^b`) |
| `shr_fast_path_eq_slow_path` (`>>`) | `rfl` (both paths are `Int.fdiv a (2^b)`) |
| `pow_fast_path_eq_slow_path` (`**`) | `bmod_fits_i64` lemma |

Why model shifts as multiplication / division rather than `<<<` /
`>>>`: core Lean 4.15 doesn't auto-synthesise the
`HShiftLeft Int Nat` instance, and `a * 2^b` is semantically
identical to `a <<< b` for non-negative shift amounts (which is the
only case Rust's `checked_shl(b: u32)` accepts). Using arithmetic
operators avoids a mathlib import.

Contract YAML now has three new equations:
`shift_left_signed_semantics`, `shift_right_signed_semantics`,
`power_signed_semantics`, each with `lean_theorem` + `lean_file`
refs so `refinement_proofs.rs` validates the citation pipeline.

`bitwise_and_signed_semantics` still has no `lean_theorem`: core
Lean lacks `Int.land` / `Int.lor` / `Int.xor`. Tracked as
XPILE-REFINE-005 (mathlib dep, or hand-rolled encoding via
cast-through-Nat). The slow-path / promotion proofs (CPython ==
BigInt::add when `¬fits_i64`) are XPILE-REFINE-006.

### Discharge mul/floor_div/mod stub theorems (PMAT-029 / XPILE-REFINE-003)

Closes the *last* `XPILE-PENDING-UNTIL` marker anywhere in the
workspace. All four `C-PY-INT-ARITH` refinement theorems are now
machine-checked by Lean 4.15.

Implementation:

- Factored out a shared lemma `bmod_fits_i64 : Int.bmod n (2^64) = n
  when fits_i64 n` (the proof technique PMAT-028 introduced for `+`).
  The lemma's proof is `rw [Int.bmod_def] + split <;> omega`.
- `mul_fast_path_eq_slow_path` (`*`) now reuses `bmod_fits_i64` via
  `i64_wrap_mul a b := Int.bmod (a * b) (2 ^ 64)`. Proof reduces to
  `exact bmod_fits_i64 (a * b) h`.
- `floor_div_fast_path_eq_slow_path` (`//`): both fast and slow path
  model floor-div as `Int.fdiv`, so the theorem reduces to `rfl`.
  The `fits_i64`-of-result + `b ≠ 0` hypotheses stay in the statement
  to document the runtime preconditions xpile-rust-codegen guarantees
  via `.checked_div(...).expect(...)`.
- `mod_fast_path_eq_slow_path` (`%`): same shape as floor-div, via
  `Int.fmod`.

Contract YAML now carries `lean_theorem` + `lean_file` refs on three
more equations (`multiplication_quadratic_promotion`,
`division_floor_semantics`, new `modulo_floor_semantics`), so the
existing `refinement_proofs.rs` gate validates them on every test
run. The landmark test was updated to assert all four theorems by
name + the positive landmark `Int.bmod_def`, with negative landmarks
for `sorry` and `by trivial` so a regression to either fires loudly.

Side effect: with zero live `XPILE-PENDING-UNTIL` markers anywhere
in the workspace, the prior live-state sanity tests
`at_least_one_marker_exists` + `scanner_picks_up_proof_lane_markers`
became contradictory (they required a marker to exist). Replaced
both with a synthetic-fixture test
`scanner_reaches_all_watched_directories` that builds a temp
workspace-shaped tree, drops a marker into each watched location,
and asserts the scanner finds them all. The new test is strictly
stronger than what it replaces — it catches a future refactor that
silently narrows the scan.

### Discharge `sorry` in `fast_path_eq_slow_path` Lean proof (PMAT-028 / XPILE-REFINE-002)

Closes the second of the two `XPILE-PENDING-UNTIL: v0.3.0` markers
on the primary refinement theorem. The load-bearing claim of
`C-PY-INT-ARITH` — that the i64 fast path agrees with the BigInt
slow path everywhere the sum fits in `i64` — is now machine-checked
by Lean 4.15 without any mathlib dep.

Implementation: refactored `i64_wrap_add` from the previous
hand-rolled `(a + b) % 2^64`-fold form to Lean core's `Int.bmod`
(*balanced mod*, returns values in `[-N/2, N/2)`). For `N = 2^64`
that's exactly the i64 signed range, so the proof becomes:

```lean
unfold i64_wrap_add bigint_add fits_i64 at *
obtain ⟨hlo, hhi⟩ := h
rw [Int.bmod_def]
split <;> omega
```

The `Int.bmod_def` rewrite exposes the conditional `(a+b) % 2^64`
case-split, and `omega` closes both branches from the `fits_i64`
hypothesis. Verified locally with `lean 4.15.0`.

Gate update: `crates/xpile/tests/refinement_proofs.rs` now asserts
the *positive* landmark `Int.bmod_def` is present and the negative
landmark `sorry` is absent from proof code (docstrings excluded).
So a future regression that reintroduces `sorry` fires loudly.

The stub trio (`mul_fast_path_eq_slow_path`,
`floor_div_fast_path_eq_slow_path`, `mod_fast_path_eq_slow_path`)
still carries `by trivial` placeholders under
`XPILE-PENDING-UNTIL: v0.3.0, ticket: XPILE-REFINE-003`. Those
need different proof shapes (`Int.bmod_mul_emod_self_left` and
friends) and will land separately.

### Lean `assert` via recursive if-then-panic encoding (PMAT-027 / PMAT-009-FOLLOWUP)

Closes one of the two `XPILE-PENDING-UNTIL: v0.3.0` markers. The
Lean codegen now lowers `Stmt::Assert` to a nested
`if cond then <rest> else panic!` chain that preserves Python's
evaluation order (innermost assert runs first because it's
deepest in the AST). Required refactoring `emit_block` into a
recursive `emit_stmts_then_trailing` that wraps each assert
around everything after it.

Sample (`safe_div` from `asserted.py`):

```
@[xpile_contract "C-PY-INT-ARITH"]
def safe_div (a : Int) (b : Int) : Int :=
  if ((b != (0: Int))) then
  if ((a >= (0: Int))) then
  (Int.fdiv a b)
  else panic! "xpile: assertion failed (contract C-PY-INT-ARITH)"
  else panic! "xpile: assertion failed (contract C-PY-INT-ARITH)"
```

Side effect: `xpile audit --target lean` jumps from F1=100% with
1 error (asserted.py) to F1=100% with 0 errors. The full Lean
corpus now compiles. Only one v0.3.0 marker remains (Lean
refinement-proof `sorry` discharge).

### BigInt bitwise / shift / power in Rust + Ruchy backends (PMAT-026 / PMAT-013-FOLLOWUP)

Closes the second of three `XPILE-PENDING-UNTIL: v0.2.0` markers.
Both Rust and Ruchy backends now handle `& | ^ << >> **` on
BigInt operands.

Implementation:
- `xpile-bigint` grows three helper functions: `shl(&BigInt, &BigInt)`,
  `shr(&BigInt, &BigInt)`, `pow(&BigInt, &BigInt)` — each converts
  the rhs from BigInt to the primitive type `num-bigint` wants
  (`usize` for shifts, `u32` for pow) with a contract-named panic
  on out-of-range / negative inputs.
- Rust + Ruchy codegens replace the `Unsupported` deferral with:
  * `& | ^` → plain infix (num-bigint impls these directly on
    BigInt operands)
  * `<< >> **` → calls to `xpile_bigint::{shl, shr, pow}`

After this PR, exactly **two `XPILE-PENDING-UNTIL: v0.2.0` markers
of three are closed** (Ruchy BigInt mode + Rust/Ruchy BigInt
bitwise/shift/power). The Lean v0.3.0 markers (assert + refinement
proofs) remain.

New fixture `bigint_bits.py` exercises the full BigInt-mode
bitwise+shift surface end-to-end.

### Ruchy BigInt mode (PMAT-025 / PMAT-012-FOLLOWUP)

Closes one of the three live `XPILE-PENDING-UNTIL: v0.2.0` markers
from PMAT-014. The Ruchy backend now supports BigInt-typed
functions end-to-end, mirroring the Rust backend's PMAT-012/013
emission. `xpile transpile foo.py --target ruchy` on a fixture
with `BigInt` annotations now produces clean Ruchy source with
`xpile_bigint::BigInt` typed signatures, `.clone()` on Ident
references, plain infix arithmetic, and the contract citation.

Sample:
```
$ xpile transpile crates/xpile/tests/fixtures/big_sum.py --target ruchy
// xpile-contract: C-PY-INT-ARITH
fun big_sum(a: xpile_bigint::BigInt, b: xpile_bigint::BigInt) -> xpile_bigint::BigInt {
    (a.clone() + b.clone())
}
```

Implementation: mechanical mirror of the Rust pattern — added
`function_bigint_mode(f)` + threaded `mode: bool` through every
`emit_*` function. Reused the same `xpile_bigint::div_floor` /
`mod_floor` helpers and the same bitwise/shift/power deferral
(now under a `[XPILE-PENDING-UNTIL: v0.2.0, ticket: PMAT-013-FOLLOWUP]`
marker shared with Rust).

Removed the previous `bigint_ruchy_errors_with_pmat_012_message`
test (bait test that asserted the bail path); replaced with two
positive tests asserting the Ruchy emission shape for explicit
and implicit BigInt promotion.

### Multi-arg fixtures in differential exec gate (PMAT-024 / XPILE-DIFF-002)

`crates/xpile/tests/diff_exec.rs` generalised from 1-arg-only to
support 2-arg fixtures via per-arg input ranges. Three new 2-arg
fixtures: `gcd`, `range_size`, `bits`. **Total: 100 differential
checks across 10 fixtures per CI run** (up from 70 across 7),
all green. Driver synthesis builds the right
`entry(argv[0], argv[1], ...)` call expression at the configured
arity. Still pending: overflow-prone ranges + panic-as-BigInt
interpretation (XPILE-DIFF-003).

### Refine F1 to applicable-contracts denominator + Lean target (PMAT-023 / XPILE-FALSIFY-002)

`xpile audit`'s F1 metric is now computed against only the
functions where `Function::applicable_contracts()` is non-empty —
the *applicable-contracts denominator*. Pre-002 the denominator was
every emitted function, which double-penalised comparison-only
and logical-only functions that correctly emit no citation by
design. With the refinement, F1 on the current corpus jumps from
83.3% [WARN] to 100.0% [OK].

Also added `--target lean`: the audit now recognises Lean's
`@[xpile_contract "..."]` attribute alongside Rust/Ruchy's
`// xpile-contract:` comment form.

New `over_citations` JSON field is a sanity check for the
symmetric failure mode (codegen wrongly cites a comparison-only
function); currently 0.

### Extend deadline scan to proof-lane + Kani harnesses (PMAT-022 / XPILE-EXEMPT-002)

Widens `crates/xpile/tests/exempt_deadlines.rs` from "Rust source
under `crates/*/src/`" to also cover `contracts/lean/*.lean` and
`contracts/kani/*.rs`. The `XPILE-PENDING-UNTIL: v0.3.0` marker
inside `PyIntArith.lean`'s `sorry` proof was effectively
decorative before; now it's gated alongside the codegen markers.
New `scanner_picks_up_proof_lane_markers` test asserts the
widening worked.

### Kani job in CI (PMAT-021 / XPILE-QUORUM-003)

New dedicated `kani` job in `.github/workflows/ci.yml` installs
`kani-verifier`, runs `cargo kani-setup`, and runs the
`kani_verify` workspace test against every harness on every PR.
Kept as a separate job (not bundled with `workspace-test`) so the
~5-minute cold-cache Kani install doesn't slow fast-feedback
gates. Not a required status check yet — flip after Kani has
bedded in for a release cycle. Symbolic stratum is now load-bearing
on every PR, not just locally.

### Run Kani harnesses in workspace tests (PMAT-020 / XPILE-QUORUM-002)

Converts the Symbolic stratum from claim to fact. New
`crates/xpile/tests/kani_verify.rs` walks every `contracts/kani/*.rs`
file, materialises a temp Cargo crate per harness, runs `cargo kani`,
asserts exit-0 AND stdout contains `VERIFICATION:- SUCCESSFUL`
(grep guards against Kani's historical "exit 0 on swallowed solver
error" failure mode). Skip-gracefully if `cargo-kani` is missing
from PATH; local users with Kani installed get the gate
automatically. Still remaining: install Kani in CI so the gate
fires on every PR (XPILE-QUORUM-003).

### Symbolic stratum: Kani harness for C-PY-INT-ARITH (PMAT-019 / XPILE-QUORUM-001)

First **Symbolic stratum** of the N-of-M oracle quorum lands.
`contracts/kani/py_int_arith.rs` carries `#[kani::proof]` functions
for `addition_no_overflow` (and a stub `subtraction_no_overflow`);
Kani 0.67 discharges both via bit-blasted i64 arithmetic in ~27ms.
`contracts/py-int-arith-v1.yaml` grows `kani_harness:` + `kani_file:`
fields wiring the citation; the new
`crates/xpile/tests/kani_harnesses.rs` validates every cited harness
exists in its file with a real `#[kani::proof] fn <name>(...)`.

Combined with PMAT-017's Lean theorem (Semantic stratum) and
PMAT-018's diff_exec runtime check (Semantic stratum), the
`addition_no_overflow` equation now has ≥1 Symbolic + ≥1 Semantic
vote per ruchy 5.0 §14.4 quorum rule.

What this does NOT include yet (XPILE-QUORUM-002+): running
`cargo kani` in CI on every PR; the §14.5 F3 pairwise-correlation
guard; Extrinsic (human review) verdict-recording.

### Differential execution check (PMAT-018 / XPILE-DIFF-001)

New `crates/xpile/tests/diff_exec.rs` runs deterministic LCG-seeded
i64 inputs through both CPython (on the original .py source) and
the rustc-compiled transpiled-Rust binary, asserts their stdout
strings agree. 10 inputs × 7 single-arg fast-path fixtures = 70
differential checks per CI run. Skip-gracefully if `python3` or
`rustc` is missing from PATH. Each fixture's input range is
hardcoded to stay inside the C-PY-INT-ARITH fast-path domain;
widening to overflow-prone ranges + multi-arg fixtures is
XPILE-DIFF-002. Generalises the 11 hand-authored runtime-verified
fixtures into a quantitative gate against fixture overfitting
(audit-design.md §4 caveat).

### Lean refinement proof for C-PY-INT-ARITH (PMAT-017 / XPILE-REFINE-001)

First contract YAML grows `lean_theorem:` + `lean_file:` fields on
its equations. `contracts/py-int-arith-v1.yaml` points at
`contracts/lean/PyIntArith.lean`'s `fast_path_eq_slow_path`
theorem, which states `i64_wrap_add a b = bigint_add a b` when
`fits_i64 (a + b)`. Proof is currently `sorry`-discharged
(XPILE-REFINE-002 follows-up); the *statement* is what the citation
pipeline points at via `@[xpile_contract "C-PY-INT-ARITH"]`.

Enforcement test (`crates/xpile/tests/refinement_proofs.rs`) walks
every contract YAML, asserts every `lean_theorem:` field references
a real file with a real theorem of that name. Closes the
citation-bridge-fragility audit caveat for this contract.

### Quarterly SOTA-gap dossier cadence (PMAT-016 / XPILE-SOTA-001)

`audit-design.md` §0 publishes the quarterly cadence + the next
dossier deadline. Enforcement test (`crates/xpile/tests/sota_dossier_deadline.rs`)
parses the deadline string, compares against wall-clock time, fails
CI when current ≥ deadline. Missing dossier ⇒ falsifier F6 fires
automatically, no manual policing.

Cadence as of v0.1.0: 2026-Q2 (initial — §1..§6 of audit-design.md);
2026-Q3 deadline 2026-08-15; 2026-Q4 deadline 2026-11-15;
2027-Q1 deadline 2027-02-15.

### `xpile audit` (PMAT-015 / XPILE-FALSIFY-001)

New CLI subcommand reports F1 (Layer-1 contract citation coverage)
on a corpus. Walks the given path, runs the transpile pipeline on
every source file the dispatch table recognises, parses the emitted
output for `// xpile-contract: <ID>` citations adjacent to function
declarations, reports % coverage with the §27 roadmap's
OK/WARN/FAIL thresholds (≥95% / ≥50% / <50%). Text + `--json`
modes. Current baseline against `crates/xpile/tests/fixtures/`:
F1 ≈ 83% (WARN — gap is by design; comparison-only functions
correctly don't carry the citation). Lean target is XPILE-FALSIFY-002.

### Time-bounded escape hatches (PMAT-014 / XPILE-EXEMPT-001)

Every "not yet implemented" panic / `Unsupported(...)` error in the
codegen carries an explicit `[XPILE-PENDING-UNTIL: v<semver>, ticket: <ID>]`
marker. A workspace test (`crates/xpile/tests/exempt_deadlines.rs`)
scans every `.rs` file under `crates/*/src/` for the marker and
asserts the current workspace version is strictly less than every
deadline. CI fails the moment a deadline is reached without the
underlying feature shipping — closes the "unimplemented forever"
hole. Adapted from ruchy 5.0 §14.7 (`#[contract_exempt(until)]`).
Current live markers:

- `Ruchy BigInt mode` — until v0.2.0, ticket PMAT-012-FOLLOWUP
- `Rust BigInt bitwise/shift/power` — until v0.2.0, ticket PMAT-013-FOLLOWUP
- `Lean assert` — until v0.3.0, ticket PMAT-009-FOLLOWUP

### Verification milestones

Ten runtime-verified semantic round-trip fixtures (emit → `rustc -O`
→ execute → `assert_eq!`):

- `factorial(n)` — recursive, `factorial(10) == 3628800`
- `fib(n)` — binary recursion, `fib(15) == 610`
- `gcd(a, b)` — tail recursion with `%`, `gcd(12, 18) == 6`
- `abs_val(x)` — statement-level if/else, `abs_val(-100) == 100`
- `sign(x)` — if/elif/else chain, `sign(i64::MIN) == -1`
- `bits(a, b)` — pins `& | ^ << >>` semantics, `bits(5, 3) == 14`
- `square_plus(a, b)` — pins `**` semantics, `square_plus(2, 3) == 10`
- `range_size(a, b)` — multi-assignment if-branches, `range_size(3, 7) == 4`
- `sum_to(n)` — while-loop accumulator, `sum_to(100) == 5050`
- `for_sum(n)` / `range_with_start` / `range_with_step` — for-in-range
  desugaring, all three `range(...)` shapes
- `factorial_iter(n)` — negative-step countdown, `factorial_iter(10) == 3628800`
- `safe_div(a, b)` — assert-precondition fixture, `safe_div(10, 2) == 5`

32 e2e tests across `crates/xpile/tests/transpile_e2e.rs`; ~60
workspace tests total.

## [0.0.1] - 2026-05-15

Initial crates.io name-reservation release. Placeholder binary that
prints a banner pointing at the GitHub repo. The full v0.1.0+ binary
is tracked in this workspace.

Published: <https://crates.io/crates/xpile/0.0.1>.
