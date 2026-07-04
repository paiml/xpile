#!/bin/sh
# bashrs_for_loop_demo.sh — PMAT-1268
#
# End-to-end witness for the bashrs-frontend `for`-loop parser. The
# IR (`Stmt::ShellLoop` + `LoopKind::For`) and the bashrs-backend
# renderer (`render_shell_loop`, PMAT-048 / PMAT-974) already existed;
# PMAT-1268 adds the FRONTEND parser so a real `.sh` for-loop flows
# bashrs-frontend → mHIR → bashrs-backend → /bin/sh and produces
# deterministic stdout.
#
# This is the first shell CONTROL-FLOW construct that round-trips
# through xpile. Before PMAT-1268 the frontend REFUSED every loop
# (PMAT-989 — refuse rather than shred into barewords). It still
# refuses while/until/if/case and nested loops; only the flat-body
# `for` dialect is handled.
#
# Constructs exercised on top of the flat-command subset:
#   Stmt::ShellLoop  — the `for … in …; do … done` block
#   LoopKind::For    — `var` + literal item list
#   Expr::ShellVar   — `$prefix` / `$i` referenced inside the body
#   ShellAssign      — the `prefix=` before the loop (loop composes
#                      with surrounding flat commands)
#
# Determinism: every output line is a literal echo of known content
# (no `date`/`pwd`/`seq`). The shell-side diff_exec gate compares
# stdout byte-for-byte against `FOR_LOOP_DEMO_EXPECTED` in
# shell_diff_exec.rs.

prefix=item

# Single-line loop: `; do … ; done` on one physical line.
for i in 1 2 3; do echo $prefix $i; done

# Multi-line loop: `do` and `done` on their own lines, multi-command
# body — proves body statements accumulate in order.
for name in alice bob
do
  echo hello $name
  echo bye $name
done
