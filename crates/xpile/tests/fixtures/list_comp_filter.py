# PMAT-502ay (Tranche 2): filtered list comprehension [elem for v in xs if cond].
def positives(xs: list[int]) -> list[int]:
    return [x for x in xs if x > 0]


def doubled_positives(xs: list[int]) -> list[int]:
    return [x * 2 for x in xs if x > 0]


def assign_form(xs: list[int]) -> int:
    ys = [x for x in xs if x > 5]
    return len(ys)
