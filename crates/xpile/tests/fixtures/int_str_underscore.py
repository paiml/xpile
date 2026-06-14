# PMAT-610: int(str) accepts PEP 515 underscore digit separators (int("1_000")
# == 1000), which Rust's parse::<i64>() rejects → panic. Python allows a single
# underscore only BETWEEN digits; xpile validates that, strips, then parses.
def parse(s: str) -> int:
    return int(s)
