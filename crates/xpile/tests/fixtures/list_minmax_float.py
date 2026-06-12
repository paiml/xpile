# PMAT-502h (Tranche 2): 1-arg min(xs)/max(xs) over a list[float].
# f64 lacks Ord, so the codegen uses a fold with f64::min / f64::max.
def lowest(xs: list[float]) -> float:
    return min(xs)


def highest(xs: list[float]) -> float:
    return max(xs)
