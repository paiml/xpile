# PMAT-797 (HUNT-V19 ND-01): a value-returning mutating method on a dict-subscript
# receiver (d[k].pop()) cloned the value — the dict read lowers to
# .get(&k).cloned(), so the pop hit a throwaway clone and the stored list kept
# its length (silent-wrong: len(d[k]) unchanged). The ListPop codegen now reaches
# the value mutably via get_mut(&k) when the receiver is a dict subscript.
# Cross-checked vs python3. (The bare-statement form d[k].pop() is a separate
# unsupported case — ND-05 — so this fixture uses the value-returning form.)


def pop_mutates() -> int:
    d: dict[str, list[int]] = {"a": [1, 2, 3]}
    x: int = d["a"].pop()
    return x * 10 + len(d["a"])


def pop_twice() -> int:
    d: dict[str, list[int]] = {"k": [5, 6, 7, 8]}
    a: int = d["k"].pop()
    b: int = d["k"].pop()
    return a + b + len(d["k"])
