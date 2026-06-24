# PMAT-922 (HUNT-V16 #29): differential-oracle coverage for the explicit-separator
# split family. The valid forms (`split`, `split` with maxsplit, `rsplit`) must
# match CPython byte-for-byte; the empty-separator ValueError path is exercised
# separately by the e2e guard test (it raises, so it cannot live in a printing
# oracle fixture). Captured CPython stdout vs transpiled-Rust stdout.
def main() -> None:
    csv: str = "a,b,c,d"
    parts: list[str] = csv.split(",")
    print(len(parts))
    print(parts[0])
    print(parts[3])

    limited: list[str] = csv.split(",", 1)
    print(len(limited))
    print(limited[1])

    tail: list[str] = csv.rsplit(",", 2)
    print(len(tail))
    print(tail[0])

    no_match: list[str] = csv.split(";")
    print(len(no_match))
    print(no_match[0])
