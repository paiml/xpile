# PMAT-502co (Tranche 2): no-arg str.split() — whitespace split (collapses
# runs of whitespace, drops empty fields), like Rust's split_whitespace.
def word_count(s: str) -> int:
    return len(s.split())


def first_word(s: str) -> str:
    parts = s.split()
    return parts[0]
