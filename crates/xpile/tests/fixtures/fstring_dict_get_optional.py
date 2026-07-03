# PMAT-1168: a no-default d.get(k) in an f-string field is Option<T>. Python
# f"{d.get(k)}" == str(d.get(k)), which renders "None" for the absent key and the
# value otherwise. This now transpiles to `if opt.is_none() { "None" } else {
# str(inner) }` — parity with the str() builtin (PMAT-857). Supersedes PMAT-620's
# blanket reject (which was stale once str(d.get(k)) started rendering). An
# Optional *variable* is covered by the same path.
def f(d: dict[str, int], k: str) -> str:
    return f"val={d.get(k)}"


def opt_var() -> str:
    x = None
    return f"x={x}"
