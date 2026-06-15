# PMAT-618: comparing a no-default `d.get(k)` with a value. `d.get(k)` (no
# default) is Option<T>, so `d.get(k) == 5` emitted `Option<i64> == i64` (E0308).
# Python returns None when the key is absent (`None == 5` is False), which Rust
# models exactly as `Option<T> == Some(5)` — the bare-value side is wrapped in
# Some. Only ==/!= (a </>  on a possibly-None is a Python TypeError).
def eq5(d: dict[str, int], k: str) -> bool:
    return d.get(k) == 5


def ne5(d: dict[str, int], k: str) -> bool:
    return d.get(k) != 5
