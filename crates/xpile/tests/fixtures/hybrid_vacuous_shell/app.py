# PMAT-1387 — the SHELL lane's vacuous twin (see hybrid_vacuous_c/app.py). The
# `sh` reference side is `_tool.sh`, which is EMPTY, so the original script and
# the re-emitted one both print nothing and the round-trip differential proves
# nothing. It must REFUSE, not report MATCH.
from ._tool import _tool


def main() -> None:
    pass
