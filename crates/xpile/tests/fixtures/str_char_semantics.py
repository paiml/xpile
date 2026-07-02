# PMAT-1032: CHAR-oriented string semantics on the WASM lane — the sweep-#11
# non-ASCII divergence cluster (finding 2). Before this slice the WASM str
# runtime was BYTE-oriented: len("héllo") read 6, `for ch in s` stepped bytes
# (iterating a 2-byte char twice and trapping in ord), s[-4] trapped instead
# of indexing from the end, and chr(20013) truncated to a lone (invalid) byte.
# Both lanes must execute this == CPython 902:
#   sum(ord(ch) for ch in "héllo") = 104+233+108+108+111 = 664
#   ord(s[-4]) = ord("é") = 233     (negative index + 2-byte char at once)
#   chr(20013) == "中" -> + len(s) = 5
#   664 + 233 + 5 = 902
def run() -> int:
    s: str = "héllo"
    total: int = 0
    for ch in s:
        total = total + ord(ch)
    total = total + ord(s[-4])
    c: str = chr(20013)
    if c == "中":
        total = total + len(s)
    return total
