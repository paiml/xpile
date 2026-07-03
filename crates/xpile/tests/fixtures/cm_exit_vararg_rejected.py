# PMAT-1094 (skeptic pass PMAT-1090, B-F10): a vararg on `__exit__` escapes
# the PMAT-1084 param scans (they read `args.args` only) — the lowered
# `fn __exit__(&self, args: Vec<i64>)` is called with ZERO args by the
# desugar, a rustc E0061 far from the cause. The vararg RECEIVES the exc
# triple in CPython (`(None, None, None)` on the clean path), so fabricating
# it would be the PMAT-1084 hole again. Must REFUSE at the `with` site.
class CM:
    def __enter__(self) -> "CM":
        print("enter")
        return self

    def __exit__(self, *args: int) -> None:
        print("exit")


def main() -> None:
    with CM():
        print("body")
