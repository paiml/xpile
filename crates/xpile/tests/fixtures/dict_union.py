# PMAT-593: PEP 584 dict union. `a | b` is a new dict (≡ {**a, **b}, b wins on
# key conflicts); `a |= b` updates a in place (same as a.update(b)). xpile
# previously fell through to a generic BitOr, emitting `HashMap | HashMap`
# (rustc E0369).
def merged(a: dict[str, int], b: dict[str, int]) -> int:
    c: dict[str, int] = a | b
    return c["x"] + c["y"] + c["z"] + len(c) * 1000


def in_place(a: dict[str, int], b: dict[str, int]) -> int:
    a |= b
    return a["y"] + len(a) * 100
