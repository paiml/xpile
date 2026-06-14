# PMAT-616: sorting a float list that contains NaN. xpile lowered the keyless
# float sort to `partial_cmp(...).unwrap()`, which PANICS on NaN (partial_cmp
# returns None). Python's sorted() does NOT raise on NaN — it produces an
# undefined-but-non-crashing order. The comparator now falls back to Equal, so
# the transpiled code no longer panics; the finite elements still sort correctly.
def sort_floats(xs: list[float]) -> list[float]:
    return sorted(xs)


def sort_desc(xs: list[float]) -> list[float]:
    return sorted(xs, reverse=True)


def sort_inplace(xs: list[float]) -> list[float]:
    xs.sort()
    return xs
