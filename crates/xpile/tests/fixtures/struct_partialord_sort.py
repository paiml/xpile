from dataclasses import dataclass


@dataclass
class Item:
    pri: int

    def __lt__(self, other: "Item") -> bool:
        return self.pri < other.pri


def sorted_asc_first(items: list[Item]) -> int:
    # in-place .sort() on a struct list with a custom __lt__ (PartialOrd, NOT Ord).
    # Was rustc E0277 (Vec::sort needs Ord); now uses sort_by(partial_cmp).
    items.sort()
    return items[0].pri


def sorted_desc_first(items: list[Item]) -> int:
    items.sort(reverse=True)
    return items[0].pri


def sorted_builtin_first(items: list[Item]) -> int:
    # the non-mutating sorted() expression form.
    out: list[Item] = sorted(items)
    return out[0].pri


def sorted_builtin_desc_first(items: list[Item]) -> int:
    out: list[Item] = sorted(items, reverse=True)
    return out[0].pri
