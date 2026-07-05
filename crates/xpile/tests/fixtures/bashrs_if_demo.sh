#!/bin/sh
# bashrs_if_demo.sh — PMAT-1283
#
# End-to-end witness for shell `if`/`then`/`else`/`fi` conditionals.
# PMAT-1283 adds the `Stmt::ShellIf` IR node, the bashrs-backend
# renderer, and the frontend parser, so a real `.sh` conditional flows
# frontend -> mHIR -> bashrs-backend -> /bin/sh with deterministic
# stdout.
#
# Exercised:
#   if/then/fi           — the `only big` block (no else, condition true)
#   if/then/else/fi      — the `parity` block (else arm taken)
#   if nested in a for    — the `loop-pick` block (if inside a loop body)
#
# The `[ … ]` condition is captured as an opaque LitStr and printed
# back verbatim; `$VAR` inside it expands at shell run time. Output is
# byte-for-byte deterministic; compared against `IF_DEMO_EXPECTED` in
# shell_diff_exec.rs.

x=5
if [ $x -gt 3 ]; then
  echo big
fi

n=4
if [ $n -eq 3 ]; then
  echo three
else
  echo not-three
fi

for i in 1 2 3; do
  if [ $i -eq 2 ]; then
    echo picked $i
  fi
done

# PMAT-1284: an elif chain (the second arm is taken for g=2).
g=2
if [ $g -eq 1 ]; then
  echo grade-a
elif [ $g -eq 2 ]; then
  echo grade-b
elif [ $g -eq 3 ]; then
  echo grade-c
else
  echo grade-f
fi
