def swap_first_two(xs: list[int]) -> int:
    # The canonical in-place element swap.
    xs[0], xs[1] = xs[1], xs[0]
    return xs[0] * 100 + xs[1]


def reverse_inplace(xs: list[int]) -> int:
    # Two-pointer reversal via repeated subscript swaps.
    i = 0
    j = len(xs) - 1
    while i < j:
        xs[i], xs[j] = xs[j], xs[i]
        i += 1
        j -= 1
    return xs[0] * 10000 + xs[1] * 1000 + xs[4]


def bubble_sort_min(xs: list[int]) -> int:
    # Bubble sort — the textbook adjacent-swap loop.
    n = len(xs)
    for i in range(n):
        for j in range(n - 1):
            if xs[j] > xs[j + 1]:
                xs[j], xs[j + 1] = xs[j + 1], xs[j]
    return xs[0]


def dict_swap(d: dict[str, int], a: str, b: str) -> int:
    # Subscript-target swap also works for dict keys.
    d[a], d[b] = d[b], d[a]
    return d[a] * 10 + d[b]
