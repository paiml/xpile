from typing import Optional
def maybe(n: int) -> Optional[int]:
    if n > 0:
        return n
    return None
def main() -> None:
    xs: list[Optional[int]] = [maybe(-1), maybe(5), maybe(-1), maybe(3)]
    total: int = 0
    seen: int = 0
    for x in xs:
        if x is None:
            continue
        total += x
        seen += 1
    print(total)
    print(seen)
