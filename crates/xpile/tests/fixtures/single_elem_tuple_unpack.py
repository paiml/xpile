# PMAT-711: a single-element tuple-unpack `x, = t` (and `(x,) = t`) emitted
# `let (x) = t` (grouping, binds the whole tuple to x) → E0308. Now emits
# `let (x,) = t` (a 1-tuple destructure). Multi-element unpack is unchanged.
def unwrap(t: tuple[int]) -> int:
    x, = t
    return x


def unwrap_str(t: tuple[str]) -> str:
    (y,) = t
    return y
