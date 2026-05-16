# `assert` statement fixture (PMAT-009).
#
# Exercises:
#   - `assert cond` (no message form) inside a function body
#   - cond reads function params, validating the assert lowering doesn't
#     break access to bindings
#
# The Rust emission becomes `assert!(...)`, which panics at runtime on
# false. Driver passes only inputs that satisfy the precondition.
def safe_div(a: int, b: int) -> int:
    assert b != 0
    assert a >= 0
    return a // b
