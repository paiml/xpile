def pop_in_range(xs: list[int]) -> bool:
    # The middle operand `xs.pop()` must be evaluated EXACTLY ONCE (Python
    # semantics). The previous lowering cloned it into both sub-comparisons of
    # `(0 < xs.pop()) and (xs.pop() < 100)`, popping twice — a wrong result
    # (and an empty-pop panic for a 1-element list).
    return 0 < xs.pop() < 100


def four_term(a: int, b: int, c: int, d: int) -> bool:
    # Every interior operand (b, c) is shared by two sub-comparisons.
    return a < b < c < d


def eq_chain(a: int, b: int, c: int) -> bool:
    return a == b == c
