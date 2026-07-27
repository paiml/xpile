#!/bin/sh
# bashrs_arith_shift_demo.sh — PMAT-1377
#
# EXECUTION witness for the ONE `<<` that is NOT a here-document: the
# arithmetic LEFT SHIFT inside `$((…))`.
#
# PMAT-1377 widened the here-doc refusal from `starts_with("<<")` to
# `contains("<<")` so that the attached spellings `cat<<EOF` and
# `cat 0<<EOF` stop shredding. That widening puts `$((1 << 2))` directly
# in the blast radius: PMAT-090 captures a whole arithmetic expansion as
# ONE `Bare` token, so a naive `contains` would have refused every left
# shift in the language.
#
# The exemption is therefore asserted by EXECUTION, not by emit: this
# fixture round-trips frontend -> mHIR -> bashrs-backend -> /bin/sh and
# its arithmetic must still be evaluated by the shell. An emit-only
# assertion would pass even if the operator were mangled in a way the
# shell then computed differently.
#
# Exercised:
#   $((1 << 2))    — spaced shift, as a command argument
#   $((1<<2))      — TIGHT shift (no spaces), the spelling closest to the
#                    `cat<<EOF` evasion this slice closes
#   x=$((3<<3))    — the ASSIGNMENT form, which reaches the guard by a
#                    different route (the full-line tokenize trips the
#                    "adjacent to a bareword" rule, so the guard fails
#                    open and the assignment branch handles it)
#   $((8 >> 1))    — the right-shift sibling, so a future guard that
#                    keys on "shift" generally is also pinned
#   echo "a << b"  — a QUOTED `<<`, ordinary text, must survive as text
#
# Output is byte-for-byte deterministic; compared against
# ARITH_SHIFT_DEMO_EXPECTED in shell_diff_exec.rs.

echo $((1 << 2))
echo $((1<<2))

x=$((3<<3))
echo $x

echo $((8 >> 1))

echo "a << b"
