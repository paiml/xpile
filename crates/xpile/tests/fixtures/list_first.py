# PMAT-457 / v0.2.0 Track 1.B: list indexed access `xs[0]`. Exercises
# Expr::Index over a Type::List parameter. The rustc round-trip
# verifies the indexed value matches Python semantics.
def first(xs: list[int]) -> int:
    return xs[0]
