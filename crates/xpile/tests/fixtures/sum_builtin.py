# PMAT-498b (Tranche 2): sum(xs) over numeric lists.
# list[int] -> xs.iter().sum::<i64>(); list[float] -> ...sum::<f64>().
def total(xs: list[int]) -> int:
    return sum(xs)


def ftotal(xs: list[float]) -> float:
    return sum(xs)
