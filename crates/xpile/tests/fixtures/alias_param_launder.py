# PMAT-1020: an intra-function alias of a PARAM no longer launders the
# PMAT-884 caller guard — `ys = xs; ys.append(9)` attributes the mutation to
# the local, so the param never marked mut and the caller's clone silently
# dropped the append (sweep-9 c1: rust 2/1 vs cpython 2/2). The class pass
# marks a param in a mutated class mutable, so the caller-side guard fires.
def grow(xs: list[int]) -> int:
    ys = xs
    ys.append(9)
    return len(ys)


def main() -> int:
    a: list[int] = [1]
    n = grow(a)
    return n + len(a)
