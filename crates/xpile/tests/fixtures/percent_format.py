def items(n: int) -> str:
    return "%d items" % n


def kv(k: str, n: int) -> str:
    return "%s=%d" % (k, n)


def frac(x: float) -> str:
    return "%f" % x


def pct(n: int) -> str:
    return "100%% of %d" % n


def two(a: str, b: str) -> str:
    return "%s and %s" % (a, b)
