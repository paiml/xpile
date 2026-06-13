# PMAT-502by (Tranche 2): print(..., sep=..., end=...) keyword args.
# sep joins the args (default " "); end terminates (default "\n"). A custom
# end uses print! (no auto-newline) so `end=""` concatenates onto the next.
def demo(a: int, b: int) -> None:
    print(a, b, sep=", ")
    print("loading", end="")
    print("...", end="")
    print("done")
    print(a, b, sep=" | ")
