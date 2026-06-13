def classify(n: int) -> int:
    # Terminal match — every case returns.
    match n:
        case 0:
            return 100
        case 1:
            return 200
        case -1:
            return 300
        case _:
            return 0


def grade_points(letter: str) -> int:
    # str literal patterns.
    match letter:
        case "A":
            return 4
        case "B":
            return 3
        case _:
            return 0


def step(state: int, x: int) -> int:
    # Statement-position match with assignment bodies, then a trailing return.
    result = 0
    match state:
        case 0:
            result = x + 1
        case 1:
            result = x * 2
        case _:
            result = x
    return result
