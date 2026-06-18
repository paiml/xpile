# PMAT-806 (HUNT-V21): a bare range(...) bound to a variable (r = range(5)) emitted
# an undefined `range(5i64)` free call typed i64 (rustc E0425). The for-loop iter,
# list(range(...)), and reduction-builtin paths intercept range earlier; a
# value-position range now materializes to a Vec<i64> (like list(range(...))), so
# the binding is list[int] and sum/len/iteration work. Cross-checked vs python3.


def from_range() -> int:
    r = range(5)
    return sum(list(r))


def with_step() -> int:
    r = range(1, 10, 2)
    return sum(list(r))


def iterate_bound() -> int:
    r = range(4)
    t = 0
    for x in r:
        t += x
    return t
