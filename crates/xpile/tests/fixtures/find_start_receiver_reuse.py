# PMAT-1011 (sweep #7): the with-start find/index form (`s.index(sub, start)`,
# PMAT-675) bound the receiver with `let __s = (s)` — a MOVE of the non-Copy
# String, so any LATER use of `s` failed rustc E0382 (use after move). The
# single-arg form was fixed by PMAT-851; this is the same clone applied to the
# start/end form (find/rfind/index/rindex/count).
def f(s: str) -> int:
    i = s.index("a", 2)
    c = s.count("a", 1)
    return i + c + len(s)
