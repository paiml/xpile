# PMAT-1013 (sweep #7): MUTATION-DURING-ITERATION — the body mutates the very
# list the loop iterates. Python iterates the LIVE list (the appended element
# IS visited; `for x in xs: xs.append(...)` per element runs forever; removals
# shift upcoming elements), semantics xpile's value model cannot express: the
# old emit was an E0502 immutable/mutable borrow conflict (invalid Rust), and
# a snapshot-iterating emit would COMPILE but silently diverge. Clean-refused
# at lowering (the PMAT-884 alias-then-mutate posture). Workaround (verified):
# iterate a copy — `for x in xs[:]` — or collect changes and apply after.
def grow(xs: list[int]) -> int:
    total: int = 0
    for x in xs:
        if x == 1:
            xs.append(99)
        total = total + x
    return total
