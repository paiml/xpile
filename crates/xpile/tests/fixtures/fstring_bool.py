def flag_line(flag: bool) -> str:
    # bool in an f-string must render Python-style "True"/"False",
    # not Rust's lowercase "true"/"false".
    return f"flag={flag}"


def lone_bool(flag: bool) -> str:
    return f"{flag}"


def two_bools(a: bool, b: bool) -> str:
    return f"{a} and {b}"


def comparison_field(a: int, b: int) -> str:
    # the field expression is a bool (a comparison).
    return f"eq={a == b}"


def mixed(n: int, ok: bool) -> str:
    return f"n={n} ok={ok}"
