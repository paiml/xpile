# PMAT-502cb (Tranche 2): str.format positional `{N}` placeholders. Rust's
# format! shares the syntax, so the format string is re-emitted verbatim.
# Reorder, repeat, and (separately) automatic `{}` all work.
def swap(a: str, b: str) -> str:
    return "{1} {0}".format(a, b)


def dup(a: int) -> str:
    return "{0}-{0}".format(a)


def seq(a: str, b: str) -> str:
    return "{} and {}".format(a, b)
