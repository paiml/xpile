# PMAT-495 (sprint): enumerate / zip in for-loops -> Stmt::ForEachPair.
# `for i, x in enumerate(xs)` -> for (i, x) in xs.iter().cloned()
#   .enumerate().map(|(i,e)| (i as i64, e)); `for a, b in zip(xs, ys)`
# -> for (a, b) in xs.iter().cloned().zip(ys.iter().cloned()).
def sum_indexed(xs: list[int]) -> int:
    total = 0
    for i, x in enumerate(xs):
        total = total + i * x
    return total


def dot(xs: list[int], ys: list[int]) -> int:
    total = 0
    for a, b in zip(xs, ys):
        total = total + a * b
    return total
