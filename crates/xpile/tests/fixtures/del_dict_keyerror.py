# PMAT-709: `del d[k]` on an absent key raises KeyError in Python; xpile's bare
# `d.remove(&k)` discarded the Option and silently succeeded (silent-wrong). Now
# it asserts the key was present (mirrors set.remove). Present-key del unchanged.
def drop_present(d: dict[str, int]) -> int:
    del d["a"]
    return len(d)


def drop_var(d: dict[str, int], k: str) -> int:
    del d[k]
    return len(d)
