def f() -> int:
    fns = [lambda: i for i in range(3)]
    return len(fns)
