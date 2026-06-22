def safe(a: int, b: int) -> int:
    try:
        return a // b
    except ZeroDivisionError:
        return -1
def main() -> None:
    print(safe(10, 2))
    print(safe(10, 0))
