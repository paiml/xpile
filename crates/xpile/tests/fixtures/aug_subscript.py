# PMAT-497 (Tranche 2): augmented subscript assignment d[k] += v / xs[i] += v.
# Desugars to d[k] = d[k] <op> v, reusing DictGet/Index + DictSet/IndexAssign.
def counts() -> dict[str, int]:
    d: dict[str, int] = {}
    d["a"] = 1
    d["a"] += 5
    return d


def bump(xs: list[int]) -> list[int]:
    i = 0
    while i < len(xs):
        xs[i] += 10
        i += 1
    return xs
