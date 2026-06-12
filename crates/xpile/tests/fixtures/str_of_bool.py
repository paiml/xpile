# PMAT-502ae (Tranche 2): str(b) over a bool -> Python's "True"/"False"
# (capitalized), via a desugar to `"True" if b else "False"`.
def flag_str(b: bool) -> str:
    return str(b)


def cmp_str(a: int, c: int) -> str:
    return str(a < c)


def labeled(b: bool) -> str:
    return "flag=" + str(b)
