# Enforcement hand-off — the org-gated tail of the fable architectural review

> **STATUS 2026-07-05 — the flip is LIVE.** Org ruleset `13878864` now requires
> `['gate', 'kani', 'lake-build', 'workspace-test']` (was `['gate']`).
> XPILE-RULESET-001 ✅ and the ruleset half of XPILE-RULESET-002 ✅ are DONE — the
> seven merged gates plus the proof lane are now **merge-blocking**, not advisory.
> Snapshot: [`ruleset-13878864.json`](ruleset-13878864.json). Still open:
> RULESET-002's `kani_verify` hard-fail companion PR, the SOT-001 governance
> decision, and PMAT-487 (GPU runners).

The EV-ranked backlog in [`docs/specifications/fable-architectural-review.md`](../specifications/fable-architectural-review.md)
§7b was implemented autonomously **as far as the PR flow reaches**. Seven gate
PRs landed (see the table below), and the org-admin ruleset flip that makes them
enforce has now been applied. What remains needs an **owner governance
decision** (SOT-001) or **hardware/ops access** (PMAT-487). This file is the
runbook + record for that tail.

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

All seven run under the `workspace-test` (or `gate`) job. Until 2026-07-05 the
ruleset required only `gate`, so they were advisory-green — red on a bad PR but
not merge-blocking. **That is now fixed (see the STATUS banner).**

---

## 1. XPILE-RULESET-001 (EV rank **1**) — make the gates actually enforce ✅ DONE 2026-07-05

**Why it was #1:** every gate inherits its meaning from enforcement. Before the
flip, ruleset `13878864` ("Green Main") required **only** the `gate` context —
which runs fmt/check/clippy/pv-lint/deny but **not** the differential witnesses,
the witness-floor manifest, or the claims-drift gate (those are `workspace-test`).
So all seven PRs above were advisory-green. The flip added `workspace-test`
(this item) plus `lake-build` + `kani` (item 2) to the required contexts.

**⚠️ endpoint gotcha:** ruleset `13878864` is **Organization-sourced**
(`source_type: Organization`, `source: paiml`), so the repo endpoint
`repos/paiml/xpile/rulesets/13878864` can **read** it but a `PUT` there returns
**404**. Edit it at the **org** endpoint `orgs/paiml/rulesets/13878864` (requires
org-admin, which is outside normal repo-push rights).

**The edit as applied (idempotent — re-running is a no-op):**

```bash
# Read from the ORG endpoint, add the 3 contexts, PUT back to the ORG endpoint.
gh api orgs/paiml/rulesets/13878864 \
  | jq '{name, target, enforcement, conditions, bypass_actors,
         rules: [ .rules[]
           | if .type=="required_status_checks"
             then .parameters.required_status_checks =
                  ( [ .parameters.required_status_checks[].context ]
                    + ["workspace-test","lake-build","kani"]
                    | unique | map({context: .}) )
             else . end ] }' > /tmp/ruleset-new.json

gh api --method PUT orgs/paiml/rulesets/13878864 --input /tmp/ruleset-new.json

# Verify the EFFECTIVE gate on main:
gh api repos/paiml/xpile/rules/branches/main   # → required now includes workspace-test
```

Then decide `strict` explicitly: `strict_required_status_checks_policy: true`
forces branches to be up-to-date with `main` before merge (safer, more rebases);
`false` (today) does not. The review recommends deciding it explicitly, not by
default.

**Enforcement verified two ways:** (1) `gh api repos/paiml/xpile/rules/branches/main`
now lists `workspace-test` among the effective required contexts; (2) this very
PR — the one committing `ruleset-13878864.json` — had to pass all four required
checks (`gate`, `workspace-test`, `lake-build`, `kani`) before it could merge, a
positive proof that the context names are wired correctly (a misnamed required
context would hang the PR unmergeable forever, not block-then-pass). The snapshot
is committed at [`ruleset-13878864.json`](ruleset-13878864.json). `strict` was
left **false** deliberately — requiring branches be up-to-date with `main` before
merge would add rebase churn to the active autonomous cron; promote to `strict`
only if stale-base merges become a real problem.

---

## 2. XPILE-RULESET-002 (EV rank **4**) — promote the proof lane ✅ ruleset half DONE 2026-07-05

The ruleset half is done — `lake-build` and `kani` were added in the same edit as
#1 (verified green on 5/5 recent completed commits before promoting; lake-build
~16s, kani ~2m — latency-free). An advisory proof lane compounds no confidence.

**Still open — one repo-side companion PR** (normal branch→PR flow): make
`contracts/kani/kani_verify.rs` **fail, not warn**, when `cargo-kani` is missing
and `CI=true` — otherwise a runner without kani installed would report
green-by-absence, re-creating the skip-as-green hole (F9) now that `kani` is a
required context. Today's CI *does* run kani (it reports success, not skip), so
the promotion is safe; this companion hardens against a future runner-config
change making the required `kani` pass vacuously. **This is the one sub-item I can
still do as an ordinary PR — say the word.**

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

These ranked items are neither org-gated nor owner-decisions — they sit below the
enforcement tail in EV and proceed as ordinary PRs. Status as of 2026-07-05:

- **XPILE-WITNESS-003** (rank 9) — ✅ **DONE, and widened past its original Ruchy
  scope.** All three string-compare-only backends now have real behavioural
  witnesses, each floored against silent deletion in the XPILE-WITNESS-002
  manifest:
  - *ruchy* — `ruchy_exec_witness.rs` drives `xpile → ruchy transpile → rustc → run`
    and byte-diffs vs CPython on 7 curated fixtures (the honest ceiling: `ruchy`
    v4.2.1 parses only 16/34 and runs 5/34; every fixture that runs matches
    CPython, so the gap is coverage, not correctness) (#1893, floored #1894).
  - *lean* — `lean_elaborate_witness.rs` emits Lean and appends a
    `example : f args = v := by decide` obligation per function, so a wrong
    emission fails Lean's decider (6 functions / 11 obligations) (#1896). The
    citation-attribute finding was corrected after adversarial re-check: the
    `@[xpile_contract]` attribute is DELIBERATE, and the fix is a registration
    prelude, not a comment (#1899).
  - *forjar* — `forjar_validate_witness.rs` runs forjar's OWN `validate` on the
    emitted YAML for 4 shell shapes. This **caught a real bug**: the emit carried
    `machines.*.addr` but no `hostname`, so `forjar validate` rejected *every*
    config while the structural test stayed green — fixed (#1900), backstopped
    in-CI (#1901).
  - floors for forjar + lean added to the witness manifest (#1902).
- **PMAT-476** (rank 14) — ✅ **DONE.** The 2026-Q3 SOTA dossier is published
  (audit-design.md §7, 2026-06-12), the `sota_dossier_deadline.rs` gate is green,
  and the deadline line is bumped to the 2026-Q4 slot (2026-11-15). Nothing due
  until then.
- **PMAT-1008** (rank 11) — OPEN. Python aliasing / value-vs-reference
  preservation. Contained today by the alias-then-mutate clean-reject stopgap;
  it is correctness-grind work the owner has explicitly de-prioritised relative
  to the provable-contract regime, so it is not being pushed autonomously.
- **PMAT-985-DICT-ITER** (rank 15) — IN PROGRESS on the WASM lane (for-in-dict
  iteration; `for k in d` / `.keys()` / `.values()` / `.items()` landed on the
  active WASM-surface stream).

---

*Generated as the hand-off tail of the autonomous implementation of
`fable-architectural-review.md`. The one action that most changes the repo's
safety posture is item #1 — it is a two-minute org-admin edit that turns seven
already-merged advisory gates into merge-blocking ones.*
