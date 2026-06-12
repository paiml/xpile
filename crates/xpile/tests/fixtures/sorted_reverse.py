# PMAT-502f (Tranche 2): sorted(xs, reverse=True) returns a new list in
# descending order (sort-then-reverse). reverse=False is plain ascending.
def order_desc(xs: list[int]) -> list[int]:
    return sorted(xs, reverse=True)


def order_asc(xs: list[int]) -> list[int]:
    return sorted(xs, reverse=False)
