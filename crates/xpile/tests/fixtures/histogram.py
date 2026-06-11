# PMAT-466 / v0.2.0 Track 1.C operations: the full dict[K, V] read +
# write API on top of the v0.1.1 dict-literal foundation. Exercises
# every operation added in this release:
#   - annotated empty literal `counts: dict[int, int] = {}`
#     → Rust `HashMap::new()` (annotation supplies K/V)
#   - Stmt::DictSet      `d[k] = v`            → `d.insert(k, v)`
#   - Expr::DictGetOr    `d.get(k, default)`   → `.get(&k).cloned().unwrap_or(default)`
#   - Expr::DictGet      `d[k]` (read)         → `d[&k].clone()`
#   - Expr::DictContains `k in d`              → `d.contains_key(&k)`
#   - Expr::Len on dict  `len(d)`              → `.len() as i64`
#
# Rust + Ruchy emit a real HashMap pipeline that compiles and computes
# correct values (verified by the rustc round-trip in transpile_e2e.rs).
# Lean refuses the dict ops (deferred to the Std.HashMap encoding,
# v0.3.0) — the same posture as Lean list iteration/mutation.
def histogram(xs: list[int]) -> dict[int, int]:
    counts: dict[int, int] = {}
    for x in xs:
        counts[x] = counts.get(x, 0) + 1
    return counts


def lookup(table: dict[int, int], key: int) -> int:
    return table[key]


def has_key(table: dict[int, int], key: int) -> bool:
    return key in table


def size(table: dict[int, int]) -> int:
    return len(table)
