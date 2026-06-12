# PMAT-502t (Tranche 2): the reverse idiom xs[::-1] over a list -> a new
# reversed list (reuses Expr::Reversed; input unchanged).
def rev(xs: list[int]) -> list[int]:
    return xs[::-1]


def rev_strs(words: list[str]) -> list[str]:
    return words[::-1]
