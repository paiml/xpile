def greet(name: str, n: int) -> str:
    return f"{name} x{n}"
def main() -> None:
    print(greet("ab", 3))
    print("xy" * 3)
    print("HeLLo".lower())
    print(",".join(["a", "b", "c"]))
