# PMAT-1364: `bool` in a `range(...)` bound position.
#
# Python's `bool` is a subclass of `int` (`True == 1`, `False == 0`), so
# `range(True)` is `range(1)` and `range(False)` is empty. Before this slice the
# desugar passed the bound to the emitter RAW, so `for i in range(b)` emitted
# `let __forstop1: i64 = b;` — rustc E0308 — and the `reversed(...)` path emitted
# `(b).checked_sub(1i64)`, rustc E0599. `xpile transpile` exited 0 either way, so
# the defect only surfaced a whole backend round-trip later.
#
# This fixture is the differential half: every bound position that can hold a
# bool, run under CPython and under the transpiled+rustc'd binary, byte-compared.
# The compile itself is the regression pin — the pre-fix emitter produced 8 rustc
# errors on exactly these shapes.


def stop_param(b: bool) -> int:
    t: int = 0
    for i in range(b):
        t = t + 1
    return t


def stop_literal() -> int:
    t: int = 0
    for i in range(True):
        t = t + 100
    return t


def stop_comparison(x: int) -> int:
    t: int = 0
    for i in range(x > 2):
        t = t + 1
    return t


def start_param(b: bool) -> int:
    t: int = 0
    for i in range(b, 3):
        t = t + i
    return t


def both_bounds(lo: bool, hi: bool) -> int:
    t: int = 0
    for i in range(lo, hi):
        t = t + 1
    return t


def reversed_stop(b: bool) -> int:
    t: int = 0
    for i in reversed(range(b)):
        t = t + 1
    return t


def reversed_start(b: bool) -> int:
    t: int = 0
    for i in reversed(range(b, 4)):
        t = t + i
    return t


def main() -> None:
    print(stop_param(True))
    print(stop_param(False))
    print(stop_literal())
    print(stop_comparison(5))
    print(stop_comparison(1))
    print(start_param(True))
    print(start_param(False))
    print(both_bounds(False, True))
    print(both_bounds(True, False))
    print(reversed_stop(True))
    print(reversed_stop(False))
    print(reversed_start(True))
    print(reversed_start(False))
