def group(pairs: list[tuple[str, int]]) -> dict[str, list[int]]:
    out: dict[str, list[int]] = {}
    for k, v in pairs:
        out.setdefault(k, []).append(v)
    return out


def group_nonempty(pairs: list[tuple[str, int]]) -> dict[str, list[int]]:
    out: dict[str, list[int]] = {}
    for k, v in pairs:
        out.setdefault(k, [0]).append(v)
    return out
