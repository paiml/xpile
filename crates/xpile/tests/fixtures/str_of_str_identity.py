# PMAT-779 (HUNT-V17 #19): str() over a str-typed value fell through to a
# generic call that inferred I64 / emitted a bare `str(...)` free call (rustc
# E0425); Python `str(s)` is the identity. The str() builtin now returns the
# value unchanged for a str arg (mirroring the existing format(s) identity).
# Cross-checked vs python3.


def echo(s: str) -> str:
    return str(s)


def echo_concat(s: str) -> str:
    return str(s) + "!"


def echo_len(s: str) -> int:
    return len(str(s))
