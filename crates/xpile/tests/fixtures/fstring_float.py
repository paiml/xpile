def value_line(x: float) -> str:
    # a float in an f-string must render Python repr ("3.0"), not Rust's "3".
    return f"v={x}"


def lone_float(x: float) -> str:
    return f"{x}"


def two_floats(a: float, b: float) -> str:
    return f"{a}+{b}"


def sum_field(a: float, b: float) -> str:
    # the field expression is a float (an arithmetic result).
    return f"sum={a + b}"


def with_precision(x: float) -> str:
    # an explicit `.2f` spec still works alongside the plain-field path.
    return f"{x:.2f}"
