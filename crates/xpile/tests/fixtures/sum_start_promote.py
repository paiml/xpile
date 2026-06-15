# PMAT-703: sum(xs, start) promotes int+float like Python — sum(list[int], 0.0)
# is a float and sum(list[float], 0) is a float. The result is float iff either
# the elements or the start is float; xpile maps whichever side is int up to f64
# (was rejected: "start type F64 must match the list element type").
def int_floatstart(xs: list[int]) -> float:
    return sum(xs, 0.0)


def float_intstart(xs: list[float]) -> float:
    return sum(xs, 0)
