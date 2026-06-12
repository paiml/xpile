# PMAT-502ax (Tranche 2): dict get-or-insert d.setdefault(k, default).
def getset(d: dict[str, int], k: str) -> int:
    return d.setdefault(k, 0)


def getset_present(d: dict[str, int], k: str) -> int:
    return d.setdefault(k, 99)


def local_setdefault() -> int:
    d: dict[str, int] = {}
    x = d.setdefault("a", 5)
    return x + len(d)
