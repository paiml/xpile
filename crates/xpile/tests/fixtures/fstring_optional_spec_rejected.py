# PMAT-1168: an Optional field WITH a format spec is refused. Python's
# format(None, ">5") is itself a TypeError, so there is no single correct static
# rendering (present -> the value formatted, absent -> a runtime error). Reject
# cleanly rather than emit uncompilable Rust or a silent divergence. The plain
# f"{d.get(k)}" (no spec) renders fine — see fstring_dict_get_optional.py.
def f(d: dict[str, int], k: str) -> str:
    return f"val={d.get(k):>5}"
