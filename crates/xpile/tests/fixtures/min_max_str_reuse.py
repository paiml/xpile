# PMAT-1171: a `str` operand of `min()`/`max()` that is READ AGAIN later in the
# function must be CLONED, not moved. Codegen lowers `min`/`max` to `.min()` /
# `.max()`, which CONSUME their operands (`String: Ord`, taken by value), so a
# bare `min(a, b)` moved `a`/`b` and a later read was rustc E0382 (accept-then-
# fail: invalid Rust). The canonical clone-if-reused helper (PMAT-588/628) now
# wraps only reused non-Copy operands, so numeric min/max (Copy) and single-use
# str min/max stay byte-identical (no clone).
def pick_min(a: str, b: str) -> str:
    m: str = min(a, b)
    # `a` is read AGAIN below — the `min` above must not have moved it.
    return m + a


def pick_max(a: str, b: str) -> str:
    # both `a` and `b` are read again after `max` — both must be cloned.
    return max(a, b) + a + b
