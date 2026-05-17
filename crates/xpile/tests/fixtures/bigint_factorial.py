# Implicit BigInt promotion via return-type annotation (PMAT-013).
#
# The user only annotates the return type. Every `int`-typed param is
# auto-promoted to BigInt, and the whole function lowers in BigInt
# mode — recursive multiplication compiles to plain `*` on BigInt,
# never overflows.
#
# This is the canonical case the C-PY-INT-ARITH slow path was always
# pointing at via panic messages: `factorial(n)` for n ~ 20 already
# overflows i64; the BigInt path computes it without panicking.
def factorial(n: int) -> BigInt:
    return 1 if n <= 1 else n * factorial(n - 1)
