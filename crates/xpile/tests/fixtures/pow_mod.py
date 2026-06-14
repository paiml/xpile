def modpow(a: int, b: int, m: int) -> int:
    # 3-arg pow = modular exponentiation a**b mod m.
    return pow(a, b, m)


def big_mod(a: int, b: int) -> int:
    # Modulus near i64::MAX — i128 intermediates avoid overflow.
    return pow(a, b, 9223372036854775783)


def neg_base(b: int) -> int:
    return pow(-3, b, 7)


def mod_one(a: int, b: int) -> int:
    return pow(a, b, 1)
