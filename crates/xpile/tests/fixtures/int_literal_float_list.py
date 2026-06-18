# PMAT-782 (HUNT-V17 #11 IFM-2): a list literal declared `list[float]` with int
# literals (`xs: list[float] = [1, 2, 3]`) emitted `vec![1i64, ...]` against the
# Rust `Vec<f64>` slot → rustc E0308. The let-init type-threading now coerces an
# int literal element to a float when the declared element type is float; a
# float element and an int-typed list are unchanged. Cross-checked vs python3.


def weights() -> float:
    xs: list[float] = [1, 2, 3]
    total = 0.0
    for x in xs:
        total = total + x
    return total


def mixed() -> float:
    xs: list[float] = [1, 2.5, 3]
    total = 0.0
    for x in xs:
        total = total + x
    return total


def int_list_unchanged() -> int:
    xs: list[int] = [1, 2, 3]
    total = 0
    for x in xs:
        total = total + x
    return total
