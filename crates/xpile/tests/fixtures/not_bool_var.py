# PMAT-502cc (Tranche 2): context-aware `not <bool var>`. The context-free
# unary lowering infers a bare Ident as int and so rejected `not b` for a
# bool parameter/local; the ctx-aware arm sees the real Bool type.
def toggle(b: bool) -> bool:
    return not b


def clamp(active: bool, x: int) -> int:
    if not active:
        return 0
    return x
