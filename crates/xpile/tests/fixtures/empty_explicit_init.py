# PMAT-1057: an explicit EMPTY `def __init__(self): pass` on a const-only /
# field-less class was NOT routed through the ctor synthesis (the dispatch
# gated on body_assigns_self, false for an empty body) — it emitted as a
# `&self` method and `Config::__init__()` was rustc E0061. __init__ is a
# CONSTRUCTOR and now always synthesizes as `fn __init__(...) -> Self`, even
# with no field assignments. Completes the PMAT-1054 class-const surface
# (instance access `c.VERSION` with an explicit empty init). Verified vs CPython.
class Config:
    VERSION: int = 5
    MAX: int = 100

    def __init__(self) -> None:
        pass


class Greeter:
    def __init__(self) -> None:
        pass

    def greet(self) -> str:
        return "hi"


def const_via_instance() -> int:
    c: Config = Config()
    return c.VERSION + c.MAX


def empty_with_method() -> str:
    g: Greeter = Greeter()
    return g.greet()
