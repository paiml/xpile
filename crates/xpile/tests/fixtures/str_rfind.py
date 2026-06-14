def last_a(s: str) -> int:
    # rfind returns the highest index of the substring, or -1.
    return s.rfind("a")


def last_missing(s: str) -> int:
    return s.rfind("z")


def last_pair(s: str) -> int:
    return s.rfind("an")


def last_a_index(s: str) -> int:
    # rindex == rfind but panics (ValueError) when absent.
    return s.rindex("a")
