# PMAT-1095 (skeptic pass PMAT-1090, C-F2): CPython binds a tuple `for`
# target left-to-right — in `for x, x in zip(a, b)` the LAST occurrence
# wins, and nothing can observe the earlier per-iteration binding. The
# emitted Rust pattern `for (x, x)` was rustc E0416 ("identifier bound
# more than once") on this valid Python. Duplicated plain-Name components
# now rewrite to `_` (all but the last occurrence), which is legal to
# repeat in one Rust pattern and binds nothing.
def dup_zip() -> int:
    total: int = 0
    for x, x in zip([1, 3], [2, 4]):
        total = total + x
    return total


def dup_enumerate() -> int:
    t: int = 0
    for x, x in enumerate([10, 20]):
        t = t + x
    return t


def dup_zip3() -> int:
    u: int = 0
    for x, y, x in zip([1, 2], [3, 4], [5, 6]):
        u = u + x + y
    return u
