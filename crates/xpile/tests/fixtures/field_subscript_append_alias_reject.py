# PMAT-1052 guard (paired with PMAT-1046): appending a LOCAL into a container
# and then mutating an element of that container in place through a subscript
# (`g.rows.append(row); g.rows[0].append(9)`) reaches the shared object the
# value model cloned — Python shows row == [1, 9], xpile would show [1]
# (silent divergence). The container-embed guard in reject_append_then_mutate
# now REFUSES this (the field-subscript-append capability re-enabled the
# shape; the guard keeps it safe).
class Grid:
    rows: list[list[int]]

    def __init__(self) -> None:
        self.rows = []


def main() -> None:
    row: list[int] = [1]
    g: Grid = Grid()
    g.rows.append(row)
    g.rows[0].append(9)
    print(row)
