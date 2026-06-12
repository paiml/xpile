# PMAT-502au (Tranche 2): dict pop d.pop(k) / d.pop(k, default) (expression form).
def take(d: dict[str, int], k: str) -> int:
    return d.pop(k)


def take_or(d: dict[str, int], k: str) -> int:
    return d.pop(k, 0)


def take_local() -> int:
    d = {"a": 1, "b": 2}
    v = d.pop("a")
    return v + len(d)
