# PMAT-969 (correctness-hunt): the WIDTH/ALIGN-combined scientific (`:12.2e`,
# `:>12.2e`, `: ^14.3e`, `:0>12.2e`) and general (`:14g`, `:>14g`, `:^16.3g`)
# float format specs that PMAT-941 (FloatSciStr) and PMAT-965 (FloatGeneralStr)
# scoped out — f"{1234.5:12.2e}" == "    1.23e+03", f"{x:<12.2e}" == "1.23e+03  ",
# f"{1234.5:14g}" == "        1234.5", f"{123456789:14g}" == "   1.23457e+08" —
# were a clean reject ("unsupported format spec `:12.2e`").
#
# The `e`/`g` value is rendered to its Python string FIRST by the existing
# FloatSciStr / FloatGeneralStr lowering exactly as for the bare spec; this slice
# pads THAT rendered string by peeling an optional `[fill][align][width]` prefix
# off the front of the spec and routing the rendered node through a FormatSpec
# carrying the verbatim prefix. Rust's string fill/align is char-count-based and
# shares Python's even-pad center split, and an `e`/`g` render is pure ASCII, so
# it reproduces Python byte-for-byte. A bare width DEFAULTS to RIGHT-align (a
# numeric value, unlike a str); an explicit `0`-fill (`:0>12.2e`) works via the
# fill+align prefix. NO new IR node — reuses FloatSciStr + FloatGeneralStr +
# FormatSpec, so codegen / meta-hir / ruchy / Lean lanes are untouched. The
# implicit `:08.2e` zero-pad and a sign-forcing `:+e` stay clean rejects
# (honest refusal, deferred). vs python3.


def sci(x: float) -> str:
    return f"{x:12.2e}"


def sci_r(x: float) -> str:
    return f"{x:>12.2e}"


def sci_l(x: float) -> str:
    return f"{x:<12.2e}"


def sci_c(x: float) -> str:
    return f"{x:^14.3e}"


def sci_star(x: float) -> str:
    return f"{x:*>14.2e}"


def sci_zero(x: float) -> str:
    return f"{x:0>12.2e}"


def sci_int(n: int) -> str:
    return f"{n:12.2e}"


def gen_w(x: float) -> str:
    return f"{x:14g}"


def gen_r(x: float) -> str:
    return f"{x:>14g}"


def gen_l(x: float) -> str:
    return f"{x:<14g}"


def gen_c(x: float) -> str:
    return f"{x:^16.3g}"


def gen_int_w(n: int) -> str:
    return f"{n:14g}"


def labeled(x: float) -> str:
    return f"[{x:>10.1e}]"
