# PMAT-1079 (correctness, differential hunt): a variable assigned from a
# loop-local SUBSCRIPT (`job = parts[2]`, parts a list[str] from split) and
# reused as a for-target in a later loop was hoisted to function scope typed
# I64 (default) instead of str — `parts` (not read after the loop) was never
# registered, so the subscript probe fell to the default (rustc E0308). Fix:
# register each body-local's inferred type as SCRATCH during the pre-declare
# scan so later assignments resolve. Found by a realistic CSV-grouping program.
def group_by_last(rows: list[str]) -> str:
    groups: dict[str, list[str]] = {}
    for row in rows:
        parts = row.split(",")
        job = parts[2]
        if job not in groups:
            groups[job] = []
        groups[job].append(parts[0])
    out: str = ""
    for job in sorted(groups.keys()):
        out = out + job + ":" + groups[job][0] + ";"
    return out
