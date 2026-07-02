# PMAT-1052: `self.<field>[k].append(e)` / `obj.<field>[k].append(e)` — the
# grouping-in-a-class idiom, the field analogue of the already-supported
# `d[k].append(e)` (IndexAppend). resolve_subscript_append_base accepts a
# struct-field base and emits IndexAppend with a dotted `obj.field` codegen
# base (valid Rust, no meta-HIR change). Covers dict-field grouping,
# list-of-list field push, an append-only method (&mut self via
# for_target_mutated_ast), a local receiver outside a method (mut via the
# pre-walk), and int→float inner-elem widening. Verified vs CPython.
class Grouper:
    g: dict[int, list[str]]

    def __init__(self) -> None:
        self.g = {}

    def add(self, w: str) -> None:
        k = len(w)
        if k not in self.g:
            self.g[k] = []
        self.g[k].append(w)


class Matrix:
    rows: list[list[int]]
    scores: dict[str, list[float]]

    def __init__(self) -> None:
        self.rows = [[1], [2]]
        self.scores = {"a": [0.5]}

    def push_row(self, i: int, v: int) -> None:
        self.rows[i].append(v)

    def push_score(self, k: str, n: int) -> None:
        self.scores[k].append(n)


def grouping() -> int:
    gr: Grouper = Grouper()
    gr.add("a")
    gr.add("bb")
    gr.add("cc")
    gr.add("dd")
    return len(gr.g[2]) * 10 + len(gr.g)


def field_pushes() -> float:
    m: Matrix = Matrix()
    m.push_row(0, 9)
    m.push_score("a", 2)
    return float(m.rows[0][1]) + m.scores["a"][0] + m.scores["a"][1]
