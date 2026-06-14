def reverse(s: str) -> str:
    # the s[::-1] reverse-slice idiom over a str (mirrors xs[::-1] for lists).
    return s[::-1]


def is_palindrome(s: str) -> bool:
    # reverse-slice used inside a larger expression.
    return s == s[::-1]


def reverse_upper(s: str) -> str:
    # compose with another string method.
    return s.upper()[::-1]
