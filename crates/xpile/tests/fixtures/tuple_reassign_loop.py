def gcd(a: int, b: int) -> int:
    # Euclid: a, b = b, a % b in a while body — must reassign, not shadow
    # (shadowing → b never updates → infinite loop).
    while b != 0:
        a, b = b, a % b
    return a


def fib(n: int) -> int:
    # Iterative Fibonacci: a, b = b, a + b in a for body.
    a = 0
    b = 1
    for _ in range(n):
        a, b = b, a + b
    return a


def max_product(xs: list[int]) -> int:
    # DP swap inside an if body (max-product-subarray).
    cur_max = xs[0]
    cur_min = xs[0]
    best = xs[0]
    for i in range(1, len(xs)):
        x = xs[i]
        if x < 0:
            cur_max, cur_min = cur_min, cur_max
        cur_max = max(x, cur_max * x)
        cur_min = min(x, cur_min * x)
        best = max(best, cur_max)
    return best


def swap_top(a: int, b: int) -> int:
    # Top-level swap (already worked) stays correct.
    a, b = b, a
    return a * 10 + b
