# PMAT-834 (HUNT-V26 #7): the zero-arg builtin constructors int()/str()/float()
# are Python's defaults 0/""/0.0, but emitted bare int()/str()/float() free calls
# (rustc E0425) mis-typed as i64. They now lower to the default literals.
# Cross-checked vs python3.


def probe() -> int:
    n = int()
    s = str()
    for c in "abc":
        s = s + c
    f = float()
    return n + len(s) + int(f + 4.0)
