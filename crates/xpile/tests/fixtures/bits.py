# Bitwise binops fixture (PMAT-003).
#
# Exercises &, |, ^, <<, >> in one function so the runtime-verified
# round-trip pins the semantics of every Layer-1 `C-PY-INT-ARITH`
# bitwise op in a single shot.
#
# Python:  ((a & b) | (a ^ b)) is `a | b`, but routing through both
#          gives us a useful product. Then `<<` and `>>` apply
#          arithmetic shifts to the result. We pick small literals to
#          keep i64 well inside range.
def bits(a: int, b: int) -> int:
    return ((a & b) | (a ^ b)) << 2 >> 1
