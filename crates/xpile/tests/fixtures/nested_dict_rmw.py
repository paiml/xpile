# PMAT-833 (HUNT-V26 #3): a nested-subscript read-modify-write where the RHS reads
# the same container (d["a"]["x"] = d["a"]["x"] + 5) emitted the RHS while the
# `&mut d` borrow was live → rustc E0502. The RHS is now hoisted into a temp
# before `&mut d` (mirrors the single-level/nested-list paths). Cross-checked vs python3.


def nested_rmw() -> int:
    d: dict[str, dict[str, int]] = {"a": {"x": 1}}
    d["a"]["x"] = d["a"]["x"] + 5
    return d["a"]["x"]


def loop_accum() -> int:
    grid: dict[str, dict[str, int]] = {"a": {"x": 0}}
    for _ in range(3):
        grid["a"]["x"] = grid["a"]["x"] + 1
    return grid["a"]["x"]


def dict_of_list() -> int:
    d: dict[str, list[int]] = {"a": [1, 2, 3]}
    d["a"][0] = d["a"][0] + 100
    return d["a"][0]
