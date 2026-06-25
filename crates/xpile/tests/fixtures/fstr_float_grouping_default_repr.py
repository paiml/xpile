# PMAT-982 (correctness-hunt): the BARE thousands-GROUPING spec `:,` / `:_` over
# a float's DEFAULT repr (no `f` presentation) — `f"{1234567.5:,}"` ==
# "1,234,567.5", `f"{1234.5:_}"` == "1_234.5", `f"{1e16:,}"` == "1e+16"
# (a scientific repr has no integer-part digit run to group) — was a clean reject
# ("unsupported format spec `:,` for a F64 value"). Python groups the integer part
# of the `str(float)` repr (NOT a fixed `.Nf` render), sign FIRST for negatives,
# leaving the fractional tail (`.5`) / scientific tail (`e+16`) / `inf`/`nan`
# untouched. Rust's format! has no grouping flag, so the spec routes to the same
# FloatGroupedStr node with `precision: None` — codegen renders the CPython float
# repr (the shared `str(float)` block) then groups the leading digit run. The
# default-repr follow-up to PMAT-940's fixed-precision grouping. vs python3.


def grp_comma(x: float) -> str:
    return f"{x:,}"


def grp_under(x: float) -> str:
    return f"{x:_}"


def labeled(x: float) -> str:
    return f"bal={x:,}!"
