# PMAT-658: format-spec fill character before an alignment (`{:->10}`,
# `{:*<8}`, `{:.^9}`). Python lets any char be a fill when it precedes an align;
# the translator dropped a `-` fill (mistaken for a sign flag → space padding)
# and rejected `*`/`.` fills. Rust uses the identical `{:fill<width}` syntax.


def fill_dash() -> str:
    return "{:->10}".format("ab")


def fill_star_left() -> str:
    return "{:*<8}".format("hi")


def fill_caret_center() -> str:
    return "{:.^9}".format("xy")


def fill_dash_fstring() -> str:
    s = "ab"
    return f"{s:->10}"


def int_fill() -> str:
    return "{:*>6}".format(42)


def width_no_fill_regression() -> str:
    return "{:>6}".format("zz")
