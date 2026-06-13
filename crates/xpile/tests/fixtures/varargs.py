def total(*args: int) -> int:
    return sum(args)


def with_prefix(prefix: int, *args: int) -> int:
    return prefix + sum(args)


def call_total() -> int:
    return total(1, 2, 3)


def call_empty() -> int:
    return total()


def call_one() -> int:
    return total(5)


def call_prefix() -> int:
    return with_prefix(100, 1, 2)


def call_prefix_only() -> int:
    return with_prefix(100)
