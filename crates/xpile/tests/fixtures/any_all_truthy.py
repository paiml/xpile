# PMAT-665: any()/all() over a list[int]/[float]/[str] apply Python per-element
# truthiness (nonzero / non-empty). These emitted a bare `any(xs)` free call
# (E0425) — only list[bool] was handled. Each element is now mapped to a bool
# (int → != 0, float → != 0.0, str → non-empty) before the reduce.


def any_int(xs: list[int]) -> int:
    return 1 if any(xs) else 0


def all_int(xs: list[int]) -> int:
    return 1 if all(xs) else 0


def any_str(ss: list[str]) -> int:
    return 1 if any(ss) else 0


def all_float(xs: list[float]) -> int:
    return 1 if all(xs) else 0


def any_bool_regression(bs: list[bool]) -> int:
    return 1 if any(bs) else 0
