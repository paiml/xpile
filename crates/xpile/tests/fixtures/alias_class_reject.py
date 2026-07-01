# PMAT-1020: transitive alias-CLASS analysis — per-hop dispositions do not
# compose (`b = a` clone + `c = b` move severed the Python sharing SILENTLY,
# sweep-9 c0: rust 3 vs cpython 4). The union-find pass catches the chain and
# every other binding form (element reads, ctor captures, if-arm/loop
# aliases, param-returning calls incl. `return xs[0]` interior returns).
def main() -> int:
    a: list[int] = [1, 2, 3]
    b = a
    c = b
    c.append(4)
    return len(a)
