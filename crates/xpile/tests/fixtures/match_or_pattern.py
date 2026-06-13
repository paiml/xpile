def day_kind(d: int) -> int:
    # `|`-pattern: weekend (5, 6) vs weekday, terminal form.
    match d:
        case 5 | 6:
            return 0
        case 0 | 1 | 2 | 3 | 4:
            return 1
        case _:
            return -1


def vowel_score(c: str) -> int:
    # `|`-pattern over str literals, statement position.
    score = 0
    match c:
        case "a" | "e" | "i" | "o" | "u":
            score = 2
        case _:
            score = 1
    return score
