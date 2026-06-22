def main() -> None:
    d: dict[str, int] = {}
    d["z"] = 1
    d["a"] = 2
    d["m"] = 3
    for k in d:
        print(k, d[k])
