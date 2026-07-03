# PMAT-1085 (skeptic-pass PMAT-1081, finding a): the inner `for x` rebinds the
# enclosing loop's variable. Python keeps ONE function-scoped binding — the
# `total = total + x` after the inner loop reads the INNER loop's last value
# (CPython: 150) — but the emitted Rust `for` introduces a new scoped binding,
# so the same read silently saw the OUTER iteration's value (96). REFUSED on
# the value-semantics lanes; the WASM reference lane (function-scoped locals)
# stays allowed and matches CPython.
def f() -> int:
    total: int = 0
    for x in [1, 2, 3]:
        for x in [10, 20]:
            total = total + x
        total = total + x
    return total
