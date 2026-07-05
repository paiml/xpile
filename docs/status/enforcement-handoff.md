# Enforcement hand-off — the org-gated tail of the fable architectural review

The EV-ranked backlog in [`docs/specifications/fable-architectural-review.md`](../specifications/fable-architectural-review.md)
§7b was implemented autonomously **as far as the PR flow reaches**. Seven gate
PRs landed (see the table below). Four items remain — not because the code is
hard, but because they require an action **outside** the `branch → PR → gate`
loop: an **org-admin ruleset edit**, an **owner governance decision**, or
**hardware/ops access**. This file is the runbook for that tail.

## What already landed (autonomous, merged)

| item | rank | PR | what it does |
|------|------|-----|--------------|
| XPILE-WITNESS-001 | 2 | #1871 region | CI installs WABT + `XPILE_REQUIRE_WASM_RUNTIME=1` makes a missing wasm runtime a **panic, not a skip** |
| XPILE-WITNESS-002 | 3 | #1879 | per-lane witness-floor manifest (wasm ≥400, shell ≥7, rust-diff ≥44, hybrid ≥3, wasi ≥1); GPU lanes must skip-with-reason, never silently absent |
| XPILE-CLAIMS-001 | 5 | #1880 | derives README/inventory/roadmap counts from code; fixed the live "25 vs 35" + PMAT-952-planned drift (red-then-green) |
| XPILE-CONTRACT-001 | 6 | #1874 region | 4 bare contracts each cite ≥1 existing executing falsifier; `pv lint` green |
| PMAT-482 | 7 | #1877 | offline naga + spirv-val gate — WGSL/SPIR-V validated on GPU-less CI, no skip path |
| XPILE-CLEANROOM-001 | 8 | #1878 | release workflow: `cargo publish --workspace --dry-run` under isolated CARGO_HOME + per-PR manifest falsifier |
| XPILE-PTX-001 | 12 | #1876 | `--hardware ptx` makes `--target ptx` CLI-reachable; refuses loudly without it |

**All seven run under the `workspace-test` (or `gate`) job — which the ruleset
below does not yet require.** They show red on a bad PR but do **not block
merge** until the ruleset edit lands. That is the single highest-EV item and it
is the one I cannot do.

---

## 1. XPILE-RULESET-001 (EV rank **1**) — make the gates actually enforce

**Why it's #1:** every gate inherits its meaning from enforcement. Today ruleset
`13878864` ("Green Main") requires **only** the `gate` context:

```
$ gh api repos/paiml/xpile/rules/branches/main
required_status_checks: ['gate']        # ← workspace-test is NOT here
strict_required_status_checks_policy: False
```

`gate` runs fmt/check/clippy/pv-lint/deny. It does **not** run the differential
witnesses, the witness-floor manifest, or the claims-drift gate — those are the
`workspace-test` job. So all seven PRs above are advisory-green until an
org-admin adds `workspace-test` to the required contexts. `ci.yml` even *says*
"Required check" in a comment (reconciled to an ENFORCEMENT NOTE by
WITNESS-001), but a comment is not a ruleset.

**The edit (org-admin only — outside PR flow):**

```bash
# Add workspace-test (RULESET-001) — and lake-build + kani (RULESET-002, below)
# in the same session. Idempotent: re-running is a no-op.
gh api repos/paiml/xpile/rulesets/13878864 \
  | jq '{name, target, enforcement, conditions, bypass_actors,
         rules: [ .rules[]
           | if .type=="required_status_checks"
             then .parameters.required_status_checks =
                  ( [ .parameters.required_status_checks[].context ]
                    + ["workspace-test","lake-build","kani"]
                    | unique | map({context: .}) )
             else . end ] }' > /tmp/ruleset-new.json

gh api --method PUT repos/paiml/xpile/rulesets/13878864 --input /tmp/ruleset-new.json
```

Then decide `strict` explicitly: `strict_required_status_checks_policy: true`
forces branches to be up-to-date with `main` before merge (safer, more rebases);
`false` (today) does not. The review recommends deciding it explicitly, not by
default.

**Falsifier (proves it's enforcing):** open a throwaway PR whose only change is
one deliberately failing `#[test]`. `workspace-test` goes red **and GitHub
refuses the merge button**. Before this edit, the same PR merges. Snapshot the
post-edit ruleset JSON to `docs/status/ruleset-13878864.json`.

---

## 2. XPILE-RULESET-002 (EV rank **4**) — promote the proof lane

Same ruleset session as #1 (the command above already adds `lake-build` and
`kani`). These jobs are **real, green, and latency-free to promote** today
(lake-build ~16s, kani ~2m). An advisory proof lane compounds no confidence.

**One repo-side companion PR** (this half *is* in PR flow, but is coupled to the
promotion so it's parked here): make `contracts/kani/kani_verify.rs` **fail, not
warn**, when `cargo-kani` is missing and `CI=true` — otherwise a runner without
kani installed would report green-by-absence, re-creating the skip-as-green hole
(F9) at the very moment kani becomes required. Do this in the same change window
as the ruleset edit so the required `kani` context can never pass vacuously.

**Falsifiers:** (a) a PR adding `sorry` to any pilot Lean module → `lake-build`
red, merge blocked; (b) a PR inverting one asserted property in an existing kani
harness → `kani` red.

> `shader-validate` (the PMAT-482 job) is a candidate for the *next* promotion
> round once it has a few stable cycles — not included above to keep this edit to
> proven-stable contexts.

---

## 3. XPILE-SOT-001 (EV rank **10**) — resolve the source-of-truth contradiction

**Not a gate — a governance decision only the owner can make.** The merger
doctrine (`bashrs-merger.md:9`, `v0.2.0-depyler-merger.md`) names the *dormant
standalone* `paiml/depyler` as the "maintenance home," while xpile has
re-implemented its surface in-tree and out-publishes it. Four un-archived
siblings (`depyler`, `decy`, `ruchy`, `bashrs`) can drift the moment anyone
commits to them.

**Pick one:**
- **(a) Execute the declared collapse** — the external repos become ~50-LoC
  re-export shims with archive/frozen notes. *Requires owner approval to touch
  external repos — outside this repo's PR flow.*
- **(b) Amend the doctrine** — declare xpile the maintenance home in
  `v0.2.0-depyler-merger.md` + `migration.md`, and add a standing watch-signal
  (a documented periodic `git ls-remote` sibling-HEAD diff). *This half is an
  in-repo doctrine PR I can draft once you pick (b).*

No code mutation verifies this; verification is reading the amended doctrine and
the repo states. **Tell me which way and I'll draft the in-repo half.**

---

## 4. PMAT-487 (EV rank **13**) — self-hosted GPU runners

**Blocked on hardware/ops access**, not code. Converts the GPU lanes'
local-only witnesses into per-PR CI execution:

- Register self-hosted **sm_89 (RTX 4090)** and **AMD-Vulkan** runners (the
  `intel` + local boxes from the hardware inventory).
- `gpu_witness.rs` executes per-PR on them; add GPU floors to the
  WITNESS-002 manifest.
- Falsifier: corrupt the PTX emitter's fma lowering → GPU witness job red.

Until the runners exist, PMAT-482's offline naga/spirv-val gate is the honest
substitute (validates emission structure on free CI; does not execute on
silicon). The WITNESS-002 manifest already asserts the GPU lanes skip **with
reason** so their absence on hosted runners is loud, not silent.

---

## Not blocked on you (available future work, in normal PR flow)

These ranked items are neither org-gated nor owner-decisions — they're just
below the enforcement tail in EV and can proceed as ordinary PRs when desired:
XPILE-WITNESS-003 (Ruchy execution witness, rank 9), PMAT-1008 (Python aliasing
preservation, rank 11), PMAT-476 (2026-Q3 SOTA dossier — **calendar-bound,
CI-gate due 2026-08-15**, rank 14), and PMAT-985-DICT-ITER (for-in-dict WASM
breadth, rank 15, now unblocked since WITNESS-001 landed).

---

*Generated as the hand-off tail of the autonomous implementation of
`fable-architectural-review.md`. The one action that most changes the repo's
safety posture is item #1 — it is a two-minute org-admin edit that turns seven
already-merged advisory gates into merge-blocking ones.*
