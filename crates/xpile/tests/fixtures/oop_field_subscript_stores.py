# PMAT-1037: subscript stores through struct FIELDS — the dominant remaining
# class-state refusal ("non-Name subscript-assignment target"). Covered here:
# (1) `self.counts[i] = v` and `+=` inside methods (list fields);
# (2) dict fields `self.m[k] = v` (insert-or-overwrite) + bool→i64 key
#     coercion (PMAT-829 parity);
# (3) nested `self.cells[r][c] = v` and mixed dict-of-list `self.grid[k][i]`;
# (4) LOCAL struct receivers `c.counts[0] = v` / `+=` (same peel, obj != self);
# (5) negative index wraps from the end (NestedSubscriptAssign emitter parity);
# (6) int value widens into a float-typed leaf (PMAT-1017 FieldAssign parity);
# (7) `for c in cs: c.counts[0] = v` drives iter_mut (foreach_elem_mutated);
# (8) method args ride the free-fn reuse-clone (`g.score(q) + g.score(q)`
#     was E0382 — clone_reused_call_args now covers Expr::MethodCall, paired
#     with the check_expr_for_alias_mutate MethodCall guard arm).
# Differentially verified vs CPython (MATCH '14\n56\n56\n4.5\n14\n6').
class Counter:
    counts: list[int]

    def __init__(self) -> None:
        self.counts = [0, 0, 0]

    def hit(self, i: int) -> None:
        self.counts[i] = self.counts[i] + 1

    def bump(self, i: int, v: int) -> None:
        self.counts[i] += v

    def last(self, v: int) -> None:
        self.counts[-1] = v

    def get(self, i: int) -> int:
        return self.counts[i]


class Board:
    cells: list[list[int]]
    grid: dict[str, list[int]]
    marks: dict[int, int]

    def __init__(self) -> None:
        self.cells = [[0, 0], [0, 0]]
        self.grid = {"r": [0, 0]}
        self.marks = {1: 0}

    def set_cell(self, r: int, c: int, v: int) -> None:
        self.cells[r][c] = v

    def set_grid(self, k: str, i: int, v: int) -> None:
        self.grid[k][i] = v

    def mark_true(self) -> None:
        self.marks[True] = 9


class Gauge:
    xs: list[float]

    def __init__(self) -> None:
        self.xs = [0.5, 1.5]

    def store(self, i: int, n: int) -> None:
        self.xs[i] = n

    def score(self, q: list[int]) -> int:
        return len(q)


def self_stores() -> int:
    c = Counter()
    c.hit(1)
    c.hit(1)
    c.bump(1, 3)
    c.last(9)
    return c.get(1) + c.get(2)


def board_stores() -> int:
    b = Board()
    b.set_cell(1, 0, 42)
    b.set_grid("r", 1, 5)
    b.mark_true()
    return b.cells[1][0] + b.grid["r"][1] + b.marks[1]


def local_store() -> int:
    c = Counter()
    c.counts[0] = 50
    c.counts[2] += 6
    return c.counts[0] + c.counts[2]


def widen_store() -> float:
    g = Gauge()
    g.store(0, 3)
    return g.xs[0] + g.xs[1]


def loop_elem_store() -> int:
    cs = [Counter(), Counter()]
    for c in cs:
        c.counts[0] = 7
    return cs[0].counts[0] + cs[1].counts[0]


def method_arg_reuse() -> int:
    g = Gauge()
    q = [1, 2]
    return g.score(q) + g.score(q) + len(q)
