# PMAT-668: sep.join(d) over a dict joins its KEYS in Python (iterating a dict
# yields keys). A bare dict arg emitted `d.join(...)` on a HashMap (no `.join`
# → E0599). The join arg now materializes to the keys (mirror PMAT-656).
# NOTE single-key dict to keep the test deterministic — multi-key iteration
# order is the deferred PMAT-537 limitation.


def join_one_key(d: dict[str, int]) -> str:
    return "-".join(d)


def join_keys_regression(d: dict[str, int]) -> str:
    return "-".join(d.keys())


def join_list_regression(xs: list[str]) -> str:
    return ",".join(xs)
