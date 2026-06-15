# PMAT-685: `xs.extend(s)` over a str arg iterates the string's CHARACTERS in
# Python (each a 1-char str). Was emitted as `(s).iter()` → E0599 (String has no
# .iter()). Now converted to its chars list.
def add_chars(xs: list[str], w: str) -> list[str]:
    xs.extend(w)
    return xs


def list_extend_regression(xs: list[int], ys: list[int]) -> list[int]:
    xs.extend(ys)
    return xs
