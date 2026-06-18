# PMAT-780: an int-declared nested def with a bool body must REJECT (was
# silently lowered to a bool-bodied closure → str(f(5)) = "true" vs "True").
def outer() -> str:
    def f(x: int) -> int:
        return x > 0

    return str(f(5))
