# PMAT-714: `"{}".format(x)` over a float/bool was rejected ("needs a spec"),
# though f"{x}" and str(x) produce the correct Python repr. The no-spec `{}` now
# wraps a float/bool arg in the f-string display conversion (float → `3.0`/`3.14`,
# bool → `True`/`False`) when the arg is referenced once.
def show_float(x: float) -> str:
    return "val={}".format(x)


def show_bool(b: bool) -> str:
    return "flag={}".format(b)


def whole_float(x: float) -> str:
    return "{}".format(x)


def mixed(n: int, x: float, s: str) -> str:
    return "{} {} {}".format(n, x, s)
