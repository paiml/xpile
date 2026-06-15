# PMAT-628: a non-Copy variable used more than once — twice in a list literal
# [inner, inner], or appended twice g.append(row) — emitted a move-then-use
# (E0382). The reused non-Copy var is now cloned (mirrors PMAT-588 call-arg
# clone). Distinct elements are not cloned. (Python aliases the same object; the
# clone gives independent copies — the documented value-semantics divergence —
# but it now compiles.)
def literal() -> list[list[int]]:
    inner: list[int] = [1, 2, 3]
    grid: list[list[int]] = [inner, inner]
    return grid


def appended() -> list[list[int]]:
    row: list[int] = [0, 0]
    g: list[list[int]] = []
    g.append(row)
    g.append(row)
    return g


def distinct() -> list[list[int]]:
    a: list[int] = [1]
    b: list[int] = [2]
    return [a, b]
