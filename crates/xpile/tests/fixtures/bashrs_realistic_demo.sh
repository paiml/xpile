#!/bin/sh
# bashrs_realistic_demo.sh — PMAT-052
#
# Comprehensive end-to-end demo: every Layer B construct shipped
# at v0.1.0 (PMAT-039 through PMAT-051) composes in a single
# realistic script. The shell-side diff_exec gate transpiles this
# file via bashrs-frontend → bashrs-backend and runs the emitted
# shell, comparing stdout to a deterministic expected output.
#
# Constructs exercised (cross-reference to spec table in
# sub/bashrs-merger.md Layer B):
#
#   Stmt::Cmd            — every `echo` line
#   Stmt::ShellAssign    — `GREETING=` / `EXCLAMATION=` / `NAME=` /
#                          `ZERO=`
#   Expr::LitStr         — bareword args
#   Expr::QuotedString   — "..." and '...' values
#   Expr::ShellVar       — $NAME / ${NAME} references
#   Expr::CommandSubstitution — $(echo ...) — verifies substitution
#                          composes with assignment and quoting
#   QuotingStrategy      — single + double both present
#
# NOT exercised at v0.1.0 (parser limitations):
#   Stmt::Pipeline   — not in this fixture (pipelines aren't
#                      meaningfully testable through echo)
#   Stmt::ShellLoop  — parser doesn't recognise multi-line loops yet
#   Special params   — $1 / $@ / $? not recognised yet
#   Backticks        — `cmd` not recognised yet
#
# Determinism: every shell-visible output is a literal echo of
# known content. No `date` / `pwd` / `uname` etc. — those vary by
# environment. The test compares stdout byte-for-byte against
# `EXPECTED_OUTPUT` constant in shell_diff_exec.rs.

GREETING=hello
EXCLAMATION="how are you"
NAME='Noah Gift'
ZERO=$(echo zero)

echo $GREETING world
echo ${EXCLAMATION}
echo "Hi, $NAME"
echo started $ZERO done
