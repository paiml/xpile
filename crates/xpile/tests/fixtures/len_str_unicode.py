def slen(s: str) -> int:
    # Python len(str) counts Unicode code points, not UTF-8 bytes.
    return len(s)


def lit_len() -> int:
    return len("αβγδ")


def list_len(xs: list[int]) -> int:
    # len() of a list is unchanged (element count).
    return len(xs)


def dict_len(d: dict[str, int]) -> int:
    return len(d)


def len_in_expr(s: str) -> int:
    return len(s) * 2 + 1


def empty_str() -> int:
    return len("")
