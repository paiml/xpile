from dataclasses import dataclass


@dataclass
class Account:
    balance: int
    bonus: int


def run_deposits(d1: int, d2: int) -> int:
    a = Account(100, 0)
    a.balance += d1
    a.balance += d2
    a.balance -= 5
    return a.balance


def scale_bonus(n: int) -> int:
    a = Account(0, 3)
    a.bonus *= n
    a.bonus += 1
    return a.bonus


def combined(start: int) -> int:
    a = Account(start, start)
    a.balance += a.bonus
    a.bonus -= 2
    return a.balance + a.bonus
