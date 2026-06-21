# PMAT-869 (HUNT-V31 #1): Python's True/False are an int subtype, so a bool in an
# int-expecting position must widen to i64. xpile emitted a bare bool against an
# i64 slot (rustc E0308). Now handled at the two central coercion chokepoints —
# lower_value_expecting (explicit `-> int` return, int-annotated local) and
# coerce_lowered_to_optional (call arg, parameter default) — made safe by also
# inferring an UNANNOTATED comparison return as bool (so `le` below stays `-> bool`
# and is NOT corrupted). Cross-checked vs python3.


def ret_compare(x: int) -> int:
    return x > 0


def takes_int(n: int) -> int:
    return n + 10


def via_call() -> int:
    return takes_int(True)


def with_default(n: int = True) -> int:
    return n


def use_default() -> int:
    return with_default()


def le(a, b):
    return a <= b


def uses_le() -> bool:
    return le(3, 5)
