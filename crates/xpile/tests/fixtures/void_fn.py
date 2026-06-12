# PMAT-502bl (Tranche 2): void functions (-> None).
def check_pos(x: int) -> None:
    assert x > 0


def put(d: dict[str, int], k: str, v: int) -> None:
    d[k] = v
