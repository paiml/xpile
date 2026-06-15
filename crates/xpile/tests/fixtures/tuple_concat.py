# PMAT-673: `a + b` over two tuples is CONCATENATION (Rust tuples have no `+`).
def concat(a: tuple[int, int], b: tuple[int, int]) -> tuple[int, int, int, int]:
    return a + b


def concat_str(a: tuple[str, str], b: tuple[str]) -> tuple[str, str, str]:
    return a + b


def concat_three(
    a: tuple[int, int], b: tuple[int], c: tuple[int, int]
) -> tuple[int, int, int, int, int]:
    return a + b + c


def concat_local(x: int, y: int) -> tuple[int, int, int]:
    a = (x, y)
    b = (x + y,)
    return a + b
