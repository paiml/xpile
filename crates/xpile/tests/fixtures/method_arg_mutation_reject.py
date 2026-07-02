# PMAT-1037 guard (the g14 witness): a MUTATING method call whose container
# arg is reused — `q.pop(0)` buried in the store's INDEX marks `drain` as
# mutating parameter #1 (expr_has_mutator's new Subscript arm), and the
# check_expr_for_alias_mutate MethodCall arm refuses the reused-arg call.
# Without the pair: the reuse-clone would land the pop on a throwaway copy
# and len(q) would print 2 instead of CPython's 1 — a SILENT divergence.
class C:
    counts: list[int]

    def __init__(self) -> None:
        self.counts = [10, 20, 30]

    def drain(self, q: list[int]) -> None:
        self.counts[q.pop(0)] += 5


def main() -> None:
    c = C()
    q = [2, 0]
    c.drain(q)
    print(len(q))
