from typing import Optional
def maybe(n: int) -> Optional[int]:
    if n > 0:
        return n
    return None
def first_pos(xs: list[Optional[int]]) -> int:
    total: int = 0
    for x in xs:
        if x is not None:
            total += x
    return total
def main() -> None:
    a: list[Optional[int]] = [maybe(-1), maybe(-1)]
    print(first_pos(a))
    b: list[Optional[int]] = [maybe(5), maybe(-1), maybe(3)]
    print(first_pos(b))
