# PMAT-502ci (Tranche 2): `for i in reversed(range(...))` — iterate a range
# descending. Desugars to a step -1 range (start b-1, stop a-1). Step-1 ranges
# (1-arg / 2-arg) only at first cut; reversed strided ranges are deferred.
def digits_desc(n: int) -> int:
    t = 0
    for i in reversed(range(n)):
        t = t * 10 + i
    return t


def mid(a: int) -> int:
    t = 0
    for i in reversed(range(2, 5)):
        t = t * 10 + i
    return t
