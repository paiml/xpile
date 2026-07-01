# PMAT-1011 (sweep #7): Python 3.8+ 3-arg pow with a NEGATIVE exponent is the
# MODULAR INVERSE (bpo-36027): pow(3, -1, 7) == 5, pow(2, -2, 9) == 7, raising
# "ValueError: base is not invertible for the given modulus" when
# gcd(base, mod) != 1. The old emit panicked with the STALE pre-3.8 message
# ("pow() 2nd argument cannot be negative when 3rd argument specified") — a
# stale-semantics divergence, not a capability gap.
def powmod(b: int, e: int, m: int) -> int:
    return pow(b, e, m)
