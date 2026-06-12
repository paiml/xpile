# PMAT-502bh (Tranche 2): str.format with sequential {} placeholders.
def one(x: int) -> str:
    return "val={}".format(x)


def two(a: int, b: int) -> str:
    return "{} + {} done".format(a, b)


def with_str(name: str, n: int) -> str:
    return "{}: {}".format(name, n)


def escaped(x: int) -> str:
    return "{{literal}} {}".format(x)
