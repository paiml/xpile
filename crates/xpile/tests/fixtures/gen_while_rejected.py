# PMAT-1083 (skeptic pass PMAT-1081, probe p2b-infinite): a `while True`
# generator is fine lazily (the consumer `break`s out) but the eager lowering
# materializes forever — a HANG. Any `while` is refused (boundedness is not
# decidable syntactically). Must refuse at the generator transform.
def naturals() -> int:
    i: int = 0
    while True:
        yield i
        i = i + 1


# NOTE (PMAT-1093): the driver consumes FULLY (sum) so the body-level `while`
# net is what fires — a partial (`break`) consumer now refuses earlier at the
# consumer-side partial-consumption net.
def entry() -> int:
    return sum(naturals())
