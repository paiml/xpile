# PMAT-969 (correctness-hunt): the IMPLICIT zero-pad `:08.2e` on a float is the
# scoped-out form of the width/align scientific lift — Python zero-pads a numeric
# AFTER the sign (`f"{-1234.5:08.2e}"` == "-1.23e+03" with the zeros between the
# sign and the magnitude), which a string-pad over the rendered repr cannot
# reproduce. It (and a sign-forcing `:+e`) stay clean REJECTS — honest refusal,
# never a silent miscompile — the same scoping discipline as PMAT-946's implicit
# zero-pad `:05c` reject. vs python3.


def bad(x: float) -> str:
    return f"{x:08.2e}"
