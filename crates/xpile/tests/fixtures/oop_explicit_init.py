# PMAT-1016B: explicit `__init__` → an associated constructor. A STRAIGHT-LINE
# body (every stmt `self.<field> = <expr>`, every declared field exactly once)
# synthesizes `pub fn __init__(<params>) -> Self { Self { f: <rhs>, … } }`
# (fields in ASSIGNMENT order = Python's evaluation order); `Rect(3, 4)` routes
# through the `Class::__init__` FnSig (arity/kwargs/defaults/coercion ride the
# ordinary call machinery). Params need not mirror the fields (renamed a/b,
# computed area). Control-flow bodies, self-reads (`self.y = self.x + 1`),
# double/missing field assignment all refuse precisely.
class Rect:
    w: int
    h: int
    area: int

    def __init__(self, a: int, b: int) -> None:
        self.w = a
        self.h = b
        self.area = a * b

    def grow(self) -> None:
        self.area = self.area + self.w

    def size(self) -> int:
        return self.area


def build(a: int, b: int) -> int:
    r = Rect(a, b)
    r.grow()
    return r.size()


def build_kw(a: int, b: int) -> int:
    q = Rect(b=b, a=a)
    return q.w + q.h + q.size()
