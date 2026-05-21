# PMAT-459 / v0.2.0 Track 1.B: builtin `len(xs)` over a list[int].
# Returns the byte/element count as Python int. Rust emits `.len() as i64`;
# Lean emits `((xs).length : Int)`. Combines with for-each and indexing
# to give the full read-side list API.
def count_xs(xs: list[int]) -> int:
    return len(xs)
