def main() -> None:
    s: str = "  Hello World  "
    print(s.strip())
    print(s.strip().lower())
    parts: list[str] = "a,b,c".split(",")
    print(len(parts))
    print("x".join(parts))
    print("banana".replace("a", "o"))
