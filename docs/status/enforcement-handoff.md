# Enforcement hand-off — the org-gated tail of the fable architectural review

> **STATUS 2026-07-29 (PMAT-1475) — re-derived against the live API.** Two
> org rulesets protect `main`, and the merge-blocking set is their **union**:
> `13878864` supplies `gate`, `19814559` ("workspace-test — repos that emit it")
> supplies `workspace-test`. Derive it with
> `gh api repos/paiml/xpile/rules/branches/main`, never from one ruleset —
> `19814559` was split out on 2026-07-27 and a per-id read misreported that as
> `workspace-test` being dropped.
>
> <!-- XPILE-ENFORCEMENT REQUIRED-CONTEXTS: gate, workspace-test -->
> **required (merge-blocking): `gate`, `workspace-test`.**
> **advisory (run every PR, red on regression, do NOT block a merge):**
> `docs`, `kani`, `lake-build`, `lean-models`, `license-scan`,
> `shader-validate`, `wasi`.
>
> XPILE-RULESET-001 ✅ is DONE and HELD (`workspace-test` is enforced).
> XPILE-RULESET-002 is **NOT** done: its ruleset half was applied on
> 2026-07-05 at 17:35 and **reverted the same day at 23:50**, and this file
> asserted the four-context set for three weeks afterwards because a committed
> JSON snapshot cannot notice the API moving underneath it. The *repo* half of
> RULESET-002 ✅ **did** ship — `crates/xpile/tests/kani_verify.rs:149`'s
> `XPILE_REQUIRE_KANI` tripwire (#1885) — but it hardens a lane that is
> advisory, so a red proof job still does not block a merge.
>
> Snapshots: one receipt per ruleset —
> [`ruleset-13878864.json`](ruleset-13878864.json) and
> [`ruleset-19814559.json`](ruleset-19814559.json) — pinned by
> `crates/xpile/tests/ruleset_drift.rs` (XPILE-RULESET-DRIFT-001) against both
> the marker lines above and — when a token with org scope is present — the
> live API. **Still open:** the RULESET-002 org-admin RE-FLIP (owner-gated,
> tracked as the `ruleset-reflip` owner decision in
> [`docs/roadmaps/queue.yaml`](../roadmaps/queue.yaml)), the SOT-001 governance
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
not merge-blocking. **That half is fixed and held:** `workspace-test` is a
required context today, so all seven are merge-blocking. What is *not* fixed is
the proof lane — `kani` and `lake-build` are advisory (see the STATUS banner).

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

> **⚠️ CORRECTION 2026-07-26 (PMAT-1347).** The two paragraphs above describe
> the edit **as applied on 2026-07-05 at 17:35**, and both verifications were
> true at that moment. They are no longer true of `kani` + `lake-build`: the
> ruleset was edited again at **23:50 the same day** and those two contexts were
> removed. `workspace-test` survived; the proof lane did not. The paragraphs are
> kept verbatim rather than rewritten because the *procedure* (org endpoint, the
> `jq` transform, the repo-endpoint 404 gotcha) is still the correct runbook for
> the re-flip — only the claimed outcome was overtaken. `strict` is still
> `false`, which is load-bearing for release abort rule **A1b**: green checks do
> not prove the merged combination was ever tested together.
>
> **The lesson, recorded because it generalises past this file:** a receipt for
> a mutation of an external system decays silently. `ruleset_drift.rs` now
> re-derives this claim from the live API instead of trusting the receipt.

---

## 2. XPILE-RULESET-002 (EV rank **4**) — promote the proof lane ⚠️ REVERTED — repo half done, ruleset half OPEN

**Corrected 2026-07-26 (PMAT-1347).** This section previously read "✅ ruleset
half DONE". It is not. `lake-build` and `kani` *were* added in the same edit as
#1 on 2026-07-05 at 17:35 (verified green on 5/5 recent completed commits
before promoting; lake-build ~16s, kani ~2m — latency-free), and were **removed
again at 23:50 the same day**. The live required set is `[gate,
workspace-test]`. An advisory proof lane compounds no confidence, and that is
the state we are in: **a red `kani` or `lake-build` does not block a merge.**

**Repo-side companion ✅ SHIPPED** (#1885): `crates/xpile/tests/kani_verify.rs:149`
fails rather than warns when `cargo kani` is not invocable and
`XPILE_REQUIRE_KANI` is set — the `kani` job sets it after installing
kani-verifier. Deliberately keyed on that env var and **not** on `CI=true`,
which would wedge the required `workspace-test` job on every runner without
kani. This closes the skip-as-green hole (F9) *within* the lane; it does not
make the lane blocking.

**Still open — the org-admin RE-FLIP, and it is OWNER-GATED.** Re-adding `kani`
+ `lake-build` requires a `PUT` to `orgs/paiml/rulesets/13878864` (the repo
endpoint 404s for an org-sourced ruleset) — outside normal repo-push rights and
outside what the autonomous loop may do. It is tracked as the `ruleset-reflip`
entry under `owner_decisions` in [`docs/roadmaps/queue.yaml`](../roadmaps/queue.yaml).
The runbook in §1 above is still the correct procedure. Until it is applied,
`ruleset_drift.rs` keeps the *description* honest rather than keeping the policy
right — those are different things, and only the owner can do the second.

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
`fable-architectural-review.md`; enforcement claims re-derived against the live
API on 2026-07-26 (PMAT-1347). The one action that most changes the repo's
safety posture is now item **#2** — the same two-minute org-admin edit, re-applied,
this time turning the **proof lane** (`kani` + `lake-build`) from advisory into
merge-blocking. Item #1's half of that edit is live and holding.*
