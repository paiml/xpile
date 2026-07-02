def count_vowels(s: str) -> int:
    n: int = 0
    for ch in s:
        if ch == "a" or ch == "e":
            n = n + 1
    return n

def shout(s: str) -> str:
    out: str = ""
    for ch in s:
        out = out + ch + ch
    return out

def skip_a(s: str) -> int:
    n: int = 0
    for ch in s:
        if ch == "a":
            continue
        n = n + 1
    return n

def find_x(s: str) -> int:
    i: int = 0
    for ch in s:
        if ch == "x":
            break
        i = i + 1
    return i

def pairs(s: str) -> int:
    n: int = 0
    for a in s:
        for b in s:
            if a == b:
                n = n + 1
    return n

def sum_ord(s: str) -> int:
    t: int = 0
    for ch in s:
        t = t + ord(ch)
    return t

def run() -> int:
    v: str = shout("abc")
    return count_vowels("banana tree") * 100000 + len(v) * 10000 + skip_a("banana") * 1000 + find_x("abxcd") * 100 + pairs("aba") * 10 + (sum_ord("AB") - 129)
