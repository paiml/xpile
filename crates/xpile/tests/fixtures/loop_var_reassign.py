# PMAT-1080 (correctness): REASSIGNING the loop variable inside a for-body
# (`for x in xs: x = x.strip()`) emitted `for x in ...cloned()` without `mut`
# → rustc E0384 "cannot assign twice to immutable variable". Python semantics:
# rebinding the loop var does NOT mutate the iterated list (matches the
# `.cloned()` owned-value posture) — the binding just needs `mut`. The fix is
# PRECISE (a reassignment scan recursing if/while/nested loops, stopping at a
# shadowing nested loop) so unreassigned loop vars stay non-mut (clippy
# `unused_mut` stays happy).
def normalize(names: list[str]) -> list[str]:
    out: list[str] = []
    for name in names:
        name = name.strip()
        out.append(name)
    return out


def clamp_all(xs: list[int], lo: int, hi: int) -> list[int]:
    out: list[int] = []
    for v in xs:
        if v < lo:
            v = lo
        if v > hi:
            v = hi
        out.append(v)
    return out


def total(xs: list[int]) -> int:
    t: int = 0
    for n in xs:
        t = t + n
    return t
