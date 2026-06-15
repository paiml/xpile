# PMAT-682: a bare WIDTH on a str (`{:5}`) LEFT-aligns to the width in Python
# (`f"{'ab':5}"` == "ab   "); Rust's `{:5}` over a String is also left-aligned.
# Was rejected ("unsupported format spec :5 for a Str value").
def col(s: str) -> str:
    return f"{s:5}"


def col10(s: str) -> str:
    return f"{s:10}"


def via_format(s: str) -> str:
    return "{:8}".format(s)


def right(s: str) -> str:
    return f"{s:>5}"


def overflow(s: str) -> str:
    return f"{s:3}"
