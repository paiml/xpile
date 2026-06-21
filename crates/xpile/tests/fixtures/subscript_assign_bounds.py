# PMAT-863 (HUNT-V30 #3): subscript ASSIGNMENT had no bounds check on the write
# path (the read path did), so an out-of-range index silently wrote a wrong slot
# (data corruption, exit 0). A negative literal also DOUBLE-normalized
# (frontend len-k, then codegen len+that). Now: single normalization + a bounds
# guard → Python IndexError. Cross-checked vs python3.


def valid_neg() -> int:
    xs: list[int] = [1, 2, 3]
    xs[-1] = 9
    return xs[2]


def valid_pos() -> int:
    xs: list[int] = [1, 2, 3]
    xs[1] = 9
    return xs[1]


def oob_neg() -> int:
    xs: list[int] = [1, 2, 3]
    xs[-5] = 9
    return xs[0]


def oob_runtime(i: int) -> int:
    xs: list[int] = [1, 2, 3]
    xs[i] = 9
    return xs[0]
