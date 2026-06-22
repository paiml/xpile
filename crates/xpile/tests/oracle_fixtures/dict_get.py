def main() -> None:
    d: dict[str, int] = {"a": 1, "b": 2}
    print(d.get("a", 0))
    print(d.get("z", -1))
    counts: dict[str, int] = {}
    for ch in "banana":
        counts[ch] = counts.get(ch, 0) + 1
    for k in counts:
        print(k, counts[k])
