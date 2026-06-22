from typing import Optional
def main() -> None:
    xs: list[Optional[int]] = [5, None, 3, None, 1]
    total: int = 0
    present: int = 0
    for x in xs:
        if x is not None:
            total += x
            present += 1
    print(total)
    print(present)
    print(len(xs))
