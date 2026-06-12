# PMAT-502b (Tranche 2): str.replace(old, new) -> .replace(&(old)[..], &(new)[..]).
def censor(s: str) -> str:
    return s.replace("bad", "***")


def swap(s: str, old: str, new: str) -> str:
    return s.replace(old, new)
