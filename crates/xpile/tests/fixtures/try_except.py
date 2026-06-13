def safe_div(a: int, b: int) -> int:
    # ZeroDivisionError → xpile's floor-div panics → caught, fallback -1.
    try:
        return a // b
    except ZeroDivisionError:
        return -1


def safe_index(xs: list[int], i: int) -> int:
    # IndexError → list index panics → caught (bare except), fallback 0.
    try:
        return xs[i]
    except:
        return 0


def safe_lookup(d: dict[str, int], k: str) -> int:
    # KeyError → HashMap index panics on a missing key → caught, fallback -1.
    try:
        return d[k]
    except KeyError:
        return -1
