from dataclasses import dataclass


@dataclass
class MathBox:
    value: int

    @staticmethod
    def add(a: int, b: int) -> int:
        return a + b

    @staticmethod
    def triple(n: int) -> int:
        return n * 3

    def boosted(self) -> int:
        # An instance method may call a static method via the class name.
        return MathBox.add(self.value, MathBox.triple(self.value))


def use_add(a: int, b: int) -> int:
    return MathBox.add(a, b)


def use_triple(n: int) -> int:
    return MathBox.triple(n)


def use_boosted(v: int) -> int:
    b = MathBox(v)
    return b.boosted()
