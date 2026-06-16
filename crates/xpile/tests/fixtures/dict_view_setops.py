def inter(a: dict[str, int], b: dict[str, int]) -> int:
    return len(a.keys() & b.keys())


def uni(a: dict[str, int], b: dict[str, int]) -> int:
    return len(a.keys() | b.keys())


def diff(a: dict[str, int], b: dict[str, int]) -> int:
    return len(a.keys() - b.keys())


def sym(a: dict[str, int], b: dict[str, int]) -> int:
    return len(a.keys() ^ b.keys())


def key_vs_set(a: dict[str, int], s: set[str]) -> int:
    return len(a.keys() & s)
