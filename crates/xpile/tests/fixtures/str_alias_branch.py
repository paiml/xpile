def run() -> int:
    s: str = "branchy"
    n: int = len(s)
    b: str = ""
    if n > 3:
        b = s
    return len(b) * 10 + len(s)
