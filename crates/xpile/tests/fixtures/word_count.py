# PMAT-466 regression (review #5/#6/#9): string-keyed dict histogram.
# The canonical `counts[w] = counts.get(w, 0) + 1` idiom over NON-Copy
# (String) keys. A naive `counts.insert(w, counts.get(w,0)+1)` moves
# `w` into argument 1 while the value still borrows it (E0382). The
# DictSet emission binds the value to a temp first, so this compiles
# for str keys (and int keys alike). This is the spec §30 count_chars
# shape, over a list instead of a string.
def word_count(words: list[str]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for w in words:
        counts[w] = counts.get(w, 0) + 1
    return counts
