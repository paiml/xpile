# PMAT-946 (correctness-hunt): an IMPLICIT zero-pad `:05c` (a `0`-leading bare
# width, no explicit alignment) is scoped OUT — Python zero-pads it ("0000A"), but
# xpile defers that form (the EXPLICIT `0`-fill `:0>5c` is supported via the
# fill+align prefix). It must therefore stay a clean reject, an honest refusal
# rather than a silent miscompile. vs python3.


def bad(n: int) -> str:
    return f"{n:05c}"
