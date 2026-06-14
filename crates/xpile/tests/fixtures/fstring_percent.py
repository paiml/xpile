def rate1(x: float) -> str:
    # `.1%` — scale by 100, 1 decimal, append `%`.
    return f"{x:.1%}"


def rate0(x: float) -> str:
    # `.0%` — no decimals (rounds).
    return f"{x:.0%}"


def rate_default(x: float) -> str:
    # Bare `%` — Python's default 6 decimals.
    return f"{x:%}"


def labeled(x: float) -> str:
    # `.2%` inside surrounding text.
    return f"share={x:.2%}"
