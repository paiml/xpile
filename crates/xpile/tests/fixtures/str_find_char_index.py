def find_g(s: str, sub: str) -> int:
    # Python find returns a CHARACTER index, not a byte offset.
    return s.find(sub)


def rfind_a(s: str, sub: str) -> int:
    return s.rfind(sub)


def index_g(s: str, sub: str) -> int:
    return s.index(sub)


def not_found(s: str, sub: str) -> int:
    return s.find(sub)


def ascii_find(s: str, sub: str) -> int:
    return s.find(sub)
