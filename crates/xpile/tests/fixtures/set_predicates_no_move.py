# PMAT-652: set relational predicates (<=, >=, <, >, issubset/issuperset/
# isdisjoint) must NOT move their operands. They lowered to `let __l = a; ...`
# which moved the set, so reusing a compared set (or self-comparing `a <= a`)
# failed to compile with E0382. The fix binds the operands by reference.


def subset_then_reuse(a: set[int], b: set[int]) -> int:
    # a is compared, then a.len() is read again -> used to be E0382
    flag = 1 if a <= b else 0
    return flag + len(a)


def disjoint_self(a: set[int]) -> int:
    # both operands are the same variable -> used to be E0382
    return 1 if a.isdisjoint(a) else 0


def subset_self(a: set[int]) -> int:
    return 1 if a <= a else 0


def issuperset_then_reuse(a: set[int], b: set[int]) -> int:
    flag = 1 if a.issuperset(b) else 0
    return flag + len(b)
