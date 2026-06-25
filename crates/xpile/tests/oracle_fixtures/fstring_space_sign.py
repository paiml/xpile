def main() -> None:
    # PMAT-942: the SPACE sign flag ` ` — a leading SPACE before a non-negative
    # magnitude, `-` before a negative one; composes with width / zero-pad /
    # precision; int coerced to float by a float-presentation spec; bool → int.
    print(f"{5: d}")
    print(f"{-5: d}")
    print(f"{0: d}")
    print(f"{123: }")
    print(f"{-7: }")
    print(f"{3.14: .2f}")
    print(f"{-3.14: .2f}")
    print(f"{0.0: .2f}")
    print(f"{5: .1f}")
    print(f"{42: 6d}")
    print(f"{-42: 6d}")
    print(f"{5: 05d}")
    print(f"{-5: 05d}")
    print(f"{3.14159: 8.2f}")
    print(f"{-3.14159: 8.2f}")
    print(f"{3.14: 08.2f}")
    print(f"{-3.14: 08.2f}")
    print(f"{True: d}")
    print(f"x={7: d} y={-3: d}")
