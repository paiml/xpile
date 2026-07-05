#!/bin/sh
# bashrs_nested_loop_demo.sh — PMAT-1281
#
# End-to-end witness for NESTED shell loops. The bashrs-backend
# `render_shell_loop` already recursed (PMAT-048/974); PMAT-1281 makes
# the FRONTEND produce the nested `Stmt::ShellLoop` shape, so a real
# `.sh` with loops-inside-loops flows frontend -> mHIR -> bashrs-backend
# -> /bin/sh and produces deterministic stdout.
#
# Exercised:
#   for-in-for       — same-dialect nesting (the `cell …` block)
#   while-wrapping-for — MIXED-dialect nesting + a `$((..))` step
#                        inside a loop body (the `mix …` block)
#
# Both loops terminate by construction (fixed bounds / countdown), so
# the gate never hangs. The shell-side diff_exec gate compares stdout
# against `NESTED_LOOP_DEMO_EXPECTED` in shell_diff_exec.rs.

for i in 1 2; do
  for j in a b; do
    echo cell $i $j
  done
done

n=2
while [ $n -gt 0 ]; do
  for k in x y; do
    echo mix $n $k
  done
  n=$((n-1))
done
