# PMAT-605: 3-arg pow(a, b, m) with a NEGATIVE modulus. Python's result takes
# the sign of the modulus (range (m, 0] for m < 0); the square-multiply loop
# produced the non-negative Euclidean residue, so it was re-signed.
def mp(a: int, b: int, m: int) -> int:
    return pow(a, b, m)
