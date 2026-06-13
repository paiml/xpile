def list_mags(a: int, b: int) -> list[int]:
    return [abs(a), abs(b)]


def dict_mag(n: int) -> dict[str, int]:
    return {"m": abs(n)}


def set_mags(a: int, b: int) -> set[int]:
    return {abs(a), abs(b)}
