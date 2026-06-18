# PMAT-796 (HUNT-V18 BIC-02): a list/str repeat with a bool count (`[x] * b`)
# fell to the int-multiply path → rustc E0599 (checked_mul on a Vec) / a "body
# produces I64" reject, where Python's bool is an int subtype: [x]*True == [x],
# [x]*False == []. try_repeat now accepts a Bool count and coerces it to i64.
# Cross-checked vs python3.


def rep_true() -> list[int]:
    b = True
    return [1, 2] * b


def rep_false() -> list[int]:
    b = False
    return [7] * b


def str_rep_true() -> str:
    b = True
    return "ab" * b
