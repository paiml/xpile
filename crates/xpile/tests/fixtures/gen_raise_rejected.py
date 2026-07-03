# PMAT-1083 (skeptic pass PMAT-1081, probe p10-gen-lateraise): a `raise`
# after a yield fires lazily at the CONSUMING iteration — a partial consumer
# (`break` after the first item) never reaches it in CPython, but the eager
# lowering raises at CALL time (clean CPython run vs exit-101 crash before
# any output). Must refuse at the generator transform.
def risky(n: int) -> int:
    for i in range(n):
        if i > 0:
            raise ValueError("late")
        yield i


# NOTE (PMAT-1093): the driver consumes FULLY (sum) so the body-level `raise`
# net is what fires — a partial (`break`) consumer now refuses earlier at the
# consumer-side partial-consumption net.
def entry() -> int:
    return sum(risky(5))
