# PMAT-696: a float dict VALUE (not key) is fine — only the key is hashed. This
# locks in that the float-key reject does NOT over-reach to values.
def g() -> float:
    d: dict[str, float] = {"a": 1.5, "b": 2.5}
    return d["a"] + d["b"]
