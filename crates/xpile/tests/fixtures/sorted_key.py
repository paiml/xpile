# PMAT-502z (Tranche 2): sorted(xs, key=lambda p: e) -> sort_by_key.
# First lambda/closure support (bounded to the key= position).
def by_len(words: list[str]) -> list[str]:
    return sorted(words, key=lambda w: len(w))


def by_neg(xs: list[int]) -> list[int]:
    return sorted(xs, key=lambda x: -x)


def by_len_desc(words: list[str]) -> list[str]:
    return sorted(words, key=lambda w: len(w), reverse=True)
