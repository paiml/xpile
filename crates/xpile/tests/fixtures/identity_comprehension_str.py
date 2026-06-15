# PMAT-678: an identity comprehension `[w for w in words]` over list[str] must
# keep the element type (str) — it previously inferred List(I64) (loop var
# unbound during type inference) and rejected sorted/max over it.
def echo_sorted(words: list[str]) -> list[str]:
    return sorted([w for w in words])


def echo_max(words: list[str]) -> str:
    return max([w for w in words])


def filtered(words: list[str]) -> list[str]:
    return sorted([w for w in words if w != ""])


def ints_regression(xs: list[int]) -> list[int]:
    return sorted([x for x in xs])


def pairs(d: dict[str, int]) -> list[str]:
    return sorted([k for k, v in d.items()])
