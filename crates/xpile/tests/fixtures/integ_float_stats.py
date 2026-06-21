# PMAT-875 (integration regression): float accumulation + min/max tracking + avg
# (true division) + str() of floats + concat. Cross-checked vs python3.


def stats(xs: list[float]) -> str:
    total: float = 0.0
    lo: float = xs[0]
    hi: float = xs[0]
    for x in xs:
        total = total + x
        if x < lo:
            lo = x
        if x > hi:
            hi = x
    avg: float = total / len(xs)
    return str(lo) + "/" + str(hi) + "/" + str(avg)
