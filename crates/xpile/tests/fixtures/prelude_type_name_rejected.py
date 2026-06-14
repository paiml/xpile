from dataclasses import dataclass


# A class named `Vec` emits `struct Vec`, which collides with Rust's prelude
# `Vec<T>` once the module also uses a list (`Vec<i64>`) — rustc E0107. xpile
# must reject this cleanly rather than emit invalid Rust.
@dataclass
class Vec:
    v: int


def total(xs: list[int]) -> int:
    s = 0
    for x in xs:
        s = s + x
    return s
