# PMAT-626: str(list)/str(tuple) and print(list)/print(tuple) were rejected
# (str fell through → I64 mismatch; print declined "list/dict/set repr deferred").
# Both now reuse the build_list_repr/build_tuple_repr desugar from f-string
# interpolation (PMAT-623/624) → the Python repr.
def s_list(xs: list[int]) -> str:
    return str(xs)


def s_tuple(p: tuple[int, str]) -> str:
    return str(p)


def s_nested(xs: list[list[int]]) -> str:
    return str(xs)


def p_list(xs: list[int]) -> None:
    print(xs)


def p_tuple(p: tuple[int, int]) -> None:
    print(p)
