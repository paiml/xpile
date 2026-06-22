# V29-1 (PMAT-885): CPython dicts iterate in INSERTION order (guaranteed since
# 3.7). xpile already lowers every dict-producing path to indexmap::IndexMap
# (insertion-ordered) rather than std::collections::HashMap (nondeterministic).
# This fixture pins that order END-TO-END across ALL dict paths using a
# deliberately NON-SORTED insertion order (keys 3, 1, 2) so a HashMap regression
# (which would tend to sort small int keys or reorder nondeterministically)
# falsifies immediately. Cross-checked vs python3:
#   lit_keys   -> "312"        lit_values -> "301020"   lit_items -> "3:30,1:10,2:20,"
#   insert_upd -> "3:30,1:99,2:20,4:40,"
#   comp_keys  -> "312"        merge_kv   -> "3=30,1=99,2=20,"


def lit_keys() -> str:
    # dict LITERAL with keys inserted 3,1,2 — .keys() must iterate 3,1,2.
    d: dict[int, int] = {3: 30, 1: 10, 2: 20}
    out: str = ""
    for k in d.keys():
        out = out + str(k)
    return out


def lit_values() -> str:
    # .values() must follow the same insertion order: 30,10,20.
    d: dict[int, int] = {3: 30, 1: 10, 2: 20}
    out: str = ""
    for v in d.values():
        out = out + str(v)
    return out


def lit_items() -> str:
    # .items() must yield (k, v) pairs in insertion order.
    d: dict[int, int] = {3: 30, 1: 10, 2: 20}
    out: str = ""
    for k, v in d.items():
        out = out + str(k) + ":" + str(v) + ","
    return out


def insert_then_update() -> str:
    # Build via inserts 3,1,2; APPEND 4; UPDATE key 1 (keeps its position).
    d: dict[int, int] = {}
    d[3] = 30
    d[1] = 10
    d[2] = 20
    d[4] = 40
    d[1] = 99
    out: str = ""
    for k in d:
        out = out + str(k) + ":" + str(d[k]) + ","
    return out


def comp_keys() -> str:
    # Dict COMPREHENSION preserves the source iterable's order: 3,1,2.
    xs: list[int] = [3, 1, 2]
    c: dict[int, int] = {k: k * 10 for k in xs}
    out: str = ""
    for k in c:
        out = out + str(k)
    return out


def merge_kv() -> str:
    # MERGE {**a, **b}: later value wins, original key position preserved.
    # a = {3,1}; b updates 1 (stays at pos 1) and appends 2 -> order 3,1,2.
    a: dict[int, int] = {3: 30, 1: 10}
    b: dict[int, int] = {1: 99, 2: 20}
    m: dict[int, int] = {**a, **b}
    out: str = ""
    for k in m.keys():
        out = out + str(k) + "=" + str(m[k]) + ","
    return out
