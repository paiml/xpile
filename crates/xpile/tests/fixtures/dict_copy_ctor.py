# PMAT-814 (HUNT-V22 #11 CC-3): dict(d) over an existing dict was rejected
# ("non-(list of 2-tuples)"). Python dict(d) is a fresh, independent copy; it now
# emits an owned clone (mirrors list(xs) -> (xs).clone()). Cross-checked vs python3.


def copy_independent() -> int:
    a = {"x": 1, "y": 2}
    b = dict(a)
    b["z"] = 9
    return len(a) * 100 + len(b)
