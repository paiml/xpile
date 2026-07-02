# PMAT-1046 (sweep #12): `container.append(local); local.<mutate>()` — Python
# appends a REFERENCE, so the later mutation shows through the container
# (grid[0] == [1,2,3]); xpile clones the appended value (needed so a read-only
# reused local survives) which SILENTLY DROPS the mutation (grid[0] == [1,2] —
# a DIVERGE). Now REFUSED. Position-sensitive: the build-then-append idiom
# (mutate BEFORE the embed) stays valid — see the loop_body_local / oop
# fixtures that still pass.
class Grid:
    rows: list[list[int]]

    def __init__(self) -> None:
        self.rows = []


def main() -> None:
    row: list[int] = [1, 2]
    g: Grid = Grid()
    g.rows.append(row)
    row.append(3)
    print(g.rows)
