# PMAT-858 (HUNT-V29 #3): `expr or default` / `expr and y` returning the operand
# by truthiness previously required every LEADING operand to be a bare name (safe
# to evaluate twice). That rejected the very common `d.get(k, 0) or default` /
# `s.strip() or default` (a method/call lead). A non-name lead is now bound to a
# typed temp ONCE (single eval + short-circuit, matching Python). vs python3.


def via_call(d: dict[str, int]) -> int:
    return d.get("k", 0) or 99


def via_method(s: str) -> str:
    return s.strip() or "empty"


def via_name(x: int, y: int) -> int:
    return x or y
