def build(n: int) -> str:
    out: str = ""
    i: int = 0
    while i < n:
        out = out + chr(65 + i)
        i = i + 1
    return out


def run() -> int:
    s: str = build(5)
    prefix: str = "id-"
    msg: str = prefix + s
    total: int = 0
    if msg == "id-ABCDE":
        total = total + 1000
    total = total + len(msg)
    total = total + ord(s[4])
    return total
