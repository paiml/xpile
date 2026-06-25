# PMAT-939 (correctness-hunt): the thousands-GROUPING format spec — `,` and `_`
# (f"{1000000:,}" == "1,000,000", f"{1000000:_}" == "1_000_000") were a clean
# reject ("unsupported format spec `:,`"). Python groups the magnitude's decimal
# digits by 3 from the right with the separator, sign FIRST for negatives
# (f"{-1234567:,}" == "-1,234,567"); a bool formats as its int (1). Rust's
# format! has no grouping flag, so the spec routes to the new IntGroupedStr
# digit-grouping loop. vs python3.


def grp_comma(n: int) -> str:
    return f"{n:,}"


def grp_under(n: int) -> str:
    return f"{n:_}"


def labeled(n: int) -> str:
    return f"total={n:,}!"
