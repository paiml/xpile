def sign(x: int) -> int:
    if x > 0:
        s = 1
    elif x < 0:
        s = -1
    else:
        s = 0
    return s
