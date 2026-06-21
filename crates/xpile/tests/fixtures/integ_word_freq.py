# PMAT-875 (integration regression): a realistic multi-feature program — word
# frequency via dict.get + .items() iteration (insertion-ordered, IndexMap) +
# conditional max-tracking + str()/concat. Exercises feature INTERACTIONS that the
# single-feature micro-fixtures don't. Cross-checked vs python3.


def top_word(text: str) -> str:
    counts: dict[str, int] = {}
    for w in text.split():
        counts[w] = counts.get(w, 0) + 1
    best: str = ""
    best_n: int = 0
    for w, n in counts.items():
        if n > best_n:
            best_n = n
            best = w
    return best + ":" + str(best_n)
