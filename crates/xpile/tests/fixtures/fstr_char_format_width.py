# PMAT-946 (correctness-hunt): the WIDTH/ALIGN-combined `:c` char-format spec that
# PMAT-945 scoped out — f"{65:>5c}" == "    A", f"{65:<5c}" == "A    ",
# f"{67:.^7c}" == "...C...", f"{65:5c}" == "    A" (bare width — `c` is an INTEGER
# presentation type, so it defaults to RIGHT-align, unlike a str). `Expr::Chr`
# already lowers to a single-char `String`, so the spec routes Chr through a
# `FormatSpec` carrying the verbatim `[fill][align][width]` prefix (Rust's string
# fill/align is char-count-based and shares Python's center even-pad split). A bare
# width starting with `0` (implicit zero-pad `:05c`) stays a reject; an explicit
# `0`-fill (`:0>5c`) works via the prefix. The shared format() builtin gets it too.
# vs python3.


def cc_right(n: int) -> str:
    return f"{n:>5c}"


def cc_left(n: int) -> str:
    return f"{n:<5c}"


def cc_center(n: int) -> str:
    return f"{n:^7c}"


def cc_bare(n: int) -> str:
    return f"{n:5c}"


def cc_fill_r(n: int) -> str:
    return f"{n:*>5c}"


def cc_fill_l(n: int) -> str:
    return f"{n:*<5c}"


def cc_fill_c(n: int) -> str:
    return f"{n:.^7c}"


def cc_zerofill(n: int) -> str:
    return f"{n:0>5c}"


def cc_emoji() -> str:
    return f"{0x1F600:>3c}"


def cc_euro() -> str:
    return f"{8364:>4c}"


def cc_han() -> str:
    return f"{0x4E2D:>5c}"


def cc_bool(b: bool) -> str:
    return f"{b:>3c}"


def cc_fmt(n: int) -> str:
    return format(n, ">4c")


def cc_multi(a: int, b: int) -> str:
    return f"[{a:>3c}|{b:<3c}]"
