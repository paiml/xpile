# PMAT-502ct (Tranche 2): default parameter values. Rust has no defaults, so
# omitted trailing arguments are filled with the declared default at each call
# site (the def's Rust signature keeps every param). Works with keyword
# overrides too (add(1, c=5)).
def greet(name: str, greeting: str = "Hello") -> str:
    return greeting + ", " + name


def use_default(name: str) -> str:
    return greet(name)


def with_hi(name: str) -> str:
    return greet(name, "Hi")


def add(a: int, b: int = 10, c: int = 100) -> int:
    return a + b + c


def call_add() -> int:
    return add(1)


def call_kw() -> int:
    return add(1, c=5)
