def widen_silently(xs: list[int]) -> int:
    # V29-5 (PMAT-883, silent-wrong): an `int`/`float`-mismatched ternary in the
    # list-comprehension element position. The int branch (`x`, an int) was
    # silently widened to f64 (`((x) as f64)`), so every int element printed as
    # `N.0` — a heterogeneous int/float list, which is out of scope for
    # C-XLATE-PY-LIST-TO-VEC. xpile must CLEAN-REJECT, mirroring the
    # heterogeneous list-literal reject (`[1, 2.0]`).
    ys = [x if x > 0 else 1.0 for x in xs]
    return len(ys)
