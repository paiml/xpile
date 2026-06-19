# PMAT-829 (HUNT-V25 #4): a bool subscript key into an int-keyed dict — d[True] = v
# — emitted d.insert(true, ...) into a HashMap<i64,_> → rustc E0308. Python True==1
# (and hash(True)==hash(1)), so the key coerces to 1. The set path now coerces a
# bool key to i64, mirroring the shipped get-path (PMAT-751). Cross-checked vs python3.


def probe() -> int:
    d: dict[int, int] = {1: 10}
    d[True] = 99
    return len(d) * 100 + d[1]
