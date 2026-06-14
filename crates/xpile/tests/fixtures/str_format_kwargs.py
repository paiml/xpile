def greet(name: str) -> str:
    # named field — keyword form of str.format.
    return "hello {who}!".format(who=name)


def coords(a: int, b: int) -> str:
    # multiple named fields, referenced in template order.
    return "{x},{y}".format(x=a, y=b)


def reorder(a: int, b: int) -> str:
    # template order differs from kwarg order.
    return "{second}-{first}".format(first=a, second=b)


def repeated(n: int) -> str:
    # a named field reused (positional {N} supports repeats; auto {} does not).
    return "{v} {v} {v}".format(v=n)


def with_spec(x: float) -> str:
    # named field carrying a format spec.
    return "{val:.2f}".format(val=x)
