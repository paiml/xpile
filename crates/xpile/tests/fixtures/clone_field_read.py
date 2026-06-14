from dataclasses import dataclass


@dataclass
class P:
    name: str
    tags: list[int]
    age: int

    def get_name(self) -> str:
        # Returning a non-Copy (String) field by value from `&self` previously
        # emitted `(self).name`, which rustc rejects (E0507: move out of a
        # shared reference). Now clones.
        return self.name

    def get_tags(self) -> list[int]:
        # list field — also non-Copy, also cloned.
        return self.tags

    def get_age(self) -> int:
        # Copy field — no clone.
        return self.age
