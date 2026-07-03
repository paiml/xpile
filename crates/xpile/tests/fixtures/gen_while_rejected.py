# PMAT-1083 (skeptic pass PMAT-1081, probe p2b-infinite): a `while True`
# generator is fine lazily (the consumer `break`s out) but the eager lowering
# materializes forever — a HANG. Any `while` is refused (boundedness is not
# decidable syntactically). Must refuse at the generator transform.
def naturals() -> int:
    i: int = 0
    while True:
        yield i
        i = i + 1


def entry() -> int:
    total: int = 0
    for x in naturals():
        if x > 3:
            break
        total = total + x
    return total
