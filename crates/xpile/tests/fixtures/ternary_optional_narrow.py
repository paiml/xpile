# PMAT-818 (HUNT-V19): the `x if x is not None else default` Optional-fallback
# idiom was rejected — the `is not None` branch of a TERNARY didn't narrow `x`
# to its inner T (it does for `if` statements), so the then-branch stayed
# Optional[int] and mismatched the else's int. The ternary now narrows the
# named Optional in the then-branch (cloned ctx → reads unwrap to T).
# Cross-checked vs python3.


def fallback(d: dict[str, int], k: str) -> int:
    v = d.get(k)
    return v if v is not None else 0


def fallback_expr(d: dict[str, int], k: str) -> int:
    v = d.get(k)
    return v + 100 if v is not None else -1
