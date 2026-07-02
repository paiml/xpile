# PMAT-1045 (sweep #12): the leaf dict key in a FieldIndexAssign /
# NestedSubscriptAssign store was MOVED into `.insert`, so a non-Copy (str)
# key reused after the store — `self.cells[r] = {}` then `self.cells[r][c] = v`
# with `r` a param used again — was E0382. The leaf key now clones (mirrors
# the single-level DictSet PMAT-852 emission). Verified vs CPython.
class Table:
    cells: dict[str, dict[int, float]]

    def __init__(self) -> None:
        self.cells = {}

    def put(self, r: str, c: int, v: float) -> None:
        if r not in self.cells:
            self.cells[r] = {}
        self.cells[r][c] = v


class Names:
    m: dict[str, int]

    def __init__(self) -> None:
        self.m = {}

    def add(self, k: str) -> str:
        self.m[k] = len(k)
        return k + "!"


def table_sum() -> float:
    t: Table = Table()
    t.put("a", 1, 2.5)
    t.put("a", 2, 3.5)
    t.put("b", 1, 1.0)
    return t.cells["a"][1] + t.cells["a"][2] + float(len(t.cells))


def key_reused() -> int:
    n: Names = Names()
    k: str = "hello"
    tag: str = n.add(k)
    return n.m[k] + len(tag) + len(k)
