# PMAT-749 (HUNT-V14 #4 nf-reassign-immutable): a nested function whose body
# reassigns a local, an accumulator, or a parameter emitted an immutable `let`
# / immutable closure-arg → rustc E0384 ("cannot assign twice to immutable
# variable" / "cannot assign to immutable argument"). The outer function's
# mutability analysis (`compute_mutable_names`) does not descend into nested
# defs, so the nested scope's reassigned names were never marked `mut` — even
# though the IDENTICAL code at top level works. The fix computes the nested
# scope's mutable set and (a) marks each reassigned local's `let mut`, (b)
# carries a per-param `mut` flag so a reassigned parameter emits `|mut p|`.
# Cross-checked vs python3.


def doubler() -> int:
    # nested local reassigned
    def inner(n: int) -> int:
        r = n
        r = r * 2
        return r

    return inner(21)


def accumulate(xs: list[int]) -> int:
    # nested accumulator reassigned in a loop
    def total() -> int:
        acc = 0
        for x in xs:
            acc += x
        return acc

    return total()


def reassign_param() -> int:
    # nested parameter reassigned (needs |mut n|)
    def f(n: int) -> int:
        n = n + 1
        return n

    return f(5)
