# PMAT-040 / XPILE-BASHRS-MERGER-001 v0.3.0 falsifier evidence:
#
# This fixture is the load-bearing demonstration that the meta-HIR
# shell variants land on a real cross-domain consumer. The `build`
# function below uses Python's `subprocess.run([...])` — depyler-frontend
# recognises it and lowers each call to a `Stmt::Cmd` in the meta-HIR.
# `xpile transpile subprocess_demo.py --target shell` then produces
# real POSIX sh via bashrs-backend.
#
# Without this PR, the `sub/bashrs-merger.md` v0.3.0 check-back would
# require a future demonstration that at least one cross-domain
# producer of shell variants ships before the IR merge is reverted
# (XPILE-UNMERGE-001). This fixture is that demonstration.
#
# Per the v0.3.0 check-back acceptance set, exactly one of:
#   (a) Python `subprocess.run` recognition  ← THIS FIXTURE
#   (b) Rust `Command::new` recognition
#   (c) Lean theorem about shell composition
# is required. (a) is the cheapest to ship — `subprocess.run` is the
# canonical safe form in Python and matches `Stmt::Cmd` exactly.
def build() -> int:
    subprocess.run(["echo", "starting"])
    subprocess.run(["ls", "/tmp"])
    subprocess.run(["pwd"])
    subprocess.run(["echo", "done"])
    return 0
