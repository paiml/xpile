# PMAT-1048 (sweep #12 t23): a nested list LITERAL appended into a float-of-list
# target — `g: list[list[float]]; g.append([2, 3])` — emitted `vec![2i64, 3i64]`
# (rustc E0308, Vec<i64> vs Vec<f64>). The scalar append widen (PMAT-1047)
# covers `xs.append(3)` but not a list-literal argument whose ELEMENTS need
# widening. coerce_expr_to_type now recurses into list literals element-wise.
# (A DIRECT sum of int-valued float slots prints X.0 vs Python X — the
# documented int-into-float-repr class; the fixture uses non-integer sums.)
def nested_append() -> float:
    g: list[list[float]] = [[1.5]]
    g.append([2, 3])
    return g[1][0] + g[1][1] + g[0][0]


def int_list_untouched() -> int:
    g: list[list[int]] = [[1]]
    g.append([2, 3])
    return g[1][0] + g[1][1]
