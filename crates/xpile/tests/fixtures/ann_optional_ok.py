# PMAT-602: the correct forms still transpile — a 2-arg d.get(k, default) infers
# a non-Optional value, so a non-Optional annotation is sound.
def with_default(d: dict[str, int]) -> int:
    x: int = d.get("a", 0)
    return x
