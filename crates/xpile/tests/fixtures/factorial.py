# PMAT-036: return-type `BigInt` triggers PMAT-013's implicit
# promotion. Every `int` param is auto-lifted to BigInt, the body
# runs in BigInt mode end-to-end, and recursive multiplication
# never overflows. This is the canonical case the C-PY-INT-ARITH
# slow path was pointing at via the panic message — now closed in
# v0.1.0 via annotation, not a codegen heuristic.
#
# `from __future__ import annotations` makes CPython treat the
# `BigInt` annotation as a string and defer its evaluation, so the
# diff_exec gate can `python3 factorial.py` to compare CPython
# output against the transpiled-Rust binary. `BigInt` is xpile's
# metadata-only synonym for "Python int (unbounded)"; CPython's
# real `int` already behaves that way.
from __future__ import annotations

def factorial(n: int) -> BigInt:
    return 1 if n <= 1 else n * factorial(n - 1)
