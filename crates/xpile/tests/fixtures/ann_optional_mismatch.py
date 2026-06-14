# PMAT-602: a non-Optional annotation over a 1-arg d.get() (which is Optional)
# is a type lie that would emit Option<i64> into an i64 binding (E0308). xpile
# rejects it cleanly; use Optional[int] or d.get(k, default).
def bad(d: dict[str, int]) -> int:
    x: int = d.get("a")
    return x
