def max_or_zero(xs: list[int]) -> int:
    # ternary with a list-truthy condition.
    return max(xs) if xs else 0


def first_or_default(xs: list[str]) -> str:
    # statement-position `if <list>:`.
    if xs:
        return xs[0]
    return "none"


def sum_drain(xs: list[int]) -> int:
    # `while <list>:` — loop until the list is empty (value-position pop).
    total = 0
    while xs:
        total += xs.pop()
    return total


def is_empty_dict(d: dict[str, int]) -> bool:
    # `not <dict>` → dict is empty.
    return not d


def has_items(s: str) -> int:
    # `not <str>` (empty string is falsy).
    if not s:
        return 0
    return 1
