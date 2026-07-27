# PMAT-1387 — the VACUOUS twin of `hybrid_sum`. Identical FFI boundary, but
# `main()` prints NOTHING, so the CPython reference and the executed artifact
# both produce the empty string and byte-identity holds TRIVIALLY. Before
# PMAT-1387 this fixture drove `--verify` to print
# `✓ MATCH — stdout byte-identical (1 line(s)): ""` and exit 0 — a green
# verdict from a run in which `square_sum` was never called and nothing at all
# was observed. It must now REFUSE.
from ._core import square_sum


def main() -> None:
    pass
