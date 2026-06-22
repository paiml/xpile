from dataclasses import dataclass


@dataclass
class Item:
    pri: int
    tag: str

    def __lt__(self, other: "Item") -> bool:
        return self.pri < other.pri


def max_tag(items: list[Item]) -> str:
    # max()/min() over a struct list with a custom __lt__ (PartialOrd, NOT Ord,
    # NOT Copy). Was rustc E0425 (bare `max(xs)`); now uses max_by(partial_cmp).
    # Python returns the FIRST element with the maximal key (tie → first).
    return max(items).tag


def min_tag(items: list[Item]) -> str:
    return min(items).tag


def max_pri(items: list[Item]) -> int:
    return max(items).pri
