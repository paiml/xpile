# PMAT-831 (HUNT-V25 #10): a bool with a NON-EMPTY format spec formats as an int
# in Python (bool.__format__ delegates to int) — f"{True:>7}" is "      1", not
# Rust's "true". A no-spec f"{flag}" keeps "True"/"False". The f-string + str.format
# spec paths now coerce a bool to i64 before applying the spec. Cross-checked vs python3.


def fstring_align() -> str:
    flag = True
    return f"[{flag:>7}]"


def format_center() -> str:
    return "{:^7}".format(False)


def mixed() -> str:
    flag = True
    return f"{flag} {flag:d}"
