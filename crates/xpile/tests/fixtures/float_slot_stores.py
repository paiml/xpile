# PMAT-1040: int (and bool) stored into a FLOAT-annotated slot on the
# Name-bottoming subscript-assign paths — `xs[0] = 3` over `list[float]`,
# `d["a"] = 3` over `dict[str, float]`, `g[1][0] = 3` over
# `list[list[float]]` — all emitted the raw int (rustc E0308 INVALID emit,
# found probing PMAT-1037's FieldIndexAssign widen: the field path widened,
# its Name-bottoming siblings didn't). Now widened via to_f64_operand, the
# PMAT-1017/1037 FieldAssign/FieldIndexAssign convention. KNOWN repr edge
# (same class as the shipped param coercion): a DIRECT print of the stored
# slot shows `3.0` where CPython (which stores the int unconverted) prints
# `3`; arithmetic uses are exact. Aug-assign already widened via
# combine_aug. Differentially verified vs CPython (MATCH 4.5/3.25/3.5/2.5).
def list_slot() -> float:
    xs: list[float] = [0.5, 1.5]
    xs[0] = 3
    return xs[0] + xs[1]


def dict_slot() -> float:
    d: dict[str, float] = {"a": 0.5}
    d["a"] = 3
    return d["a"] + 0.25


def nested_slot() -> float:
    g: list[list[float]] = [[0.5], [1.5]]
    g[1][0] = 3
    return g[1][0] + g[0][0]


def bool_slot() -> float:
    xs: list[float] = [0.5]
    xs[0] = True
    return xs[0] + 1.5
