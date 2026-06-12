# PMAT-502 (Tranche 2): general Stmt::If with side-effecting branches.
# `if w in freq: freq[w] += 1 else: freq[w] = 1` — the branches mutate a
# dict (subscript assign / aug-subscript), which the if-as-let form
# rejected; now they lower to a real Stmt::If. The canonical histogram.
def word_freq(words: list[str]) -> dict[str, int]:
    freq: dict[str, int] = {}
    for w in words:
        if w in freq:
            freq[w] += 1
        else:
            freq[w] = 1
    return freq
