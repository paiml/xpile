# PMAT-623: interpolating a list in an f-string. xpile emitted
# format!("{}", Vec), but Vec has no Display → E0277. Python renders the list as
# its repr ([1, 2, 3], [True, False], ['a', 'b'], [[1, 2], [3, 4]]). Desugar to
# "[" + ", ".join([repr(e) for e in xs]) + "]", recursive for nested lists.
def ints(xs: list[int]) -> str:
    return f"v={xs}"


def floats(xs: list[float]) -> str:
    return f"v={xs}"


def bools(xs: list[bool]) -> str:
    return f"v={xs}"


def strs(xs: list[str]) -> str:
    return f"v={xs}"


def nested(xs: list[list[int]]) -> str:
    return f"grid={xs}"


def empty(xs: list[int]) -> str:
    return f"{xs}"
