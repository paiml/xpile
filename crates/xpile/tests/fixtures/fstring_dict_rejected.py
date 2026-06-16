# PMAT-708: a bare dict (or set) interpolated in an f-string emitted
# `format!("{}", hashmap)` → E0277 (HashMap/HashSet have no Display). It is now
# rejected at lowering (parity with str()/print()/.format()/% over a dict/set;
# the iteration order is also non-deterministic, PMAT-537).
def f(d: dict[str, int]) -> str:
    return f"d = {d}"
