# PMAT-502ao (Tranche 2): assert cond, msg -> assert!(cond, "{}", msg).
def checked(x: int) -> int:
    assert x > 0, "x must be positive"
    return x


def bare(x: int) -> int:
    assert x > 0
    return x
