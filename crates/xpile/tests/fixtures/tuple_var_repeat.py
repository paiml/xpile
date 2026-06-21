# PMAT-859 (HUNT-V29 #4): a tuple-TYPED variable repeated by an int literal
# (`a * 3`) emitted checked_mul on a tuple (rustc E0599) — try_repeat only handled
# a tuple LITERAL. It now expands a tuple-typed value to a fixed-arity tuple of
# per-element reads. Runtime-count repeat stays a documented limitation (Rust
# tuples aren't variadic). Cross-checked vs python3.


def var_repeat() -> int:
    a: tuple[int, int] = (1, 2)
    b = a * 3
    return len(b)


def left_count() -> int:
    a: tuple[int, int] = (5, 6)
    b = 2 * a
    return len(b)


def lit_repeat_regression() -> int:
    return len((1, 2) * 2)
