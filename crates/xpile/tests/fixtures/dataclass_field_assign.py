from dataclasses import dataclass


@dataclass
class Counter:
    value: int
    step: int


def advance(c: Counter, times: int) -> int:
    # Field assignment mutates the struct param in place (param emitted `mut`).
    c.value = c.value + c.step * times
    return c.value


def reset_and_set(c: Counter) -> int:
    # Multiple field assignments.
    c.value = 0
    c.step = 1
    return c.value + c.step
