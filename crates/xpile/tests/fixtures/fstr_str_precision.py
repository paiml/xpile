# PMAT-947 (correctness-hunt): a bare precision `.N` on a STR f-string field
# TRUNCATES to N chars in Python — f"{'hello':.3}" == "hel", f"{s:.0}" == "",
# f"{s:.10}" == "hello" (precision is a MAX, not a pad). This was a clean reject
# ("unsupported format spec `:.3`"). Rust's `{:.N}` over a `String` is the
# IDENTICAL char-count truncation (multibyte/astral counted by char, never by
# byte), so the spec passes through verbatim — NO new IR, the existing FormatSpec
# node carries it (with of_float=false so the float NaN-guard is skipped). The
# shared str.format() path gets it too. An int/float `.N` (Python *significant
# figures*, not Rust decimal places — Python itself raises ValueError for int)
# stays a reject, as do width/align-COMBINED str precision specs (`:10.3`,
# `:>10.3`, scoped follow-up). vs python3.


def trunc(s: str) -> str:
    return f"{s:.3}"


def empty(s: str) -> str:
    return f"{s:.0}"


def wide(s: str) -> str:
    return f"{s:.10}"


def lit() -> str:
    return f"{'hello':.3}"


def multibyte(s: str) -> str:
    return f"{s:.3}"


def via_format(s: str) -> str:
    return "{:.4}".format(s)


def multi(a: str, b: str) -> str:
    return f"[{a:.2}|{b:.2}]"
