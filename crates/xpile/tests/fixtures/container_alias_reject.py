# PMAT-1008-interim reject: `b = a` then a SUBSCRIPT WRITE through the alias
# with the source still observable — Python shares (a[0] becomes 99); a move
# was rustc E0382 and a clone would SILENTLY print 1. Clean-refused. (The
# subscript write is not a "read": the dead-alias shortcut must require
# never-read AND never-mutated, else this very case slips into the clone.)
def main() -> int:
    a = [1, 2, 3]
    b = a
    b[0] = 99
    return a[0]
