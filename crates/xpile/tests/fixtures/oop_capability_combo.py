# PMAT-1055 (adversarial-verify): a SINGLE class that COMBINES the recent OOP
# codegen slices in one differential fixture — the combination the per-slice
# fixtures don't reach. Covers:
#   * class constants folded across methods (str BASE + int WEIGHT/HOT), never
#     emitted as struct fields (PMAT-1054);
#   * an empty-dict field seeded from `{}` via the class-body K/V annotation,
#     grown by `self.buckets[k].append(v)` grouping (PMAT-1052/1037);
#   * a parallel insertion-order list so `trail()` PROVES dict iteration order
#     == insertion order (indexmap parity with CPython dict);
#   * a class-constant threshold read inside a nested loop (HOT);
#   * negative-index read-back through a field (`keys_seen[-1]`);
#   * a class constant in overflow-checked multiplication (WEIGHT).
# Differentially verified vs CPython (MATCH 'Hzam' / 2 / 6 / 'm').
class Histo:
    BASE: str = "H"
    WEIGHT: int = 3
    HOT: int = 10
    buckets: dict[str, list[int]]
    keys_seen: list[str]

    def __init__(self) -> None:
        self.buckets = {}
        self.keys_seen = []

    def record(self, k: str, v: int) -> None:
        if k not in self.buckets:
            self.buckets[k] = []
            self.keys_seen.append(k)
        self.buckets[k].append(v)

    def trail(self) -> str:
        out: str = self.BASE
        for k in self.buckets:
            out += k
        return out

    def hot_count(self) -> int:
        c: int = 0
        for k in self.buckets:
            for v in self.buckets[k]:
                if v > self.HOT:
                    c += 1
        return c

    def weighted(self, k: str) -> int:
        return len(self.buckets[k]) * self.WEIGHT

    def newest(self) -> str:
        return self.keys_seen[-1]


def combo_trail() -> str:
    h: Histo = Histo()
    h.record("z", 1)
    h.record("a", 20)
    h.record("z", 15)
    h.record("m", 2)
    return h.trail()


def combo_hot() -> int:
    h: Histo = Histo()
    h.record("z", 1)
    h.record("a", 20)
    h.record("z", 15)
    h.record("m", 2)
    return h.hot_count()


def combo_weighted() -> int:
    h: Histo = Histo()
    h.record("z", 1)
    h.record("z", 9)
    return h.weighted("z")


def combo_newest() -> str:
    h: Histo = Histo()
    h.record("z", 1)
    h.record("a", 2)
    h.record("m", 3)
    return h.newest()
