# PMAT-755 (HUNT-V15 #2 AUG-1): an augmented subscript assignment with a
# SIDE-EFFECTING index/key double-evaluated it — the index was emitted once for
# the write target and again inside the implicit current-value read, so a
# `.pop()`/stateful index ran twice (wrong slot, silent-wrong). It also missed
# marking the index's pop-receiver `mut` (E0596). The index/key is now bound to
# one temp and reused; the aug-target is scanned for pop receivers. A pure index
# (`i + 1`) is unchanged (no temp). Cross-checked vs python3.


def list_idx() -> int:
    q = [0, 2, 2]
    xs = [10, 20, 30]
    xs[q.pop(0)] += 100  # one pop → index 0 → xs[0] = 110
    return xs[0]


def dict_key() -> int:
    keys = ["a", "b"]
    d = {"a": 1, "b": 2}
    d[keys.pop(0)] += 100  # one pop → "a" → d["a"] = 101
    return d["a"]


def pure_idx(i: int) -> int:
    # a pure index is duplicated (no temp) — unchanged behavior
    xs = [10, 20, 30]
    xs[i + 1] += 5
    return xs[1]
