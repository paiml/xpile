# Bigint scaffold fixture (PMAT-012).
#
# `BigInt` is xpile-specific annotation that opts a function into the
# slow path of the `C-PY-INT-ARITH` Layer-1 contract — Rust/Ruchy emit
# `xpile_bigint::BigInt` (wrapping num-bigint) instead of i64, so
# arithmetic never overflows. Lean's `Int` is already unbounded, so
# both annotations produce the same Lean output.
#
# This fixture exercises:
#   * BigInt param annotations on both args
#   * BigInt return type
#   * Plain `+` infix arithmetic on BigInt operands (no checked_add)
def big_sum(a: BigInt, b: BigInt) -> BigInt:
    return a + b
