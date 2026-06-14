# PMAT-597: the standalone format(value[, spec]) builtin (distinct from
# str.format / %-formatting). format(x) == str(x); format(x, "<spec>") applies
# the Python format mini-language (shared with f-string fields). Previously
# format(...) fell through to a generic call emitting a bare `format(...)` —
# but Rust's `format` is a macro, so rustc rejected it (E0423).
def hex_of(n: int) -> str:
    return format(n, "x")


def padded(n: int) -> str:
    return format(n, "05d")


def plain(n: int) -> str:
    return format(n)


def pct(x: float) -> str:
    return format(x, ".1%")
