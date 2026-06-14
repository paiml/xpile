def srt(xs: list[float]) -> list[float]:
    # `sorted()` over a float list previously emitted `Vec<f64>::sort()`, which
    # fails to compile (f64: Ord not satisfied). Now uses `sort_by(partial_cmp)`.
    return sorted(xs)


def srt_rev(xs: list[float]) -> list[float]:
    return sorted(xs, reverse=True)


def srt_int(xs: list[int]) -> list[int]:
    # int list keeps the plain `.sort()` — must remain unchanged.
    return sorted(xs)
