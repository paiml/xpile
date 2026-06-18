# PMAT-778 (HUNT-V17 #6): repr() of a string with non-printable control chars
# pushed them raw (only \\, \n, \r, \t, the quote were escaped) — silent-wrong;
# Python escapes them as \xNN. The repr escaper now emits \xNN for the ASCII
# controls (< 0x20, after the named ones), DEL (0x7f), and the C1 block
# (0x80-0x9f) — a fixed always-non-printable set. Cross-checked vs python3.


def r_ctrl() -> str:
    s: str = "a\r\x00b"
    return repr(s)


def r_esc() -> str:
    s: str = "x\x1b\x07y"
    return repr(s)


def r_plain() -> str:
    s: str = "hello\tworld"
    return repr(s)
