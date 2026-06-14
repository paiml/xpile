# PMAT-615: augmented set assignment (s -= / |= / &= / ^= other). Previously fell
# through to a numeric/bitwise BinOp → HashSet::checked_sub (E0599) for -= and
# owned-value |/&/^ on HashSet (E0369) for the others: transpile succeeded but
# emitted invalid Rust. Now reuses the binop SetOp path (difference / union /
# intersection / symmetric_difference), like the non-augmented `s - other`.
def aug_sub(xs: list[int], ys: list[int]) -> list[int]:
    s: set[int] = set(xs)
    s -= set(ys)
    return sorted(s)


def aug_or(xs: list[int], ys: list[int]) -> list[int]:
    s: set[int] = set(xs)
    s |= set(ys)
    return sorted(s)


def aug_and(xs: list[int], ys: list[int]) -> list[int]:
    s: set[int] = set(xs)
    s &= set(ys)
    return sorted(s)


def aug_xor(xs: list[int], ys: list[int]) -> list[int]:
    s: set[int] = set(xs)
    s ^= set(ys)
    return sorted(s)
