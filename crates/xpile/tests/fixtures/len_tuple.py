# PMAT-654: len() of a tuple. Rust tuples have no `.len()` method, so the
# `Expr::Len` emission (`t.len()`) failed to compile (E0599). A Python tuple's
# length is fixed by its type, so len(tuple) folds to the arity constant.


def len_tuple_param(t: tuple[int, int, int]) -> int:
    return len(t)


def len_tuple_literal() -> int:
    t = (1, 2, 3, 4)
    return len(t)


def len_tuple_mixed(p: tuple[int, str]) -> int:
    return len(p)


def len_list_regression(xs: list[int]) -> int:
    # list len still uses .len()
    return len(xs)
