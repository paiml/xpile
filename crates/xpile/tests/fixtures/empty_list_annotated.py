def build_with_append(n: int) -> int:
    # Annotated empty list, then build it up — the canonical accumulator idiom.
    xs: list[int] = []
    for i in range(n):
        xs.append(i)
    return len(xs)


def empty_int_list() -> list[int]:
    return []


def empty_str_list() -> list[str]:
    # element type str — the declared return type supplies it (an empty `[]`
    # can't self-infer).
    return []


def early_empty(n: int) -> list[int]:
    if n < 0:
        return []
    xs: list[int] = []
    xs.append(n)
    return xs


def empty_dict_return() -> dict[int, int]:
    return {}


def annotated_str_accumulate() -> int:
    words: list[str] = []
    words.append("a")
    words.append("b")
    return len(words)
