# PMAT-851 (HUNT-V28 #2): s.index(sep)/find/rindex bound `let __s = (s)`, MOVING a
# non-Copy String, so the everyday `i = s.index(sep); s[i:]` idiom failed rustc
# E0382 (s used after move). The receiver is now cloned (find/rfind/index/rindex
# share the path — all were affected). Cross-checked vs python3.


def index_then_slice() -> str:
    s: str = "key:value"
    i: int = s.index(":")
    return s[i + 1:]


def find_then_reuse() -> int:
    s = "a:b:c"
    i = s.find(":")
    return i + len(s)


def rindex_then_slice() -> str:
    s = "x.y.z"
    i = s.rindex(".")
    return s[:i]
