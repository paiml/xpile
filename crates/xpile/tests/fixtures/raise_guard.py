# PMAT-503a (first sub-slice of PMAT-503 exceptions): `raise Exc("msg")`
# as a guard clause lowers to `panic!("{}", <message>)`. The diverging
# panic lets the guarded `if` omit an `else` while the function keeps its
# single trailing `return`.
def checked_div(a: int, b: int) -> int:
    if b == 0:
        raise ValueError("b must be nonzero")
    return a // b


def must_be_positive(n: int) -> int:
    if n <= 0:
        raise ValueError("n must be positive")
    return n
