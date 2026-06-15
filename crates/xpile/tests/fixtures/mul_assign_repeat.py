# PMAT-629: `s *= n` (str) and `xs *= n` (list) are REPETITION, not numeric
# multiplication. `s *= n` emitted String::checked_mul (E0599); `xs *= n` was
# rejected ("only +="). Both now route to Expr::Repeat (like `s * n` / `xs * n`).
# An int `x *= 3` is unchanged (numeric checked_mul).
def rep_str(s: str, n: int) -> str:
    s *= n
    return s


def rep_list(xs: list[int], n: int) -> list[int]:
    xs *= n
    return xs


def int_mul(x: int) -> int:
    x *= 3
    return x
