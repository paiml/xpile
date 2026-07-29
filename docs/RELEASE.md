# xpile — release procedure

**Status:** live procedure. This file is cited *by name* from test source
(`crates/xpile/tests/ruleset_drift.rs`) as the place that arms an anti-vacuity
tripwire, and it is the disclosure surface for the deliberate tag/CHANGELOG
date skew. It is not narrative: `crates/xpile/tests/release_preflight_witness.rs`
(XPILE-RELEASE-PREFLIGHT-001) machine-checks that the tripwire set named here
agrees, in both directions, with the tripwire set the test corpus actually
reads.

**No hand-typed counts.** Same rule as [`docs/status/CURRENT.md`](status/CURRENT.md):
every number below is given as the command that derives it. The crate count,
the version and the required-context set all move; a number typed here would
rot exactly like the one that rule exists to prevent.

---

## 0. Why this document exists

`ruleset_drift.rs::live_ruleset_matches_the_committed_snapshot` compares the
committed enforcement snapshot against the live org ruleset. Reading an *org*
ruleset needs a token with org scope, which Actions' repo-scoped
`GITHUB_TOKEN` does not have — so in CI that test legitimately **skips**, and
its skip branch says the release pre-flight is what refuses the skip.

For the whole life of that comment there was no release pre-flight and no such
file, so nothing ever set `XPILE_REQUIRE_RULESET_CHECK` and the enforcement
claim had never once been checked outside an ad-hoc local run. The org ruleset
drifted on 2026-07-27 (`workspace-test` silently dropped from the required
set) and it was a cron fire that happened to hold an org-scoped token, not the
named mechanism, that noticed. PMAT-1416 closed that: the mechanism now
exists, and the gate above keeps it and the code in agreement.

The generalisation the same slice measured: of the `XPILE_REQUIRE_*` tripwires
the corpus reads, only `WASM_RUNTIME`, `KANI` and `DENY` were armed anywhere.
The other five were armed by nothing. §2 is where the remaining five are armed.

---

## 1. Cadence

Releases are cut on a **Friday**, from a tag created the **Thursday before**.

| Day | Action |
|---|---|
| Mon–Wed | Ordinary slices merge to `main`. |
| Wed 18:00 local | **Hard freeze.** No merge may touch `crates/*/src`, `contracts/*.yaml`, or any gate. CHANGELOG text, version, docs and release mechanics only. |
| Wed | **Pre-flight 1** — `gh workflow run release.yml` (workflow_dispatch) against the release-candidate SHA. |
| Thu | Release commit, then **cut and push the tag** on a PINNED SHA. **Pre-flight 2** fires automatically on the tag push. **Pre-flight 3** is the local dry-run. Do **not** create the GitHub release yet. |
| Fri | Re-verify, `gh release create`, then the crates.io batch. **No code merges.** |

### The one-day tag/CHANGELOG date skew is deliberate

`.github/workflows/release.yml` triggers on `push: tags: v*`, so its
`cleanroom-publish` job **cannot gate the tag it fires on**. Pushing the tag on
Thursday converts that job from a post-hoc observation into a real pre-flight
with a full day of runway.

The consequence, stated plainly because it looks like an error otherwise: the
**tag object is created on Thursday** while the CHANGELOG heading for the same
version reads **Friday's date** (the ship date). That is a deliberate one-day
skew, not a mistake, and it is the reason the two disagree in
`git log --format='%ci' -1 <tag>` versus `CHANGELOG.md`.

---

## 2. The pre-flight tripwires

Several witnesses in this repo skip when a tool or a credential is absent.
Every one of them takes an `XPILE_REQUIRE_*` env var that turns the skip into a
hard failure — the anti-vacuity half. A tripwire nobody sets is decorative, so
the release pre-flight is where the ones CI cannot arm get armed.

| Tripwire | Armed by | Tier |
|---|---|---|
| `XPILE_REQUIRE_WASM_RUNTIME` | `.github/workflows/ci.yml` (`workspace-test`, REQUIRED) | already enforced per-PR |
| `XPILE_REQUIRE_KANI` | `.github/workflows/ci.yml` (`kani`, advisory) | already armed in CI |
| `XPILE_REQUIRE_DENY` | `.github/workflows/ci.yml` (`license-scan`, advisory) | already armed in CI |
| `XPILE_REQUIRE_CC` | this pre-flight | **blocking** |
| `XPILE_REQUIRE_SH` | this pre-flight | **blocking** |
| `XPILE_REQUIRE_RUCHY` | this pre-flight | **blocking** |
| `XPILE_REQUIRE_CHANGELOG_HISTORY` | this pre-flight | **blocking** |
| `XPILE_REQUIRE_RULESET_CHECK` | this pre-flight | **record, do not block** — see below |

### 2a. Blocking tripwires — must exit 0

Run from a checkout **at the tag** (§4 step 3), with `git` history present:

```bash
XPILE_REQUIRE_WASM_RUNTIME=1 \
XPILE_REQUIRE_CC=1 \
XPILE_REQUIRE_SH=1 \
XPILE_REQUIRE_RUCHY=1 \
XPILE_REQUIRE_CHANGELOG_HISTORY=1 \
  cargo test --workspace
```

Unpiped. A non-zero exit is abort rule **A1**.

`XPILE_REQUIRE_RUCHY` needs `ruchy` on `PATH` and `XPILE_REQUIRE_CC` needs a
real `cc`/`gcc` — note that an interactive shell aliasing `cc` to something
else does not satisfy the second; the witness resolves the binary, not the
alias. If a tool is genuinely unavailable on the release host, that is a
**host** defect: fix the host, do not drop the flag. Dropping the flag is the
skip-green this section exists to prevent.

### 2b. `XPILE_REQUIRE_RULESET_CHECK` — run it, record it, do not block on it

```bash
gh auth status   # must show org read scope; `gh auth login` if not
XPILE_REQUIRE_RULESET_CHECK=1 cargo test -p xpile --test ruleset_drift
```

This compares the committed `docs/status/ruleset-*.json` receipts against the
live `gh api repos/paiml/xpile/rules/branches/main` — the union over every
ruleset that protects the branch. It is **not** a publish blocker, for one
reason: the rulesets are owner-controlled state outside this repo, so a red here
reports someone else's change, not a defect in the artifact being shipped.

**It is also not optional.** Its result goes in the release body verbatim. As of
2026-07-29 it is **GREEN**: `main` is blocked by `gate` (ruleset `13878864`) and
`workspace-test` (ruleset `19814559`).

Two rules when it goes red, and the second one is new because ignoring it cost
two days (PMAT-1475):

1. Do **not** re-derive a receipt to make it green. A receipt rewritten to match
   a mutation it was supposed to detect is worse than a red one, and this repo
   has already been misled for three weeks by exactly that (the 2026-07-05
   flip).
2. Do **not** conclude a weakening from a single ruleset. Between 2026-07-27 and
   2026-07-29 this check was red because `workspace-test` had been **moved** out
   of ruleset `13878864` into a new dedicated ruleset `19814559`; the effective
   protection on `main` never changed. It was read as a dropped requirement,
   escalated as an owner decision, and three documents were edited to claim
   *less* enforcement than the repo actually has. **Read
   `gh api repos/paiml/xpile/rules/branches/main` before you believe anything
   about what blocks a merge** — a per-ruleset read cannot answer that question
   and never could.

---

## 3. Pre-flight 1 — Wednesday clean-room dispatch

```bash
gh workflow run release.yml --ref main
gh run list --workflow release.yml --limit 1 --json conclusion,headSha
```

`cleanroom-publish` runs `cargo publish --workspace --dry-run --locked` under
an isolated, empty `CARGO_HOME`, so no sibling `[patch.crates-io]` path
override and no cached crate can satisfy a dependency the real registry could
not. Advisory tier — it is not a required status check, so **read its
conclusion; a green `gh pr checks` does not include it.**

---

## 4. Thursday — the release commit and the tag

1. **Bump the version.** It is single-sourced at `[workspace.package] version`
   in `Cargo.toml`, but the bump is **not one line**: every intra-workspace
   path dep in `[workspace.dependencies]` repeats the number, because a
   `[workspace.dependencies]` entry cannot inherit from `[workspace.package]`.
   Do a global replace of the old version string across `Cargo.toml`, then
   `cargo check --workspace` to refresh the `Cargo.lock` entries.
   `crates/xpile/tests/publish_manifest_integrity.rs` reds a bump that touched
   only `[workspace.package]`.

2. **Promote the CHANGELOG heading** from `## [Unreleased]` to
   `## [<version>] - <FRIDAY's date>` (see the skew note in §1). The section
   must carry all three of *What still REFUSES*, *What is NOT merge-blocking*
   and *Known divergences*. The merge-blocking section states the live
   required set and the advisory set — read them from the
   `XPILE-ENFORCEMENT REQUIRED-CONTEXTS:` and
   `XPILE-ENFORCEMENT ADVISORY-CONTEXTS:` markers in
   `.github/workflows/ci.yml`; do not retype either list.

3. **Re-derive the witness floors** (`crates/xpile/tests/witness_floor.rs`) for
   every lane. Floors are lower bounds with headroom, not tracking equalities.

4. **Run the pre-flight tripwires** — §2a unpiped, then §2b recorded.

5. **Purge the stale package overlay, then dry-run locally.** The overlay is
   the documented `no hash listed for <crate> vX` killer and it survives
   between releases:

   ```bash
   # tmp-registry and tmp-crate are the two that cause the failure; the
   # per-crate `<name>-<version>` dirs from previous releases accumulate
   # beside them and the whole directory is a rebuildable cache, so clear it
   # wholesale rather than with a version glob that rots at the next minor.
   rm -rf /mnt/nvme-raid0/targets/xpile/package
   cargo publish --workspace --dry-run --locked
   ```

   Unpiped. `/mnt/nvme-raid0/targets/xpile` is this host's target dir (repo
   `.cargo/config`); on another host, derive it rather than copying the path.

6. **Tag a PINNED SHA and push.**

   ```bash
   git tag "v<version>" "<PINNED_SHA>"
   git push origin "v<version>"
   ```

   **Do not create the GitHub release yet.** The tag push fires pre-flight 2
   (`cleanroom-publish` on the artifact that will actually be published).

---

## 5. Friday — publish

**No code merges. None.** See abort rule A7.

1. **Re-verify A1** (§6). All preconditions, before the first upload byte.
2. **Record the advisory statuses** on the pinned SHA and name every
   non-success in the release body:

   ```bash
   gh api "repos/paiml/xpile/commits/<PINNED_SHA>/check-runs" \
     --jq '.check_runs[] | "\(.name) \(.conclusion)"'
   ```

   `license-scan` green means *the licence surface still equals `NOTICE.md`* —
   it does **not** mean there is no licence problem. `cargo deny check
   licenses` exits 4 by design and the LGPL-in-the-shipped-binary owner
   decision is open. A release note saying otherwise would be the exact class
   of claim this procedure exists to prevent.
3. **Create the release from an EXTRACTED body, never a hand-assembled one.**

   ```bash
   V=<version>
   awk -v h="## [$V]" 'index($0,"## [")==1 { f = (index($0,h)==1) } f' \
     CHANGELOG.md > /tmp/relbody.md
   gh release create "v$V" --title "v$V" --notes-file /tmp/relbody.md   # non-draft
   diff /tmp/relbody.md <(gh release view "v$V" --json body -q .body)
   ```

   It is `index()` and not a regex on purpose: a *dynamic* awk regex cannot
   carry `\[`, and the escaped form
   (`awk -v h="^## \\[$V\\]" '… $0 ~ h …'`) degrades `[0.1.618]` into a
   **character class**, matches nothing, and writes a **zero-byte body at
   exit 0** — a silent empty release note. Do not "simplify" it back.

   The `diff` is the point, not decoration. Through `v0.1.617` this step said
   only *"body matching the CHANGELOG section"*, and **that match was measured
   for the first time on 2026-07-30 (PMAT-1480), over all 613 published
   releases: 0 matched.** Only 1 of the 613 bodies even begins with its section
   heading, all 613 carry at least one line found nowhere in `CHANGELOG.md`
   (3,044 lines in total), and `v0.1.616` shipped a 1,364-character body against
   a 24,725-character section — 5.5% of the story. So the sentence was not a
   description of what the procedure did; it was a description of what nobody
   had checked.

   **The release page is a publisher of this repository's facts that no gate in
   `crates/xpile/tests/` can see.** Every one of them derives its corpus from the
   working tree or from `git ls-files`, and a GitHub release body is in neither.
   Two consequences, both measured on `v0.1.617`:

   - **A CHANGELOG correction does not reach the page.** PMAT-1479 rewrote the
     `;&` / `;;&` entry because it scoped a false universal to two shapes when
     twenty were accepted. The published body still carries the pre-correction
     sentence, so a reader who never opens this repository still re-derives the
     claim that fix exists to kill. Repairing one publisher of a shared fact does
     not repair the fact.
   - **A page-only disclosure does not reach the repository.** The `v0.1.617`
     body carries a 29-line *"Post-release correction (2026-07-26, PMAT-1370):
     the `wasi` job was RED on this SHA"* section that existed **nowhere** in
     `CHANGELOG.md` until PMAT-1480 ported it in. `git grep` could not find the
     repo's own most load-bearing release disclosure. Corpus-wide this is the
     norm, not an outlier: 212 bodies are longer than their section, and the
     enforcement claim *"Full gate green: fmt, clippy -D warnings, check,
     workspace tests, pv lint (0 errors)"* appears on 25 release pages and in no
     CHANGELOG section.

   **Therefore: a post-release correction is written to `CHANGELOG.md` first and
   mirrored to the page second, never the other way round**, and the mirror is
   an append with its own date and PMAT id so the original text stays legible as
   what was published at the time.
4. **Publish, unpiped, from a worktree checked out AT THE TAG:**

   ```bash
   cargo publish --workspace
   ```

   Cargo auto-orders the DAG. No manual order file is needed: every
   intra-workspace path dep carries an explicit `version =`, statically
   checked per-PR by `publish_manifest_integrity.rs`.
5. **Verify — with a `User-Agent`.**

   ```bash
   for c in $(cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name'); do
     printf '%s ' "$c"
     curl -s -H "User-Agent: xpile-release-verify" \
       "https://crates.io/api/v1/crates/$c/<version>" | jq -r '.version.num // "MISSING"'
   done
   ```

   ⚠️ **Without the `User-Agent` header the crates.io API returns an error
   body**, so the loop reports every crate `MISSING` on a fully successful
   publish. This fired on v0.1.617. Cross-check against
   `https://index.crates.io/<a>/<b>/<name>`. When a verifier contradicts a
   green exit code on an irreversible operation, **debug the verifier first**:
   uniform failure across N independent items means the harness is broken, not
   the operation.

---

## 6. Abort rules

- **A1 — PRECONDITIONS.** Publish only if ALL hold: the tag exists on origin;
  its SHA shows every REQUIRED context SUCCESS; the on-tag clean-room run is
  SUCCESS; the local dry-run exited 0 unpiped; the §2a tripwire run exited 0 on
  that exact SHA; the release worktree is at the **tag** (not `main`) and
  `git status --porcelain` is empty. Any failure ⇒ do not publish.
- **A1b — DRIFT.** If the SHA that was *tested* differs from the SHA being
  *tagged* (the cron merged something), re-run the full suite on the new SHA or
  abort the day. `strict_required_status_checks_policy` is `false`, so green
  checks do not prove the merged combination was ever tested together.
- **A2 — NEVER PIPE.** `cargo publish … | tail && echo OK` masks the failure
  exit. Never `--no-verify`.
- **A3 — TAG FAILURE (pre-publish).** Do **not** delete and do **not** move the
  tag — destructive git is owner-gated. Fix on `main`, bump the patch number,
  re-tag. A burned patch number is free.
- **A4 — PARTIAL BATCH.** If crate *k* fails mid-publish: **STOP IMMEDIATELY.**
  crates.io versions are immutable. Do **not** yank the *k−1* already
  published (they are valid). Do **not** retry at the same version (409). Do
  **not** bump a single crate to route around it. Record exactly which crates
  landed in §7, then fix forward with a **whole-workspace** patch bump the
  following Friday.
- **A5 — OVERLAY (recoverable, do NOT abort).** `no hash listed for <crate> vX`
  is the stale local package overlay, not a real failure: purge per §4 step 5
  and restart from the dry-run. Without this rule A4 would fire on a fully
  recoverable condition and needlessly kill the release.
- **A6 — HARD TIME-BOX.** If `cargo publish --workspace` has not **started** by
  16:00 local Friday, abort the crates.io step entirely: ship
  GitHub-tag-and-release only (a tagged, unpublished release is an acceptable
  outcome) and roll the batch to the following Friday. Never begin an
  irreversible whole-workspace batch into an evening with no recovery window.
- **A7 — NO FRIDAY CODE.** If a defect surfaces Friday morning the answer is
  A6 (ship the tag, defer the batch), never a same-day hotfix onto the release
  SHA.
- **A8 — CONCURRENCY.** All release work happens in an isolated `git worktree`;
  re-check `git rev-parse origin/main` against the recorded high-water mark
  before every push; the release SHA is **pinned** and never re-resolved as
  "whatever `HEAD` is."

---

## 7. Slip and partial-batch ledger

Every A4 stop and every A6 slip is recorded here, with the reason, on the day
it happens. An unrecorded slip is indistinguishable from a forgotten release.

| Date | Version | What happened | Reason |
|---|---|---|---|
| — | — | no slip or partial batch recorded to date | — |
