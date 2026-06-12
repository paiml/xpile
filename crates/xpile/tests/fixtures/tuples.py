# PMAT-494 (sprint): tuples — multiple return + tuple[...] annotation.
# `return a, b` lowers to Expr::TupleLit; `tuple[T0, T1]` -> Type::Tuple;
# Rust/Ruchy emit `(e0, e1)` / `(T0, T1)`. Unpacking (`a, b = f()`) is a
# follow-up slice; here the Rust driver destructures the returned tuple.
def divmod_pair(a: int, b: int) -> tuple[int, int]:
    return a // b, a % b


def tagged(name: str, count: int) -> tuple[str, int]:
    return name, count
