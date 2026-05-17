# BigInt bitwise + shift + power (PMAT-026 / PMAT-013-FOLLOWUP).
#
# Exercises `&`, `|`, `^`, `<<`, `>>`, `**` in BigInt mode by
# annotating params + return as BigInt. The implicit-promotion path
# (PMAT-013) lifts `int`-typed params to BigInt when the return is
# BigInt.
#
# `((a & b) | (a ^ b)) << 2 >> 1 + a ** 2`
#
# In Python: arithmetic on int is unbounded. In xpile-Rust with
# `-> BigInt`, the emission uses `xpile_bigint::BigInt` end-to-end
# with `xpile_bigint::{shl, shr, pow}` helpers and infix `& | ^`.
def bigbits(a: int, b: int) -> BigInt:
    return ((a & b) | (a ^ b)) << 2 >> 1
