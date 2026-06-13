def to_tuple(xs: list[int]) -> list[int]:
    # `tuple(<iterable>)` has no Rust fixed-arity target — xpile must REJECT
    # this with a clear diagnostic rather than silently emit an undefined
    # `tuple(...)` call that fails rustc (a "transpile-success ⟹ valid Rust"
    # guarantee violation).
    return tuple(xs)
