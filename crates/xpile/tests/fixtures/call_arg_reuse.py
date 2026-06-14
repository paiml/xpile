def helper(xs: list[int]) -> int:
    return len(xs)


def two_calls(xs: list[int]) -> int:
    # `xs` (a non-Copy list) is passed by value to two calls — moving it into
    # the first leaves the second a use-after-move (rustc E0382). The reused
    # arg is now cloned so the original survives.
    return helper(xs) + helper(xs)


def call_then_use(xs: list[int]) -> int:
    # passed to a call, then read again afterwards.
    a = helper(xs)
    return a + len(xs)


def single_use(xs: list[int]) -> int:
    # single use — NOT cloned (passed by value as before).
    return helper(xs)
