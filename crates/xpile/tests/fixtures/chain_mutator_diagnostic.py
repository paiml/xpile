# PMAT-1051 (sweep #12): a container mutator through a DEEP receiver chain —
# `g.rows[0].append(9)` (mutation through a subscript of a field) — reached
# the subprocess-recognizer fall-through and refused with the factually WRONG
# "only subprocess.run([...]) is recognised" message. It now refuses with a
# PRECISE message naming the unsupported receiver shape (the PMAT-989/1027
# honest-diagnostics posture). The refusal itself is correct (Python shares
# the element; the value model can't express it — Rc<RefCell> is deferred).
class Grid:
    rows: list[list[int]]

    def __init__(self) -> None:
        self.rows = []


def main() -> None:
    row: list[int] = [1]
    g: Grid = Grid()
    g.rows.append(row)
    g.rows[0].append(9)
    print(g.rows)
