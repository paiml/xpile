# PMAT-854 (HUNT-V28 #11): str.index/rindex/rfind rejected start[/end] args while
# find/count accepted them (asymmetry). They now reuse the same slice+offset path:
# r* search rightmost (rfind), *index raise ValueError where find/rfind return -1.
# Cross-checked vs python3.


def index_start(s: str) -> int:
    return s.index("a", 2)


def rfind_start(s: str) -> int:
    return s.rfind("a", 1)


def index_start_end(s: str) -> int:
    return s.index("a", 1, 5)


def rindex_start(s: str) -> int:
    return s.rindex("a", 2)
