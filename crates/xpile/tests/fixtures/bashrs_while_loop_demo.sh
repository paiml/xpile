#!/bin/sh
# bashrs_while_loop_demo.sh — PMAT-1276
#
# End-to-end witness for the bashrs-frontend `while` / `until` loop
# parser. The IR (`LoopKind::While` / `Until`) and the bashrs-backend
# renderer already existed (PMAT-048 / PMAT-974); PMAT-1276 adds the
# FRONTEND parser (extending the PMAT-1268 `for`-loop parser) so a real
# `.sh` while/until loop flows bashrs-frontend → mHIR → bashrs-backend
# → /bin/sh and produces deterministic stdout.
#
# Constructs exercised:
#   Stmt::ShellLoop / LoopKind::While  — `while COND; do … done`
#   Stmt::ShellLoop / LoopKind::Until  — `until COND; do … done`
#   Opaque condition (Expr::LitStr)    — the `[ … ]` test round-trips
#                                        VERBATIM; `$i`/`$n` inside it
#                                        expand at shell run time
#   ShellAssign + $(( )) arithmetic    — the loop-variable step
#
# Determinism: both loops count over a fixed bound, so the output is
# byte-for-byte stable. The shell-side diff_exec gate compares stdout
# against `WHILE_LOOP_DEMO_EXPECTED` in shell_diff_exec.rs.
#
# Terminating by construction: `while` counts UP to a bound and `until`
# counts DOWN to zero — neither spins (an infinite loop would hang the
# gate).

i=0
while [ $i -lt 3 ]; do
  echo tick $i
  i=$((i+1))
done

n=3
until [ $n -eq 0 ]; do
  echo down $n
  n=$((n-1))
done
