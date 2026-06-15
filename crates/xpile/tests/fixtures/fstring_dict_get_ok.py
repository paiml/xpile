# PMAT-620: the supported f-string dict-access forms (NOT the bare no-default
# d.get(k) Optional). d.get(k, default) and d[k] both yield a concrete value and
# interpolate fine.
def with_default(d: dict[str, int], k: str) -> str:
    return f"val={d.get(k, 0)}"


def index(d: dict[str, int], k: str) -> str:
    return f"val={d[k]}"
