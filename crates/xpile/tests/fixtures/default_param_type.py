# PMAT-821 (HUNT-V24 #5): a parameter with a default value but NO annotation was
# hardcoded to i64, ignoring the default's type (def greet(name="x") emitted
# name: i64 → rustc E0308 the moment the body used it as a str). The param type
# is now inferred from the default literal (str/bool/int/float, incl. a negated
# numeric) at the function def AND the signature table. Cross-checked vs python3.


def greet(name="world") -> str:
    return "hi " + name


def flag(on=True) -> int:
    return 1 if on else 0


def scale(factor=1.5) -> float:
    return factor * 2.0


def probe() -> str:
    return greet() + " / " + greet("bob")


def flags() -> int:
    return flag() * 10 + flag(False)


def scaled() -> float:
    return scale() + scale(2.0)
