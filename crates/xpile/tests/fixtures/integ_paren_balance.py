# PMAT-875 (integration regression): string iteration + nested if + elif +
# accumulator/max tracking + arithmetic return. Cross-checked vs python3.


def balance(s: str) -> int:
    depth: int = 0
    maxd: int = 0
    for c in s:
        if c == "(":
            depth = depth + 1
            if depth > maxd:
                maxd = depth
        elif c == ")":
            depth = depth - 1
    return maxd * 10 + depth
