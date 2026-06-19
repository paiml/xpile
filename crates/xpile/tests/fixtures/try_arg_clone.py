# PMAT-827 (HUNT-V25 #2): a try body that passes a non-Copy arg to a helper and
# reuses that arg in the except handler — try: return parse_it(s) except: return
# len(s) — moved s into the catch_unwind closure, then the handler read the moved
# s → rustc E0382. count_name_reads now descends into try/except, so s is seen as
# reused and the body call clones it. Cross-checked vs python3.


def parse_it(s: str) -> int:
    return int(s)


def probe(s: str) -> int:
    try:
        return parse_it(s)
    except ValueError:
        return len(s)
