#!/bin/sh
# bashrs_case_demo.sh — PMAT-1285
#
# End-to-end witness for shell `case … in … esac`. PMAT-1285 adds the
# `Stmt::ShellCase` IR node + `CaseArm`, the bashrs-backend renderer,
# and the frontend parser, so a real `.sh` case statement flows
# frontend -> mHIR -> bashrs-backend -> /bin/sh with deterministic
# stdout.
#
# Exercised:
#   single-pattern arm  — `a)` / `go)`
#   multi-pattern arm   — `b|c)` (the `|` pattern list)
#   default arm         — `*)`
#   nested loop in arm  — a `for` inside the `go)` arm body
#
# The matched word (`$fruit` / `$mode`) lowers to a ShellVar and
# renders back verbatim. Output is byte-for-byte deterministic;
# compared against `CASE_DEMO_EXPECTED` in shell_diff_exec.rs.

fruit=c
case $fruit in
  a) echo apple ;;
  b|c) echo bee-or-cee ;;
  *) echo unknown ;;
esac

mode=go
case $mode in
  go)
    for i in 1 2; do
      echo step $i
    done
    ;;
  *) echo halt ;;
esac
