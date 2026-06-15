# PMAT-624: interpolating a tuple in an f-string. xpile emitted
# format!("{}", tuple), but tuples have no Display → E0277. Python renders the
# tuple repr: (1, 2), (1, 'a', True), (3.5, 2), (1, (2, 3)). Heterogeneous, so a
# per-position desugar: "(" + repr(p.0) + ", " + repr(p.1) + ... + ")".
# (Single-element tuples are blocked by a separate `(T)` vs `(T,)` type bug.)
def pair(p: tuple[int, int]) -> str:
    return f"p={p}"


def mixed(p: tuple[int, str, bool]) -> str:
    return f"{p}"


def floaty(p: tuple[float, int]) -> str:
    return f"{p}"


def nested(p: tuple[int, tuple[int, int]]) -> str:
    return f"{p}"


def list_of_pairs(xs: list[tuple[int, int]]) -> str:
    return f"{xs}"
