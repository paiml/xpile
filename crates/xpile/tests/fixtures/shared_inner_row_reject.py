# PMAT-1008-interim reject: `[[0, 0]] * 2` replicates ONE shared inner list in
# Python — `m[0][0] = 5` is visible in every replica (CPython 10); xpile's
# per-replica clone gave 5: a confirmed SILENT miscompile (PMAT-1007 case c).
# Refused when the bound name is NESTED-mutated (depth>=2 write); read-only
# grids and depth-1 slot replacement stay accepted (companion fixture).
def main() -> int:
    m = [[0, 0]] * 2
    m[0][0] = 5
    return m[0][0] + m[1][0]
