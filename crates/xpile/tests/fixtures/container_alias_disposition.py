# PMAT-1008-interim: the PMAT-1016C three-way alias disposition extended to
# CONTAINERS (list/dict/set) — read-only alias CLONES (was rustc E0382),
# source-dead alias MOVES; mutation-with-both-live refuses (reject fixture).
# The object-mutation set now covers builtin mutators (append/…/setdefault),
# subscript-write bases (any depth), del, and expression-position pop.
def read_only_alias() -> int:
    a = [1, 2, 3]
    b = a
    return len(b) + len(a) + b[0]


def dead_source_alias() -> int:
    a = [1, 2]
    b = a
    b.append(3)
    return len(b)


def dict_read_only() -> int:
    d = {"x": 1}
    e = d
    return e["x"] + d["x"]
