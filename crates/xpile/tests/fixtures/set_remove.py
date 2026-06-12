# PMAT-502av (Tranche 2): set element removal s.remove(x) / s.discard(x).
def drop(s: set[int], x: int) -> int:
    s.remove(x)
    return len(s)


def disc(s: set[int], x: int) -> int:
    s.discard(x)
    return len(s)


def drop_local() -> int:
    s = {1, 2, 3}
    s.remove(2)
    return len(s)
