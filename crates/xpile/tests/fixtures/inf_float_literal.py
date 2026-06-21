# PMAT-866 (HUNT-V30 #17): a non-finite float literal (1e400 -> inf, also nan)
# emitted the invalid Rust token `inff64`/`nanf64` (the `{}f64` formatter). It now
# emits the f64 constant. Cross-checked vs python3.


def big() -> float:
    return 1e400


def neg_big() -> float:
    return -1e400


def normal() -> float:
    return 3.14
