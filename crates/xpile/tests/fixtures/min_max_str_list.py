def min_word(words: list[str]) -> str:
    # 1-arg min/max reduction over a list[str] (lexicographic, str is Ord).
    return min(words)


def max_word(words: list[str]) -> str:
    return max(words)


def min_word_default(words: list[str]) -> str:
    return min(words, default="zzz")


def min_int_regression(xs: list[int]) -> int:
    return min(xs)
