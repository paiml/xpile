# PMAT-1015 (sweep #7): the TUPLE-target analogue of the PMAT-1012 loop-var
# leak — `for i, v in enumerate(xs): pass` then `return i + v` (Python leaks
# both targets; i==len-1, v==last elem). PMAT-871 leaks PRE-BOUND tuple
# targets, but a FRESH tuple target had no pre-declare, so the post-loop read
# was rustc E0425. The pre-declare now derives per-element types per iterable
# form: enumerate → (int, elem); zip → per-arg elems; anything probe-lowering
# to list[tuple] (dict.items(), a list-of-tuples var) → the tuple elems.
def enum_leak(xs: list[int]) -> int:
    for i, v in enumerate(xs):
        pass
    return i + v


def zip_leak(a: list[int], b: list[int]) -> int:
    for x, y in zip(a, b):
        pass
    return x + y


def items_leak(d: dict[str, int]) -> int:
    n = 0
    for k, v in d.items():
        n = n + v
    return n + v
