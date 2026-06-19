# PMAT-846 (HUNT-V27 #6): s.split(None) is an explicit whitespace split (Python
# treats None as "any whitespace run"), identical to the no-arg s.split(). It
# lowered None to &(None)[..] → rustc E0608; it now routes to the same
# whitespace-split path. (s.split(None, n) maxsplit form is deferred.)
# Cross-checked vs python3.


def split_none() -> int:
    s = "  a  b   c "
    return len(s.split(None))


def split_noarg() -> int:
    return len("a b  c".split())


def split_sep() -> int:
    return len("a,b,c".split(","))
