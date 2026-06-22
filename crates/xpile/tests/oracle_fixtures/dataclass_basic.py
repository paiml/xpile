from dataclasses import dataclass
@dataclass
class Point:
    x: int
    y: int
    def norm1(self) -> int:
        return abs(self.x) + abs(self.y)
def main() -> None:
    p: Point = Point(3, -4)
    print(p.norm1())
    print(p.x, p.y)
