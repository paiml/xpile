# PMAT-043 / XPILE-BASHRS-MERGER-001:
#
# Fixture for the shell-side diff_exec gate. The test runs this
# file two ways and compares stdout:
#
#   1. CPython: `exec(open(file).read()); demo()` → the function's
#      subprocess.run calls fire and their stdout flows through.
#
#   2. Shell:   `xpile transpile file --target shell | /bin/sh`
#      depyler-frontend recognises each subprocess.run([...]) and
#      lowers it to a Stmt::Cmd; bashrs-backend emits real POSIX sh;
#      /bin/sh executes the same commands directly.
#
# Both pipelines must produce the same stdout. The test compares
# byte-for-byte. `demo()` returns int (not implicitly None) to
# keep depyler-frontend's "exactly one final return" lowering
# invariant happy.
#
# This fixture is the load-bearing **Runtime stratum witness** for
# `C-BASHRS-POSIX-IDEMPOTENCE` — it ships the contract from
# UNVERIFIED to PARTIAL on the `xpile quorum` table by demonstrating
# that the cross-domain Python→shell transpile preserves observable
# behaviour on a real command sequence. Without this PR, no test
# actually runs the bashrs-emitted shell; PMAT-040's e2e test only
# verifies the string matches.
def demo() -> int:
    subprocess.run(["echo", "starting"])
    subprocess.run(["echo", "middle-line"])
    subprocess.run(["echo", "done"])
    return 0
