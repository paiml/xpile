def ord_first(s: str) -> int:
    return ord(s[0])


def ord_arith(s: str) -> int:
    return ord(s[0]) - ord("a")


def ord_var(c: str) -> int:
    return ord(c)


def ord_reuse(s: str) -> int:
    x = ord(s[0])
    return x + len(s)
