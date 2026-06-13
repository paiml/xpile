def total(*args: int) -> int:
    return sum(args)


def with_prefix(prefix: int, *args: int) -> int:
    return prefix + sum(args)


def forward(xs: list[int]) -> int:
    return total(*xs)


def forward_prefixed(xs: list[int]) -> int:
    return with_prefix(10, *xs)


def forward_empty(xs: list[int]) -> int:
    return total(*xs)
