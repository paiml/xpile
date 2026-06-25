# PMAT-945 (correctness-hunt): the `:c` char-format spec on an INTEGER f-string
# field — f"{65:c}" == "A", f"{0x1F600:c}" == "😀", f"{8364:c}" == "€" — was a
# clean reject ("unsupported format spec `:c`"). Python's `int.__format__` for
# code `c` is exactly `chr(n)`: the single character at Unicode code point n. Rust
# already lowers `chr(n)` to `char::from_u32(n as u32).expect(..).to_string()`, so
# the spec routes to the existing `Expr::Chr` node — NO new IR. A bool delegates
# to int (f"{True:c}" == chr(1)); chr(0) is the NUL char. The shared format()
# builtin gets it too. A float/str `:c` stays a reject (Python ValueError), as
# does a width/align-combined `:c` (scoped follow-up). vs python3.


def cc(n: int) -> str:
    return f"{n:c}"


def cc_lit() -> str:
    return f"{65:c}"


def cc_hex() -> str:
    return f"{0x1F600:c}"


def cc_euro() -> str:
    return f"{8364:c}"


def cc_fmt(n: int) -> str:
    return format(n, "c")


def cc_bool(b: bool) -> str:
    return f"{b:c}"


def cc_zero() -> str:
    return f"{0:c}"


def cc_multi(a: int, b: int) -> str:
    return f"go {a:c}{b:c}!"
