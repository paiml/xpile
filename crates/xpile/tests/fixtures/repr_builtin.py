def rs(s: str) -> str:
    # repr(str) adds Python-style quotes + escapes (was rejected / emitted a
    # bare `repr(...)` call → rustc E0423).
    return repr(s)


def ri(n: int) -> str:
    # repr(int) == str(int).
    return repr(n)


def rf(x: float) -> str:
    # repr(float) == str(float) (whole-valued keeps `.0`).
    return repr(x)
