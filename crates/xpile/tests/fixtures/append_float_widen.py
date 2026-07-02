# PMAT-1047 (sweep #12): int/bool appended into a FLOAT-typed list slot —
# `xs: list[float]; xs.append(3)` (ListAppend) and `d[k].append(3)` over
# `dict[str, list[float]]` (IndexAppend) emitted the raw int (rustc E0308).
# Both paths now widen via to_f64_operand, mirroring the subscript-slot
# (PMAT-1040) and field-append (PMAT-1037 slice D) widening. Verified vs CPython.
def plain_append() -> float:
    xs: list[float] = [1.5]
    xs.append(3)
    return xs[0] + xs[1]


def dict_inner_append() -> float:
    d: dict[str, list[float]] = {}
    d["a"] = []
    d["a"].append(2)
    d["a"].append(0.5)
    return d["a"][0] + d["a"][1]
