# PMAT-502cq (Tranche 2): str.removeprefix(p) / removesuffix(p) (Python 3.9+).
# Map to Rust strip_prefix/strip_suffix, returning the string unchanged when
# the affix is absent.
def strip_pre(s: str) -> str:
    return s.removeprefix("foo_")


def strip_suf(s: str) -> str:
    return s.removesuffix(".txt")
