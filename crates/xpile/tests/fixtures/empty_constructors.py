# PMAT-502i (Tranche 2): empty collection constructors set()/dict()/list().
# Element type comes from a binding annotation or a subsequent mutation.
def set_then_add() -> int:
    s = set()
    s.add(1)
    s.add(2)
    s.add(2)
    return len(s)


def set_annotated() -> int:
    s: set[int] = set()
    return len(s)


def list_then_append() -> int:
    xs = list()
    xs.append(10)
    xs.append(20)
    return len(xs)


def dict_annotated() -> int:
    d: dict[int, int] = dict()
    return len(d)
