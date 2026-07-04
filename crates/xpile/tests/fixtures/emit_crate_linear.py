def predict(x0: float, x1: float, x2: float) -> float:
    return 2.5 * x0 + -1.3 * x1 + 0.75 * x2 + 0.4

def main() -> None:
    print(predict(1.0, 2.0, 3.0))
