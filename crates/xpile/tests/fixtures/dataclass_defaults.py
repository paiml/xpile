from dataclasses import dataclass


@dataclass
class Config:
    timeout: int = 30
    retries: int = 3
    name: str = "default"


def all_defaults() -> int:
    # No args → all fields take their literal defaults.
    c = Config()
    return c.timeout + c.retries


def partial() -> int:
    # Override one; the rest default.
    c = Config(timeout=5)
    return c.timeout + c.retries


def named_override() -> str:
    c = Config(name="custom")
    return c.name
