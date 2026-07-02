# PMAT-1023 (sweep #10 — the realistic-idiom sweep): the UNDER-CLONE class.
# The PMAT-588 clone-if-reused pass skipped three everyday contexts because
# count_name_reads under-counted: (1) f-string interiors were INVISIBLE
# (`f"{mean(temps)}"` never counted temps → the compute-then-reuse shape was
# E0382); (2) loop bodies counted ONCE (a single in-loop call read moves on
# iteration 2); (3) reassignment from a re-read name moved it (`best = name`
# in a loop — the track-the-best-key idiom); chained-comparison temps moved
# their non-Copy operands (`"a" <= ch <= "z"` then reuse ch).
def mean(values: list[float]) -> float:
    return sum(values) / len(values)


def spread(values: list[float]) -> float:
    return max(values) - min(values)


def stats_report() -> str:
    temps = [20.5, 22.1, 19.8]
    return f"{mean(temps):.2f}/{spread(temps):.2f}/{len(temps)}"


def best_key() -> str:
    grades = {"alice": 88, "bob": 95, "carol": 72}
    best = ""
    best_score = -1
    for name in sorted(grades):
        if grades[name] > best_score:
            best = name
            best_score = grades[name]
    return best


def total(values: list[int]) -> int:
    t = 0
    for v in values:
        t = t + v
    return t


def loop_calls() -> int:
    nums = [1, 2, 3]
    acc = 0
    for i in range(3):
        acc = acc + total(nums) + i
    return acc


def shift_lower(text: str) -> str:
    out: str = ""
    for ch in text:
        if "a" <= ch <= "z":
            out = out + chr(ord("a") + (ord(ch) - ord("a") + 1) % 26)
        else:
            out = out + ch
    return out
