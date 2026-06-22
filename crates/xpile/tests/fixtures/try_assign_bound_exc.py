def safe_div(a: int, b: int) -> str:
    # assignment-form try with `as e` — the exception message binds to a String
    # local usable in the handler (`str(e)`). Was rejected before PMAT-886.
    try:
        msg = str(a // b)
    except ZeroDivisionError as e:
        msg = "err: " + str(e)
    return msg


def parse_or_zero(s: str) -> int:
    try:
        n = int(s)
    except ValueError as e:
        # `e` used inside an f-string in the handler.
        n = len(f"{e}") * 0
    return n
