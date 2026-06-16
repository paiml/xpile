def risky(n: int) -> int:
    if n < 0:
        raise ValueError("neg")
    return 100 // n


def catch_value(n: int) -> int:
    try:
        return risky(n)
    except ValueError:
        return -1


def catch_all(n: int) -> int:
    try:
        return risky(n)
    except Exception:
        return -99


def lookup(d: dict[str, int], k: str) -> int:
    try:
        return d[k]
    except KeyError:
        return -1
