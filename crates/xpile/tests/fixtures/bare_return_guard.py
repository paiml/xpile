# PMAT-502bv (Tranche 2): bare `return` (no value) in a void function — the
# early-exit guard-clause shape `if invalid: return`. Lowers to `return ();`.
def guard_div(v: int) -> None:
    if v == 0:
        return
    x = 100 // v
    assert x >= 0


def push_pos(xs: list[int], v: int) -> None:
    if v < 0:
        return
    xs.append(v)
