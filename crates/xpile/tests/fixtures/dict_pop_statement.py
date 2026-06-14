def remove_key(d: dict[str, int]) -> int:
    # bare `d.pop(k)` statement (discard the removed value).
    d.pop("a")
    return len(d)


def remove_with_default(d: dict[str, int]) -> int:
    # bare `d.pop(k, default)` statement — missing key is tolerated.
    d.pop("missing", 0)
    return len(d)


def drain_two(d: dict[str, int]) -> int:
    # mix bare pop with value-position pop.
    d.pop("a")
    taken = d.pop("b")
    return taken + len(d)
