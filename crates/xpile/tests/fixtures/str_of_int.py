# PMAT-502ad (Tranche 2): str(x) over an int -> its decimal string.
# Unblocks the common "prefix" + str(n) concatenation idiom.
def show(n: int) -> str:
    return "count: " + str(n)


def num_str(n: int) -> str:
    return str(n)


def neg_str(n: int) -> str:
    return str(0 - n)
