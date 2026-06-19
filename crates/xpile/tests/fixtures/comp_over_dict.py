# PMAT-832 (HUNT-V25 #14): a comprehension over a bare dict ([k for k in d]) was
# rejected ("dict iterables deferred") though the for-loop form works. Python
# iterates a dict as its keys. Both comp paths — the statement-form desugar and
# the value-position closure-chain — now iterate the keys (over_keys / DictView::
# Keys). Cross-checked vs python3.


def keys_len(d: dict[str, int]) -> int:
    ks = [k for k in d]
    return len(ks)


def sum_values(d: dict[str, int]) -> int:
    return sum([d[k] for k in d])
