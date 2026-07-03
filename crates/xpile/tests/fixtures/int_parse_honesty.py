def parse10(s: str) -> int:
    # PMAT-1097: int() parse-failure honesty. A well-formed decimal that
    # overflows i64 panics with the bigint-posture message (CPython returns a
    # bigint); a Unicode-decimal-digit string panics with an honest digit-class
    # refusal (CPython accepts it: int("١٢") == 12); only a genuinely
    # invalid literal claims the exact CPython ValueError message.
    return int(s)


def parse16(s: str) -> int:
    return int(s, 16)


def mod_msg(a: int, b: int) -> str:
    # PMAT-1097: `%` by zero carries the CPython 3.10 (local-oracle) message —
    # the same "integer division or modulo by zero" text as `//` (the old text
    # was the 3.12 "integer modulo by zero" variant, an inconsistent pair).
    try:
        msg = str(a % b)
    except ZeroDivisionError as e:
        msg = "err: " + str(e)
    return msg
