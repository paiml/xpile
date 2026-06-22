# V29-2 (PMAT-884, silent-wrong): the arg-clone-drops-mutation object-reference
# miscompile. `add_item` mutates its `list[int]` parameter in place (Python
# passes objects by reference, so the caller's `nums` would become [1, 2, 3, 99]).
# xpile lowers parameters by value, and because `nums` is re-read after the call
# (`print(nums)`) the ownership pre-pass (PMAT-588) clones the argument to avoid
# a use-after-move (E0382). The clone makes `.append(99)` land on the throwaway
# copy, so the caller's `nums` is never touched — the emitted Rust COMPILES but
# prints `[1, 2, 3]` (a silent wrong answer). The full fix is the Rc<RefCell>
# reference layer + escape/aliasing analysis (architectural). Until then xpile
# must CLEAN-REJECT this alias-then-mutate pattern rather than miscompile it,
# mirroring the heterogeneous-list-literal / tuple-call rejects.
def add_item(lst: list[int]) -> int:
    lst.append(99)
    return len(lst)


def main() -> int:
    nums = [1, 2, 3]
    n = add_item(nums)
    print(nums)
    return n
