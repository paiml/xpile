# PMAT-847 (HUNT-V27 #12): a for-loop over a set or a tuple was clean-rejected
# ("other iterables are deferred") though both are everyday Python iterables. A
# set iterates its elements (HashSet .iter().cloned(), like a Vec); a homogeneous
# tuple materializes to a list of its elements. Cross-checked vs python3.
# (A comprehension over a set/tuple is a separate path — sibling follow-up.)


def over_set(s: set[int]) -> int:
    total = 0
    for x in s:
        total += x
    return total


def over_tuple() -> int:
    total = 0
    for x in (10, 20, 30):
        total += x
    return total


def over_str_tuple() -> str:
    out = ""
    for w in ("a", "bb", "ccc"):
        out = out + w
    return out
