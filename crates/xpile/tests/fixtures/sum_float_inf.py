# PMAT-679: sum() of floats uses Neumaier compensation (CPython 3.12+), but a
# non-finite partial poisoned the result (`inf - inf = NaN`). Python yields inf;
# the compensation must be skipped once the running total is non-finite.
def total(values: list[float]) -> float:
    return sum(values)
