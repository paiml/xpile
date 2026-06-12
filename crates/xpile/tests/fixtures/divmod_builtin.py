# PMAT-502n (Tranche 2): divmod(a, b) -> (a // b, a % b) over ints.
# Pure desugar reusing the existing floor-div + mod ops.
def split_div(a: int, b: int) -> tuple[int, int]:
    return divmod(a, b)


def combine(a: int, b: int) -> int:
    q, r = divmod(a, b)
    return q * 100 + r
