# PMAT-599: a dict comprehension reusing a non-Copy loop var in both the key
# and the value moved it into the (key, value) tuple before the value could use
# it (rustc E0382). The key is now cloned (gated on read-count>1 + non-Copy) so
# the value keeps a live value.
def identity(words: list[str]) -> int:
    d: dict[str, str] = {w: w for w in words}
    return len(d)


def with_suffix(words: list[str]) -> str:
    d: dict[str, str] = {w: w + "!" for w in words}
    return d["a"]


def key_lengths(words: list[str]) -> int:
    d: dict[str, int] = {k: len(k) for k in words}
    return d["abc"]
