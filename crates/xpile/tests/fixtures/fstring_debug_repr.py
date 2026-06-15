# PMAT-694: f-string `!r` (repr) / `!s` (str) conversions + the `{x=}` debug form
# (which the parser desugars to `"x=" + {x!r}`). `!r` of a string adds quotes;
# `!s` matches the plain field. `!a` (ascii) is still declined.
def dbg_int(x: int) -> str:
    return f"{x=}"


def dbg_str(s: str) -> str:
    return f"{s=}"


def dbg_float(f: float) -> str:
    return f"{f=}"


def dbg_bool(b: bool) -> str:
    return f"{b=} done"


def expl_r(s: str) -> str:
    return f"{s!r}"


def expl_s(s: str) -> str:
    return f"{s!s}"


def r_with_spec(s: str) -> str:
    return f"[{s!r:>10}]"
