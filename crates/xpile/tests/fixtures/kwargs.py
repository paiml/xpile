# PMAT-474 (R5): keyword arguments f(x=1, y=2), reordered to positional
# at lowering using the callee's declared parameter order.
def area(x: int, y: int, w: int, h: int) -> int:
    return x + y + w + h


def mixed() -> int:
    return area(1, 2, h=4, w=3)


def all_kw() -> int:
    return area(x=10, y=20, w=30, h=40)
