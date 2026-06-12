# PMAT-504b (Tranche 2): multi-parameter + nullary closures.
def add(a: int, b: int) -> int:
    f = lambda x, y: x + y
    return f(a, b)


def nullary() -> int:
    g = lambda: 42
    return g()


def combine(a: int, b: int, c: int) -> int:
    h = lambda x, y, z: x * y + z
    return h(a, b, c)
