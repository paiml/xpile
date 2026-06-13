from dataclasses import dataclass


@dataclass
class Box:
    value: int

    def get(self) -> int:
        return self.value


def bad() -> int:
    # `get` is an INSTANCE method, not a @staticmethod — calling it via the
    # class name (Python's unbound-method form) is unsupported and must be
    # rejected cleanly rather than emitting `Box::get(5)` (which needs `&self`).
    return Box.get(5)
