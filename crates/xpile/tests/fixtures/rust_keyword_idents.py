def move(type: int) -> int:
    # `move` (fn name), `type`/`match`/`loop` (locals) are all Rust keywords
    # but legal Python identifiers.
    match = type + 1
    loop = match * 2
    return loop


def process(box: list[int]) -> int:
    # `box` (param), `final`/`ref` (locals + for-var) are Rust keywords.
    final = 0
    for ref in box:
        final = final + ref
    return final


def transform(do: list[int]) -> list[int]:
    # `do` (param) and the comprehension binder `type` are Rust keywords.
    return [type * 2 for type in do]


def mutate(impl: list[int]) -> int:
    # `impl` is a Rust keyword used as a mutated (method-receiver) param;
    # the internal call to `move(...)` must also escape to `r#move`.
    impl.append(99)
    return impl[move(0)]
