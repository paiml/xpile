def main() -> None:
    # PMAT-1363: the GREEN half of the annotated-comprehension element-type
    # check. Every shape below has an annotation that AGREES with what the
    # comprehension produces, so all of them must keep transpiling and must
    # keep matching CPython. The refusal added for the contradicting shapes
    # (`xs: list[str] = [i for i in range(5)]`, which used to emit
    # `let xs: Vec<String> = ...collect::<Vec<i64>>()` and fail rustc E0308)
    # is deliberately conservative; this fixture is what "conservative" means
    # operationally — the differential oracle fails loudly if the check ever
    # starts refusing a program that agrees with its annotation.
    ints: list[int] = [i * i for i in range(5)]
    print(ints[3])

    filtered: list[int] = [i for i in range(6) if i % 2 == 0]
    print(filtered[1])

    words = ["a", "bb", "ccc"]

    # Identity body over a str list — the element type comes from the iterable,
    # not the body (the PMAT-678 comp-binder inference).
    same: list[str] = [w for w in words]
    print(same[2])

    # Method-call and call bodies: the element type comes from the callee.
    upper: list[str] = [w.upper() for w in words]
    print(upper[1])

    lens: list[int] = [len(w) for w in words]
    print(lens[2])

    # A genuinely float-valued body under a `list[float]` annotation stays
    # accepted — only an INT-valued body under `list[float]` refuses, because
    # CPython keeps those elements ints (a float annotation is non-enforcing).
    halves: list[float] = [i / 2 for i in range(4)]
    print(halves[3])

    flags: list[bool] = [i % 2 == 0 for i in range(4)]
    print(flags[1])

    # Two-generator comprehension (the nested-loop lowering, not the map form).
    pairs: list[int] = [i + j for i in range(2) for j in range(3)]
    print(len(pairs))
    print(pairs[4])

    # Set and dict comprehensions in both key and value positions. Sets print
    # via len()/sorted() only — bare set repr carries the documented CPython
    # hash-order divergence.
    uniq: set[int] = {i % 3 for i in range(9)}
    print(len(uniq))

    names: set[str] = {w for w in words}
    print(len(names))

    doubled: dict[int, int] = {i: i * 2 for i in range(4)}
    print(doubled[3])

    by_word: dict[str, int] = {w: len(w) for w in words}
    print(by_word["ccc"])

    by_len: dict[int, str] = {len(w): w for w in words}
    print(by_len[2])

    # A non-scalar element leaf: the check declines to judge these at all, so
    # they must pass through exactly as before.
    nested: list[list[int]] = [[i, i + 1] for i in range(3)]
    print(nested[2][1])
