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

The one permitted form, because PMAT-1483 added some: a **dated measurement**
carrying the command that reproduces it, framed in the past tense about a named
subject (*"measured 2026-07-30 on `v0.1.617`"*). Those are honest by framing —
they say what was true of one thing at one time. What is banned is the same digit
written as a **standing expectation** (*"the section is 80,000 characters"*,
*"expect 4 hits"*), which reads as checked and rots invisibly. §5 step 3's size
table is the first kind; if you find yourself unable to date one, it is the second.

**⚠️ READ THIS FILE FROM `origin/main`, NOT FROM THE RELEASE WORKTREE.**
§2a, §5 step 4 and abort rule A1 all put the operator in a worktree pinned to
the **tag**. This document is version-controlled *inside* that tree, so it
arrives there frozen at whatever it said when the tag was cut — and §1's freeze
table permits a docs-lane merge on Thursday and Friday, precisely the window in
which this file is most likely to be corrected. Every such fix is absent from
the copy the pinned worktree hands you.

Measured on `v0.1.618`, and the interval is the point — the tag object was
created at `2026-07-30 00:42:18 +0200` and the commit carrying the extracting
form of §5 step 3 landed at `01:22:16`, **forty minutes later, the very next
merge.** So `git show v0.1.618:docs/RELEASE.md` still carries the superseded
two-line step 3 whose body rule was *"body matching the CHANGELOG section"* — the
exact sentence that measured 0-of-613 and the reason step 3 was rewritten. An
operator who did what A1 says and read the runbook out of the release worktree
would have re-committed the defect that had been fixed forty minutes earlier.
Staleness here is not measured in days; one merge is enough.

**A runbook that instructs you to work from a pinned historical tree cannot
assume it will be read from that tree.** Before starting, materialise the
current one:

```bash
git -C <repo> show origin/main:docs/RELEASE.md > /tmp/RELEASE.md   # follow THIS copy
```

---

## 0. Why this document exists

`ruleset_drift.rs::live_ruleset_matches_the_committed_snapshot` compares the
committed enforcement snapshot against the live org ruleset. Reading an *org*
ruleset needs a token with org scope, which Actions' repo-scoped
`GITHUB_TOKEN` does not have — so in CI that test legitimately **skips**, and
its skip branch says the release pre-flight is what refuses the skip.

For the whole life of that comment there was no release pre-flight and no such
file, so nothing ever set `XPILE_REQUIRE_RULESET_CHECK` and the enforcement
claim had never once been checked outside an ad-hoc local run. On 2026-07-27 an
org ruleset changed underneath the repo, and it was a cron fire that happened to
hold an org-scoped token, not the named mechanism, that noticed. PMAT-1416
closed that: the mechanism now exists, and the gate above keeps it and the code
in agreement.

**What that 2026-07-27 change actually was — stated here because this section
had it wrong until PMAT-1483.** The sentence above used to end *"(`workspace-test`
silently dropped from the required set)"*. Nothing was dropped. `workspace-test`
was **moved** into a second org ruleset, and `main` was blocked by both contexts
before, during and after; §2b rule 2 carries the full account. That misreading is
the one that cost two days and three documents edited to claim *less* enforcement
than the repo has (PMAT-1475) — and it sat here, in the section a Friday operator
reads first, for a day after §2b was written to forbid it. **The detected event
was real; the consequence written beside it was not.** A per-ruleset read can
report a change; only `gh api repos/paiml/xpile/rules/branches/main` can say
whether anything stopped blocking a merge.

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
   and *Known divergences* — **and, because the release page is the only
   non-clone path to this file, so must the published body: §5 step 3 carves
   them out of the tail and budgets around them rather than letting a size cut
   decide (PMAT-1484).** The merge-blocking section states the live
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

6. **Measure every published metadata URL — BEFORE the tag, because after it
   they are immutable (PMAT-1487).**

   `description`, `homepage` and `documentation` are uploaded verbatim with
   every crate and rendered in the crates.io sidebar. `crate_metadata_honesty.rs`
   (XPILE-CRATEMETA-001) reads the `description`s **in the working tree**.
   **Nothing reads the URLs, and nothing reads what the registry is currently
   serving for any of the three** (PMAT-1489) — those are different questions,
   and arm 3 below is the only one that asks the second.
   This is the one metadata check that can still be *repaired* — §5 step 6 runs
   after the upload and can only disclose — so it belongs here, ahead of the
   point of no return.

   ```bash
   command cargo metadata --no-deps --format-version 1 \
     | jq -r '.packages[] | [.name, (.documentation // "-"), (.homepage // "-")] | @tsv' \
     | while IFS=$'\t' read -r name doc home; do
         for u in "$doc" "$home"; do
           [ "$u" = "-" ] && continue
           c=$(curl -s -o /dev/null -w '%{http_code}' -L \
                 -H 'User-Agent: xpile-release-verify' "$u")
           case "$c" in 200) ;; *) echo "DEAD $c $name $u — A12" ;; esac
         done
       done

   # THE SECOND ARM IS NOT OPTIONAL: a docs.rs URL answers 200 with a metadata
   # shell even when the build FAILED, so the status code above acquits it.
   # docs.rs only documents a `lib` target, and a bin-only package can never
   # have one. Ask docs.rs directly, and only for crates that name it.
   command cargo metadata --no-deps --format-version 1 \
     | jq -r '.packages[] | select((.documentation // "") | test("docs\\.rs"))
              | [.name, ((.targets | map(.kind[]) | index("lib")) != null)] | @tsv' \
     | while IFS=$'\t' read -r name haslib; do
         s=$(curl -s -H 'User-Agent: xpile-release-verify' \
               "https://docs.rs/crate/$name/<previous-version>/status.json")
         echo "$name has_lib=$haslib $s"
         case "$s" in *'"doc_status":false'*)
           echo "DEAD-DOCS $name claims docs.rs and does not build there — A12" ;;
         esac
       done

   # ARM 3 — THE OTHER DIRECTION (PMAT-1489). Arms 1-2 ask whether the TREE's
   # claims are dead. This asks whether the REGISTRY is still serving prose the
   # tree already retired. `crate_metadata_honesty.rs` structurally cannot see
   # this: it reads the tree, so it went green the instant the tree was
   # corrected, while the falsehoods it was written to catch stayed published.
   # BOTH SIDES MUST DECODE THE SAME WAY — see the escape note below. Never
   # `@tsv` the description and never regex it out of the TOML; take each side
   # through a single `jq -r`.
   md=$(command cargo metadata --no-deps --format-version 1)
   echo "$md" | jq -r '.packages[].name' | while read -r name; do
     tree=$(echo "$md" | jq -r --arg n "$name" \
              '.packages[] | select(.name == $n) | .description // ""')
     live=$(curl -s -H 'User-Agent: xpile-release-verify' \
              "https://crates.io/api/v1/crates/$name" \
            | jq -r '.crate.description // ""')
     [ "$live" = "$tree" ] || echo "STALE $name — A13"
   done
   ```

   ⚠️ **A `200` from a documentation host is not a rendered document**, and that
   is the whole reason this step has two arms. `https://docs.rs/xpile` returns
   `200` at every version; the page it returns is the dependency/version sidebar
   with no documentation in it, because `cargo rustdoc --lib` failed with
   `error: no library targets found in package "xpile"`. Only `status.json` —
   the signal docs.rs computes for its own badge — distinguishes the two. Same
   shape as §5 step 6's `200`-to-the-wrong-object pair: **check what the URL
   serves, not what it answers.**

   Three controls, executed 2026-07-30 against the live hosts, each mutation
   `diff`-ed against the original before its result was believed:

   | control | mutation | result |
   |---|---|---|
   | red half | `documentation` restored to `https://docs.rs/xpile` | arm 2 fires `DEAD-DOCS`; **arm 1 stays silent** |
   | over-refusal | `xpile-core` (*has* a lib) given `documentation = https://docs.rs/xpile-core` | `has_lib=true`, `doc_status:true`, no finding |
   | arm 1 live | `homepage` pointed at a nonexistent repo path | `DEAD 404 … — A12` |

   The first row is the reason there are two arms and not one: **arm 1 acquits
   the exact defect this step was written to catch.** That is measured, not
   argued. The second row is why the rule is conditional rather than a ban on
   docs.rs, and the third is the anti-vacuity control for arm 1 — on a clean
   tree both arms print nothing, and nothing is also what a broken checker
   prints.

   ⚠️ **Do not read a red here as an xpile-specific defect before checking the
   population.** Bin-only crates failing on docs.rs is the ecosystem norm —
   `ripgrep`, `fd-find`, `hyperfine` and `sd` are all `doc_status:false`
   (measured 2026-07-30). What separates them is the *claim*: `hyperfine` and
   `sd` set no `documentation` key at all and `ripgrep` points its at GitHub.
   Only `fd-find` makes the same broken claim xpile did. **An acquittal for the
   artefact class is not an acquittal for the artefact** — the failure was
   normal, the assertion pointing at it was not, and "everyone's is red" is
   exactly the reasoning that kept this unexamined for 617 releases.

   **Arm 3's controls** (PMAT-1489), executed 2026-07-30, tree restored and
   `git diff --stat` checked after each:

   | control | mutation | result |
   |---|---|---|
   | red half | `xpile-core` (registry matches tree) description prefixed `MUTATED — ` | count 7 → 8, `STALE xpile-core` named |
   | over-refusal | none — `xpile-latex-contract-backend`, whose description contains a `\` escape | **silent**, correctly (its two sides are byte-equal) |
   | cross-check | independent Python implementation decoding TOML via `cargo metadata` | same 7, same names |

   ⚠️ **THE ESCAPE LANDMINE BIT TWICE, IN TWO DIFFERENT TOOLS, AND BOTH TIMES IT
   INFLATED THE COUNT BY THE SAME CRATE.** A first pass regexed `description`
   straight out of the TOML and reported **8**; `jq … | @tsv` then re-escaped the
   backslash in `\xpileContract` and also reported **8**. `@tsv` emits `\\`,
   `read -r` does not interpret it, and the live side came through a plain
   `jq -r` that did — so the two sides were decoded by different rules and a
   byte-identical pair compared unequal. **Both sides must go through one
   `jq -r`.** The direction of the error is what matters: this over-reports, and
   an over-report here puts a crate's name in a release body asserting its
   published description is false when it is not. A disclosure rule's false
   positive is itself a published falsehood.

7. **Tag a PINNED SHA and push.**

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
3. **Create the release from an EXTRACTED body, never a hand-assembled one —
   check its SIZE, because from `v0.1.618` on it does not fit, and cut it in
   the MIDDLE, because the three mandatory disclosure sections are its TAIL.**

   ```bash
   V=<version>
   CAP=125000   # MEASURED, not read off a doc page — see the derivation below.
   # THE TREE IS PART OF THE RULE — extract from the TAG, never from the
   # working tree. `git show "v$V":CHANGELOG.md` is what shipped.
   git show "v$V":CHANGELOG.md \
     | awk -v h="## [$V]" 'index($0,"## [")==1 { f = (index($0,h)==1) } f' > /tmp/relsection.md
   test -s /tmp/relsection.md || { echo "ABORT: empty section"; exit 1; }
   # `wc -m` and NOT `wc -c`: the cap counts CHARACTERS, `-c` counts bytes.
   FULL_C=$(wc -m < /tmp/relsection.md); FULL_L=$(wc -l < /tmp/relsection.md)

   # WHICH LINES SURVIVE IS A SEPARATE DECISION FROM HOW MANY. The three
   # mandatory sections (§4 step 2) are the TAIL of every release section, so a
   # leading-prefix cut drops exactly the disclosure a non-cloning reader came
   # for. Carve them out FIRST and budget the prefix around them.
   MAND_L0=$(grep -n '^### What still REFUSES$' /tmp/relsection.md | tail -1 | cut -d: -f1)
   [ -n "${MAND_L0:-}" ] || { echo "ABORT: no 'What still REFUSES' heading — A10"; exit 1; }
   tail -n +"$MAND_L0" /tmp/relsection.md > /tmp/relmand.md
   grep -q '^### What is NOT merge-blocking$' /tmp/relmand.md &&
   grep -q '^### Known divergences$' /tmp/relmand.md ||
     { echo "ABORT: mandatory sections are not all at the tail — A10"; exit 1; }
   MAND_C=$(wc -m < /tmp/relmand.md); MAND_L=$(wc -l < /tmp/relmand.md)

   # THE POST-TAG DELTA IS A FACT ABOUT THIS BODY, so it is built HERE — before
   # the budget — and rides INSIDE the body. Printing it to a terminal is not
   # disclosing it. PMAT-1484's lesson applied to the delta: carve out what MUST
   # be published first, then budget the prefix around what is left.
   DELTA_N=$(git rev-list --count "v$V"..origin/main -- CHANGELOG.md)
   DELTA_L=$(git diff --numstat "v$V"..origin/main -- CHANGELOG.md | awk '{ s += $1 } END { print s+0 }')
   : > /tmp/reldelta.md
   if [ "${DELTA_N:-0}" -gt 0 ]; then
     {
       echo
       echo "> **⚠️ \`CHANGELOG.md\` HAS MOVED SINCE THIS TAG, AND THIS BODY DOES NOT"
       echo "> CONTAIN THE DIFFERENCE.** The text below was extracted from tag"
       echo "> \`v$V\`, which is what shipped. Since the tag object was written,"
       echo "> **${DELTA_N} further commit(s) added ${DELTA_L} line(s)** to"
       echo "> \`CHANGELOG.md\` on \`main\`. That text is in neither this body nor"
       echo "> \`git show v$V:CHANGELOG.md\`; read it with"
       echo "> \`git log -p v$V..origin/main -- CHANGELOG.md\`. Long subjects are"
       echo "> elided in the MIDDLE so the trailing \`(Refs PMAT-nnnn)\` survives:"
       git log --format='%h%x09%s' "v$V"..origin/main -- CHANGELOG.md \
         | awk -F'\t' '{ s = $2; id = ""
             # ELIDE THE MIDDLE, NOT THE TAIL. Every subject in this repo ends
             # in `(Refs PMAT-nnnn)` — the only handle a reader has for finding
             # the CHANGELOG entry this line is pointing them at. A trailing
             # cut removed it from 7 of 7 lines (PMAT-1486).
             if (match(s, /\(Refs PMAT-[0-9]+\)/)) id = substr(s, RSTART, RLENGTH)
             if (length(s) > 120) {
               keep = 117 - length(id); if (keep < 24) keep = 24
               s = substr(s, 1, keep) "… " id
             }
             printf "> - `%s` %s\n", $1, s }'
     } > /tmp/reldelta.md
   fi
   DELTA_C=$(wc -m < /tmp/reldelta.md)

   if [ "$FULL_C" -le "$CAP" ] && [ "$(( FULL_C + DELTA_C ))" -le "$CAP" ]; then
     # PMAT-1481's append rule, relocated to the TOP of the body: a disclosure
     # buried past 7,700 lines is not one, and appending it at the END would
     # break the byte-for-byte tail assertion below. An empty delta degenerates
     # this to `cp` — body IS the section, byte for byte.
     { head -1 /tmp/relsection.md; cat /tmp/reldelta.md; tail -n +2 /tmp/relsection.md; } > /tmp/relbody.md
   else
     RESERVE=2000; BUDGET=$(( CAP - RESERVE - MAND_C ))
     [ "$BUDGET" -gt 0 ] || { echo "ABORT: mandatory sections alone are ${MAND_C} chars — A10"; exit 1; }
     {
       head -1 /tmp/relsection.md; echo
       echo "> **⚠️ THIS BODY IS TRUNCATED IN THE MIDDLE, AND BY HOW MUCH IS STATED"
       echo "> HERE.** The \`[$V]\` section of \`CHANGELOG.md\` at tag \`v$V\` is"
       echo "> **${FULL_C} characters / ${FULL_L} lines**. A GitHub release body is"
       echo "> capped at **${CAP} characters** (the API returns \`422 body is too"
       echo "> long\`), so the full section cannot be published here. What follows is"
       echo "> its leading prefix, cut at a line boundary, and then — VERBATIM, past"
       echo "> the cut marker — the three mandatory disclosure sections"
       echo "> *What still REFUSES* / *What is NOT merge-blocking* /"
       echo "> *Known divergences* (${MAND_L} lines), which are the tail of the"
       echo "> section and which a leading-prefix cut would otherwise drop in full."
       echo "> The authoritative text is the file: \`git show v$V:CHANGELOG.md\`."
       cat /tmp/reldelta.md
       echo
     } > /tmp/relbody.md
     HDR_L=$(wc -l < /tmp/relbody.md); HDR_C=$(wc -m < /tmp/relbody.md)
     [ "$(( BUDGET - HDR_C ))" -gt 0 ] || { echo "ABORT: header+delta ${HDR_C} exhausts the budget — A10"; exit 1; }
     # awk's `length()` counts characters in a UTF-8 locale, the same unit as
     # the cap — so the budget arithmetic is consistent end to end.
     tail -n +2 /tmp/relsection.md \
       | awk -v b="$(( BUDGET - HDR_C ))" \
           '{ n = length($0) + 1; if (used + n > b) exit; used += n; print }' \
       >> /tmp/relbody.md
     KEPT_L=$(( $(wc -l < /tmp/relbody.md) - HDR_L + 1 ))
     {
       echo
       # A cut at an arbitrary line boundary can land INSIDE a fenced block. The
       # unclosed fence then swallows the cut marker AND all three mandatory
       # sections into one code span: `grep -q '^### …'` still finds every
       # heading, so the acquittal control below passes while the reader sees
       # 400 lines of monospace. Close it, and assert parity after.
       [ "$(( $(grep -c '^```' /tmp/relbody.md) % 2 ))" -eq 0 ] || { echo '```'; echo; }
       echo "> **⟨cut here⟩** — lines 1-${KEPT_L} and ${MAND_L0}-${FULL_L} of ${FULL_L}"
       echo "> are published; the **$(( FULL_L - KEPT_L - MAND_L )) lines in between are omitted**"
       echo "> (~$(( FULL_C - $(wc -m < /tmp/relbody.md) - MAND_C )) characters)."
       echo "> Read the whole section with"
       echo "> \`git show v$V:CHANGELOG.md | awk -v h='## [$V]' 'index(\$0,\"## [\")==1 { f = (index(\$0,h)==1) } f'\`"
       echo; echo "---"; echo
     } >> /tmp/relbody.md
     cat /tmp/relmand.md >> /tmp/relbody.md
   fi

   test -s /tmp/relbody.md || { echo "ABORT: empty body"; exit 1; }
   B=$(wc -m < /tmp/relbody.md)
   [ "$B" -le "$CAP" ] || { echo "ABORT: body ${B} chars over cap ${CAP}"; exit 1; }
   # THE ACQUITTAL CONTROL, and it is not optional — the whole point of the
   # carve-out is that these three sections reach the page. Assert it, do not
   # eyeball it. Zero hits here is the defect PMAT-1484 found, at exit 0.
   for h in '^### What still REFUSES$' '^### What is NOT merge-blocking$' '^### Known divergences$'; do
     grep -q "$h" /tmp/relbody.md || { echo "ABORT: body lost $h — A10"; exit 1; }
   done
   # A heading inside an unclosed fence satisfies the loop above and renders as
   # code. Parity is the half of that control that reads what the reader sees.
   [ "$(( $(grep -c '^```' /tmp/relbody.md) % 2 ))" -eq 0 ] ||
     { echo "ABORT: body has an unclosed code fence — A10"; exit 1; }
   # …and the delta's own acquittal control. `git diff --stat` to a terminal is
   # not a disclosure: nobody records a terminal. Assert it reached the BODY.
   [ "${DELTA_N:-0}" -eq 0 ] || grep -q 'HAS MOVED SINCE THIS TAG' /tmp/relbody.md ||
     { echo "ABORT: ${DELTA_N} post-tag commits and the body discloses none — A10"; exit 1; }
   # …and the OTHER end of the headings guard: three headings present does not
   # mean three sections intact. Assert the body's last MAND_L lines are the tail
   # BYTE FOR BYTE — this holds in both regimes, because the delta notice goes to
   # the TOP in both, so the tail is the tail either way.
   tail -n "$MAND_L" /tmp/relbody.md > /tmp/relbody.tail.md
   cmp -s /tmp/relbody.tail.md /tmp/relmand.md ||
     { echo "ABORT: body tail is not the mandatory sections verbatim — A10"; exit 1; }
   gh release create "v$V" --title "v$V" --notes-file /tmp/relbody.md   # non-draft
   gh release view "v$V" --json body -q .body > /tmp/relbody.published.md
   diff /tmp/relbody.md /tmp/relbody.published.md
   ```

   **Measured on `v0.1.618` (2026-07-30, PMAT-1484), by running the block:**
   the section is 7,700 lines; the three mandatory sections start at line
   **7,319** and run to the end — **382 lines / 24,324 characters, 19.5% of the
   cap**. The pre-1484 leading-prefix form published lines 1–1,972 and
   therefore **contained none of the three headings**. The 1484 form published
   lines 1–1,570 plus 7,319–7,700, at 123,236 characters with 1,764 to spare.

   **Re-measured 2026-07-30 after PMAT-1485 added the post-tag notice**, by
   extracting this block from this file and running it: the notice is **1,157
   characters / 5 commits / 526 lines**, the prefix shrinks to absorb it, and
   the body is lines 1–**1,550** plus 7,319–7,700 — **123,266 characters, 1,734
   to spare**, three headings present, tail byte-identical, fence parity even.

   **The cap is real, it is 125,000 characters, and `v0.1.618` is the first
   release that cannot fit under it.** Measured 2026-07-30 by POSTing an
   oversized body to the API:

   ```text
   422 {"resource":"Release","field":"body",
        "message":"body is too long (maximum is 125000 characters)"}
   ```

   Measured 2026-07-30 at each tag, in **characters** (`wc -m`), against
   `jq`-counted published body lengths:

   | version | `[x.y.z]` section at its tag | published body | % of the 125,000 cap |
   |---|---|---|---|
   | `v0.1.615` | 1,325 | (no release) | 1.1% |
   | `v0.1.616` | 24,727 | 1,364 | 19.8% |
   | `v0.1.617` | 79,256 | 80,982 | **63.4%** |
   | `v0.1.618` | **469,246** (7,700 lines) | — | **375.4%** |

   Re-derive rather than trust the table:
   `git show "v$V":CHANGELOG.md | awk … | wc -m`. The trend is the point — the
   wall was one release away and nothing in this procedure was watching for it,
   because `test -s` guards the **empty** end only. PMAT-1480 found a zero-byte
   body at exit 0; this is the same failure at the other end of the same
   dimension, and it does **not** fail silently: `gh release create` returns
   `422` and creates nothing, so the release simply does not happen. Under **A7
   — no Friday code** there is no time to invent a policy at that moment, which
   is why the policy is here.

   **Truncation is permitted; SILENT truncation is not.** The body carries, at
   the top, the section's true character and line count, the cap, and the
   `git show <tag>:CHANGELOG.md` command that yields the whole text — and at the
   cut marker, exactly which line ranges survived and how many lines and
   characters were dropped. A reader who never clones learns that they are
   holding an excerpt and how to get the rest. Cutting silently would publish a
   body that reads complete and is 26% of the story, which is `v0.1.616`'s 5.5%
   defect (PMAT-1480) reintroduced by a size limit instead of by hand-assembly.

   **HOW MUCH TO CUT AND WHAT TO CUT ARE TWO DECISIONS, AND PMAT-1483 ONLY MADE
   THE FIRST (PMAT-1484).** A size budget answers *how many* lines survive; it
   says nothing about *which*, and "the leading ones" is a default, not a
   choice. Made explicit, that default is indefensible here. §4 step 2 calls
   *What still REFUSES* / *What is NOT merge-blocking* / *Known divergences*
   **mandatory** — they are the release's honest-disclosure surface — and this
   CHANGELOG is written newest-arc-first, so all three sit at the **end** of the
   section. On `v0.1.618` they begin at line 7,319 of 7,700. **The leading-prefix
   body published lines 1–1,972 and contained none of them**, while publishing
   1,972 lines of internal slice narrative — and this file argues 60 lines below
   that the release page is the **only non-clone path** to `CHANGELOG.md`, since
   Cargo packages it into none of the 31 crates. So the pre-1484 procedure
   dropped, from the one copy an outside reader can reach, precisely the three
   sections written for that reader.

   **It would have looked fine.** The truncated body mentions all three section
   names ten times in prose (the PMAT-1473/1474/1478 slice narratives are *about*
   them), so a spot-check for the strings finds them and a reader has no way to
   tell a citation from the section. That is why the block asserts the three
   **headings** (`^### …$`) and not the phrases, and why the acquittal control is
   inside the runbook rather than in a reviewer's head.

   **`wc -m`, not `wc -c` — and this was a real bug in the first version of this
   step.** The cap counts characters; `wc -c` counts bytes, and this CHANGELOG is
   dense with `—`, `★` and `⚠️`. Measured here: a body of **124,941 bytes** is
   **124,025 characters** — a 916-unit gap on one release note. The first
   spelling budgeted with `awk length()` (characters, in a UTF-8 locale) and then
   *checked* with `wc -c` (bytes), so the guard came within **59 bytes** of
   aborting a body that was 975 characters clear of the cap. Mixing the two units
   inside one budget is the defect; both are now `-m`, the same unit the API
   enforces. Caught by running the block, not by reading it.

   **Over the cap, the post-tag delta is disclosed by REFERENCE, not appended.**
   PMAT-1481's rule — repair the tag/`main` gap by *appending* the delta under
   its own dated heading — assumes there is room. There is not: the body is
   already 3.8× over. So when `FULL_C > CAP`, name the post-tag commits (`git log
   --oneline "v$V"..origin/main -- CHANGELOG.md`) in the truncation notice and
   leave the text in the file, which is where the notice already points. The
   append rule stands for any release that fits.

   **AND THAT RULE WAS PROSE ONLY — THE BLOCK EMITTED NO SUCH NOTICE
   (PMAT-1485).** Both spellings of the tag/`main` repair — PMAT-1481's *append*
   and PMAT-1483's *by reference* — were written into this section and **neither
   was ever implemented**. Measured 2026-07-30 by running the block and grepping
   its output: `grep -ci post-tag /tmp/relbody.md` → **0**. The delta's only
   appearance was a `git diff --stat` printed to the operator's terminal on the
   line before `gh release create`, and **a terminal is not a publisher** — the
   figure scrolls past one person and reaches no reader. So the body would have
   gone out saying *"The authoritative text is the file: `git show
   v0.1.618:CHANGELOG.md`"* while that command returns a file missing **526
   lines across 5 commits** — and those five are `PMAT-1480` … `PMAT-1484`, the
   entire arc that built this procedure, **including the entry that announces
   the truncation rule the reader is holding**. The gap is not static and that is
   the part worth watching: PMAT-1481 measured it at **97 lines** on 2026-07-29
   and it is **526** a day later, because the freeze permits exactly the docs
   lane that edits `CHANGELOG.md`. A pointer whose target is knowably incomplete,
   with the incompleteness unstated, is the `v0.1.616` defect in one more place.
   Fixed: `DELTA_N`/`DELTA_L` are computed **before** the budget, the notice is
   built into `/tmp/reldelta.md`, it rides at the **top** of the body in both
   regimes, and a `grep -q 'HAS MOVED SINCE THIS TAG'` acquittal control refuses
   to publish when `DELTA_N > 0` and the notice is absent. Driven both ways: the
   guard aborts with `ABORT: 5 post-tag commits and the body discloses none` on
   the pre-1485 body.

   **The delta notice goes at the TOP, and that placement is load-bearing
   twice.** Appending it — PMAT-1481's literal wording — would put the
   disclosure past 7,700 lines of slice narrative in the under-cap regime, which
   is not disclosure; and it would break the byte-for-byte tail assertion, since
   the body's last `MAND_L` lines would then be the notice rather than *Known
   divergences*. At the top, one spelling serves both regimes and the tail
   invariant holds in each. When there is no delta the notice is an empty file
   and the under-cap body is the section byte for byte, exactly as before.

   **A prefix cut can land inside a code fence, and the acquittal control cannot
   see it (PMAT-1485).** The cut is at an arbitrary line boundary; nothing made
   it respect fenced blocks. An odd number of `^```` lines in the prefix leaves
   a fence open, and everything after it — the cut marker **and all three
   mandatory sections** — renders as one code span. `grep -q '^### What still
   REFUSES$'` still matches, so the 1484 control passes at exit 0 while the
   reader sees 400 lines of monospace: the same *"a spot-check finds the string
   and the reader cannot tell a citation from the section"* failure that
   paragraph was written about, one layer down. On `v0.1.618` the prefix happens
   to hold **8** fences — even, and safe **by luck, not by construction**. Fixed
   at both ends: the cut marker closes an odd fence, and a parity assertion runs
   beside the headings loop. Demonstrated, not assumed — a synthetic prefix
   ending inside a fence aborts with `ABORT: body has an unclosed code fence`
   when the auto-close is disabled.

   **Which tree, stated because the two already disagree.** Through PMAT-1480
   this step read `CHANGELOG.md` as a bare relative path, which resolves against
   whatever checkout the operator happens to be in — and A1 puts them in the tag
   worktree while the docs lane keeps moving `main`. On `v0.1.618` the gap opened
   one commit after the tag was cut: the `## [0.1.618]` section is 7,699 lines at
   the tag and 7,796 on `main`. Both choices are wrong in a different direction,
   so the choice has to be made rather than inherited from a `cd`:

   - extract at the **tag** → the page omits post-tag entries, including
     PMAT-1480's own, which is the entry that announces this very rule;
   - extract on **`main`** → the page publishes prose describing commits that are
     **not in the released source**, which is the stronger falsehood, since the
     page's job is to document what shipped.

   The tag wins, and the omission is repaired by the **post-tag notice** the
   block builds into `/tmp/reldelta.md` — never by folding the delta into the
   section silently, which would reproduce the page-vs-file drift in a third
   place.

   ⚠️ **This sentence read *"repaired by appending the post-tag delta under its
   own dated heading"* until PMAT-1485, 21 lines below the paragraph that had
   already scoped that rule to *"any release that fits"* — and `v0.1.618` does
   not fit, so the unconditional form is unsatisfiable on the very release it
   governs.** It was also the **last** word in the step, which is the one an
   operator reads last. This is PMAT-1483's own finding — §0 still teaching what
   §2b forbids — repeating one slice later inside a single section: **amending a
   rule in one place does not amend its restatements**, and a document that
   argues with itself is decided by reading order.

   **The round-trip `diff` is TREE-BLIND — it cannot catch this.** It compares
   the published body against the same file the body was extracted from, so it
   proves the upload was faithful and says nothing about whether the source was
   the right tree. A wrong-tree extraction passes it cleanly. `test -s` is the
   other half: PMAT-1480's first extractor spelling wrote a **zero-byte body at
   exit 0**, and the `diff` was green on it, because empty matched empty.

   It is also, above the cap, **equality-blind by construction**: the file it
   diffs against is `/tmp/relbody.md`, the *assembled* body, so a green `diff`
   certifies transport and says nothing about whether the body equals the
   section — and above the cap it provably does not. *"Published body == the
   `[x.y.z]` section at the tag"* is therefore **not a satisfiable invariant**
   for a release this size, only for one that fits; the honest generalisation is
   *the body is a **prefix** of the section, plus a disclosure naming exactly what
   was cut*, which holds in both regimes and degenerates to equality when
   `FULL_C <= CAP`. That is the form `next_lane[0]`'s `XPILE-RELBODY-001` spec
   now carries (constraint **(e)**); the equality spelling it had would have
   red-ed on `v0.1.618` itself — the second time in two days that gate's
   unwritten spec has been caught mandating a false red on a correct release.

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
   Three consequences, all measured on `v0.1.617`:

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
   - **And the page is the ONLY non-clone path this file has, which is why the
     above matters more than it looks.** `CHANGELOG.md` sits at the workspace
     root, outside every package directory, so Cargo packages it into **none** of
     the crates: `xpile-0.1.617.crate` holds 916 files and exactly two `.md`
     (`README.md`, `examples/README.md`), neither a CHANGELOG. Derive it, do not
     trust this sentence —
     `curl -sL -H 'User-Agent: xpile-release-verify' https://static.crates.io/crates/xpile/xpile-$V.crate | tar tz | grep -ci changelog || true`
     (executed 2026-07-30 against `0.1.617`: prints `0`. The `|| true` is load
     bearing — `grep -c` **exits 1 when the count is 0**, which is the answer
     being asked for, so under `set -e` the correct result aborts the shell).
     What *is* packaged is the root `README.md`, via `readme = "../../README.md"`.
     So a reader who does not clone reaches this file through the release body and
     through nothing else, and reaches the README frozen per version forever —
     crates.io versions are immutable, so a post-tag README correction can never
     reach an already-published page (PMAT-1471's finding, generalised).
     ⚠️ **This paragraph named the README as the second non-clone publisher for
     four days and no step ever fetched the rendered page** (PMAT-1486). Step 6
     now does. Naming a publisher is not measuring it — see A11 for what the
     first measurement found.

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
6. **Verify the RENDERED FRONT PAGE — the second non-clone publisher (PMAT-1486).**

   Step 5 proves the crate *arrived*. It says nothing about the document
   crates.io puts on its front page, which is the only part of this repository a
   reader reaches without `git clone` other than the release body — and which is
   **immutable per version**, so this is a measurement of what shipped, not a
   check that can be fixed in place. Run it anyway: the finding belongs in the
   CHANGELOG for the *next* release (A11).

   ```bash
   C=xpile V=0.1.618                       # crate whose front page to measure
   FP=$(mktemp -d)/frontpage.html
   curl -sL -o "$FP" -H 'User-Agent: xpile-release-verify' \
     "https://crates.io/api/v1/crates/$C/$V/readme"
   test -s "$FP" || echo "ABORT: $C $V renders no front page — A11"

   # (a) THE TELL. `readme = "../../README.md"` cannot be packaged by reference:
   #     cargo COPIES the file to the tarball root and rewrites the key to
   #     `readme = "README.md"` (probe-verified). crates.io then resolves every
   #     relative path inside it against the package's REPO SUBDIRECTORY —
   #     `crates/$C/` — not the repo root the file was written for. So any such
   #     prefix in the rendered HTML is a relative link that has been silently
   #     reinterpreted, and it is wrong whether it 404s or not.
   #     `raw` is NOT optional: crates.io rewrites link hrefs under `blob`/`tree`
   #     but IMAGE srcs under `raw`. The first spelling of this arm omitted it
   #     and undercounted `xpile-bigint@0.1.617` 10-of-11, missing a dead
   #     `hero.svg`; that one surfaced only in (b), and a rewritten image that
   #     resolved 200 to the wrong picture would have escaped BOTH arms.
   grep -oE "https://github\.com/paiml/xpile/(blob|tree|raw)/HEAD/crates/$C/[^\"?]*" "$FP" \
     | sort -u > /tmp/fp_rewritten.txt
   test ! -s /tmp/fp_rewritten.txt \
     || { echo "ABORT: $(wc -l < /tmp/fp_rewritten.txt) relative link(s) reinterpreted against crates/$C/ — A11"; cat /tmp/fp_rewritten.txt; }

   # (b) Every URL the page actually offers must resolve — hrefs AND img srcs,
   #     because a broken badge is also a broken claim.
   { grep -oE 'href="https://[^"]+"' "$FP" | cut -d'"' -f2
     grep -oE '<img[^>]*src="https://[^"]+"' "$FP" | sed 's/.*src="//;s/"$//'; } \
     | sed 's/?sanitize=true$//' | sort -u \
     | grep -v '^https://crates\.io/crates/' | while read -r u; do
         c=$(curl -s -o /dev/null -w '%{http_code}' -H 'User-Agent: xpile-release-verify' "$u")
         case "$c" in 200|301|302) ;; *) echo "DEAD $c $u — A11" ;; esac
       done
   ```

   ⚠️ **`https://crates.io/crates/<name>` is EXCLUDED from (b) on purpose, and
   the exclusion is a measurement, not a convenience.** That endpoint returns
   `404` to every non-browser client — `serde` and `tokio` return `404` too
   (measured 2026-07-30) — so a link checker that includes it reports the
   README's own registry link dead on a perfectly good page. Registry presence
   is step 5's job, via the API. This is the §5-step-5 lesson one layer out:
   **before believing a checker that contradicts a known-good artefact, run it
   against a control you know is fine.**

   ⚠️ **(a) is the arm that matters, and a 404 is the FRIENDLY failure.** Two of
   the eleven targets measured on `0.1.618` resolve at `200` **to the wrong
   thing**: `crates/xpile/contracts` is a **symlink** (git mode `120000`), so
   GitHub serves a page whose entire content is the text `../../contracts`
   instead of the 35 contracts the sentence promises; and `crates/xpile/examples/`
   is a real but *different* directory — 7 Rust API examples, none of the four
   Python programs the words "more runnable programs" refer to. Neither shows an
   error to the reader. **Check the prefix, never just the status code.**
7. **Verify the RENDERED API DOCS — the THIRD non-clone publisher (PMAT-1490).**

   Step 5 proves the crate arrived. Step 6 measures the front page crates.io
   renders. Neither reads the page the registry's own sidebar sends a library
   consumer to: **30 of the 31 crates have a `lib` target, so crates.io
   auto-links `docs.rs` for every one of them**, and that page is rendered from
   the doc comments in `crates/*/src`. Like the front page it is **immutable per
   version**, so this is a measurement of what shipped, not a check that can be
   fixed in place; the finding belongs in the CHANGELOG for the *next* release
   (A14).

   ⚠️ **`doc_status: true` IS NOT AN ACQUITTAL.** §4 step 6 and A12 read that
   flag, and it answers exactly one question — *did rustdoc exit 0*. All 30
   siblings answer `true` and 13 of them publish a defective page. This is the
   step-6 lesson (`a 200 is not an acquittal`) one publisher along: **a green
   build is not a correct page.**

   ```bash
   # ⚠️ RUN IT IN A FRESH TARGET DIR. `cargo doc` emits a rustdoc warning only
   #    when it actually re-documents a crate; against a warm target dir the
   #    whole command prints NOTHING and exits 0 — an acquittal indistinguishable
   #    from a clean page, and how the first run of this measurement reported
   #    zero defects over 58 real ones.
   DOCDIR=$(mktemp -d)
   RUSTDOCFLAGS='-W rustdoc::all' CARGO_TARGET_DIR="$DOCDIR" \
     cargo doc --workspace --no-deps > "$DOCDIR/rustdoc.log" 2>&1
   echo "cargo doc exit: $?   (0 is EXPECTED — rustdoc lints are warnings)"

   # NON-VACUITY: the run must have documented the whole published population.
   DOCUMENTED=$(grep -c '^ Documenting ' "$DOCDIR/rustdoc.log")
   LIBS=$(cargo metadata --no-deps --format-version 1 \
          | jq '[.packages[] | select(any(.targets[]; .kind[]|.=="lib" or .=="proc-macro"))] | length')
   echo "documented=$DOCUMENTED  crates-with-a-lib-target=$LIBS"
   test "$DOCUMENTED" -ge "$LIBS" \
     || echo "ABORT: doc run covered $DOCUMENTED < $LIBS crates — the measurement is vacuous"

   # THE SPLIT. The flagship has no `lib` target (A12), so a warning under
   # `crates/xpile/src/` reaches NO published page. Report it; never count it.
   awk '/^warning: /{h=substr($0,10)}
        /^ *--> /{p=$2; sub(/:[0-9]+:[0-9]+$/,"",p);
                  print (p ~ /^crates\/xpile\/src\//?"UNPUBLISHED":"PUBLISHED"), p, h}' \
     "$DOCDIR/rustdoc.log" > "$DOCDIR/sites.txt"
   echo "PUBLISHED page defects: $(grep -c '^PUBLISHED' "$DOCDIR/sites.txt") \
   across $(grep '^PUBLISHED' "$DOCDIR/sites.txt" | cut -d' ' -f2 | cut -d/ -f2 | sort -u | wc -l) crates"
   echo "UNPUBLISHED (no docs.rs page — A12): $(grep -c '^UNPUBLISHED' "$DOCDIR/sites.txt")"
   grep '^PUBLISHED' "$DOCDIR/sites.txt" | cut -d' ' -f2 | cut -d/ -f2 | sort | uniq -c | sort -rn
   ```

   ⚠️ **DO NOT REPLACE THIS WITH AN HTML GREP OF THE LIVE PAGE, and that is a
   measurement, not a preference.** A failed intra-doc link renders as the
   literal `[<code>X</code>]` where a resolved one renders as an `<a href=…>`, so
   grepping the published HTML for `\[<code>` *looks* like a free network arm
   that needs no toolchain. Scored against rustdoc's own verdict over the 30
   crates it gets **3 false positives and 2 false negatives**: prose that
   legitimately brackets a code span (the `` `[ … ]` `` shell test, an `` `[T]` ``
   slice) fires it, and a defect in an *item* doc renders on that item's page and
   not on `index.html`, so a root-page scan misses it. **rustdoc's own lints are
   the oracle; the rendered page is only how you confirm what a reader sees.**

---

## 6. Abort rules

⛔ **THIS SECTION IS THE ONLY DEFINITION SITE. Every other document cites it and
transcribes none of it** (PMAT-1488). The sprint plan carried a second copy until
2026-07-30; it was short four rules and eight of its nine had drifted, two of
them back into falsehoods this section had already retired. If you are adding a
rule, add it *here* and add it *only* here.

⚠️ **THE GATE OVER THIS SECTION STOPS AT A8, and that is a MEASUREMENT, not an
invariant.** `crates/xpile/tests/release_preflight_witness.rs` — whose test is
named `the_release_doc_documents_the_procedure_and_the_abort_rules` — checks a
**hard-coded literal** running A1, A1b, A2 through A8, written when eight was the
whole set. Demonstrated 2026-07-30 with both halves run: deleting A8's definition
bullet **reds** it, and deleting all four of A9's, A10's, A11's and A12's leaves
it **green at exit 0**. So the four rules that fire or are disclosed on ship day
are protected by nothing. Falsify it yourself — this is deliberately spelled with
a computed pattern, because *printing* a rule's bullet marker in prose would
itself feed the gate its needle and mask a genuinely deleted rule:

```bash
cp docs/RELEASE.md /tmp/rel.bak            # not `git checkout` — that would also
                                           # discard any uncommitted edit here
python3 - <<'EOF'
import re
p = 'docs/RELEASE.md'
lines = open(p).read().split('\n')
marker = '- ' + '*' * 2                   # computed, never written out literally
# DERIVED, not enumerated: every rule whose ordinal is past where the gate's
# literal stops. Writing "A9|A10|A11|A12" here would rot the day A13 lands —
# which is the whole defect this recipe demonstrates.
LITERAL_STOPS_AT = 8
def past_the_literal(line):
    if not line.startswith(marker):
        return False
    m = re.match(r'^A(\d+)b? — ', line[len(marker):])
    return bool(m) and int(m.group(1)) > LITERAL_STOPS_AT
kept = [l for l in lines if not past_the_literal(l)]
dropped = len(lines) - len(kept)
assert dropped > 0, f'mutation did not apply: nothing matched ({len(lines)} lines)'
print(f'removed {dropped} rule definition(s) past A{LITERAL_STOPS_AT}')
open(p, 'w').write('\n'.join(kept))
EOF
cargo test -p xpile --test release_preflight_witness    # still GREEN — the defect
cp /tmp/rel.bak docs/RELEASE.md
```

Fixing that is a gate edit, barred by the 2026-07-29 18:00 freeze; the spec is
`XPILE-ABORTRULE-001` in `docs/roadmaps/queue.yaml` `next_lane`, and it must
**derive** the rule set from this section rather than extend the literal by four
— four additions escaped in four days, which is evidence about the shape and not
about anyone's memory. **Until it lands, a rule is kept by this file and by
review, not by CI** — so do not read a green `workspace-test` as evidence that
the rule you are about to rely on is still written down.

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
  following Friday. The *k−1* being valid is not the same as this being free:
  re-run §4 step 6 arm 3 and record which of the `31 − k` are left serving
  retired prose (A13).
- **A5 — OVERLAY (recoverable, do NOT abort).** `no hash listed for <crate> vX`
  is the stale local package overlay, not a real failure: purge per §4 step 5
  and restart from the dry-run. Without this rule A4 would fire on a fully
  recoverable condition and needlessly kill the release.
- **A6 — HARD TIME-BOX.** If `cargo publish --workspace` has not **started** by
  16:00 local Friday, abort the crates.io step entirely: ship
  GitHub-tag-and-release only (a tagged, unpublished release is an acceptable
  outcome) and roll the batch to the following Friday. Never begin an
  irreversible whole-workspace batch into an evening with no recovery window.
  "Acceptable" is about *risk*, not about *cost*: a rolled batch keeps every A13
  crate's retired prose published for another week, so name them in the release
  body rather than shipping a tag-only release that reads as a no-op.
- **A7 — NO FRIDAY CODE.** If a defect surfaces Friday morning the answer is
  A6 (ship the tag, defer the batch), never a same-day hotfix onto the release
  SHA.
- **A8 — CONCURRENCY.** All release work happens in an isolated `git worktree`;
  re-check `git rev-parse origin/main` against the recorded high-water mark
  before every push; the release SHA is **pinned** and never re-resolved as
  "whatever `HEAD` is."
- **A9 — BODY OVER CAP (recoverable, do NOT abort).** `gh release create`
  returning `422 body is too long (maximum is 125000 characters)` is not a defect
  in the artifact and not a reason to slip the day. It creates nothing, so it is
  safe to retry. Build the body with the size-checked form in §5 step 3, which
  truncates at a line boundary *and discloses the cut*, then re-run. Two things
  this rule exists to forbid: trimming the body by hand until it fits
  (undisclosed truncation — the `v0.1.616` 5.5%-of-the-story defect with a new
  cause), and skipping the release body altogether. `v0.1.618`'s section is 3.8×
  the cap, so this is the **expected** path now, not an edge case — which is why
  it is an abort *rule* and not a footnote.
- **A10 — MANDATORY SECTIONS UNREACHABLE (abort the body, not the day).** §5
  step 3 refuses to build a body if the three mandatory sections cannot be
  located as a contiguous tail (`^### What still REFUSES$` missing, or the other
  two not below it), or if they alone exceed the cap, or if the assembled body
  does not contain all three headings. Every one of those is a *documentation*
  defect, not an artifact defect: the tag, the crates and the dry-run are
  unaffected, so **do not slip the crates.io batch over it.** Fix the CHANGELOG
  section on `main` and rebuild the body — the release can be created after the
  publish. What this rule forbids is publishing a body that silently lacks them,
  which is what happened by default until PMAT-1484 measured it: on `v0.1.618`
  the leading-prefix body carried **zero of the three headings** at exit 0, and
  nothing in the procedure looked. If the sections legitimately do not fit
  alongside any useful prefix, publish them *first* and the prefix second —
  never drop them.

---

- **A11 — FRONT PAGE DEFECTIVE (record it; never touch the batch).** §5 step 6
  measures the rendered crates.io front page. It runs **after** the upload and
  the version is immutable, so A11 can never be *repaired* for the release that
  triggers it — it is a **disclosure** rule: write the finding to
  `CHANGELOG.md` under the *next* version and, if the release body is still
  editable, mirror it per §5 step 3's ordering. **A11 must never delay, retry or
  partially revert a batch** — an already-published crate is valid regardless of
  what its front page links to (contrast A4, which stops the batch). Like A10 it
  aborts an artefact, not the day.

  First run, `0.1.618` (PMAT-1486, the measurement that created this rule): the
  flagship publishes **12 relative link refs across 11 targets, and every one is
  wrong** — 9 hard `404`, 2 resolving `200` to the wrong object. Among the dead:
  both `LICENSE-MIT` and `LICENSE-APACHE` on a dual-licensed crate, both design
  specs, `ci.yml`, the enforcement handoff, and — at the doubled path
  `crates/xpile/crates/xpile/tests/readme_quickstart_witness.rs` — **the
  README's own cited evidence for its own quickstart.**

  ⛔ **`v0.1.618` FIRES A11, AND THAT IS THE EXPECTED OUTCOME — DO NOT STOP
  FRIDAY OVER IT.** The tag was cut 2026-07-30 at `6b5f6c02`; PMAT-1486's fix
  (absolute `blob/main` / `tree/main` URLs, each form HTTP-measured) landed
  hours later, so **the tagged tree still carries all 12 relative refs and the
  flagship's `0.1.618` front page will publish every one of them broken.** The
  repaired README reaches `0.1.619`. Re-derive rather than trust this sentence:
  `git show v0.1.618:README.md | grep -oE '\]\([^)]*\)' | grep -vcE '\]\((https?:|#)'`
  → **12** at the tag, **0** on `main`. Nothing about this touches an uploaded
  crate's validity.

  ⚠️ **THIS SENTENCE SAID THE OPPOSITE FOR EIGHT HOURS, AND IT IS THE ONLY
  SENTENCE IN A11 THAT TELLS THE OPERATOR WHAT TO DO (PMAT-1491).** It read
  *"the fix … landed after the tag, so `0.1.618`'s pages carry it and
  `0.1.619`'s will not"* — false in both halves. A11's only action is
  disclosure, so an operator who believes `0.1.618` carries the fix has nothing
  to disclose and the rule **self-cancels at exit 0**: PMAT-1486's measurement
  would never reach `0.1.619`'s CHANGELOG, which is the one place A11 sends it.
  **Root cause — the disposition was PARAPHRASED from PMAT-1486's CHANGELOG
  entry instead of derived from the tree.** That entry states it correctly
  (*"`0.1.618`'s own pages carry all 12 broken links"*); the paraphrase dropped
  the noun, `it` bound to *the fix* — the new sentence's grammatical subject —
  and the trailing clause was kept although it only parses against the noun that
  was removed. This is PMAT-1488's finding one level down: **a rule written down
  twice, with nothing comparing the two copies.** PMAT-1488 could fix its copy
  by deleting it; A11 cannot be deleted, because §6 must stand alone for the
  operator — so it carries the falsification command above instead. The other
  five `0.1.618` records were cross-checked against their CHANGELOG entries the
  same way and all five agree (A9 `469,246`/`7,700`; A10 `7,319`/`382`/`24,324`;
  A12; A13's seven crates, each re-verified `tag == main`; A14 `38+18+2 = 58`).

- **A12 — PUBLISHED METADATA URL DEAD.** Its disposition depends entirely on
  *when* it fires. (Until PMAT-1489 this rule claimed to be the *only* one for
  which that holds; A13 is the second, and inverts it.)

  - **Before the tag (§4 step 6) it BLOCKS the tag.** A `documentation` or
    `homepage` URL that does not serve what it claims is a one-line manifest
    fix. Fix it on `main`, re-pin, and if a tag was already cut, A3 applies —
    a burned patch number is free.
  - **After the tag it is DISCLOSURE ONLY, exactly like A11.** The manifest is
    inside the tagged tree and crates.io versions are immutable, so the claim
    cannot be repaired for the release that carries it. **A12 must never delay,
    retry or partially revert a batch.**

  ⛔ **`v0.1.618` FIRES A12 IN ITS POST-TAG FORM, AND THAT IS THE EXPECTED
  OUTCOME — DO NOT STOP FRIDAY OVER IT.** The tag was cut 2026-07-30 at
  `6b5f6c02`; PMAT-1487 measured and fixed the key hours later, so the tagged
  tree still carries `documentation = "https://docs.rs/xpile"` and all 31 crates
  will publish it. The repaired value (`https://paiml.github.io/xpile/`) reaches
  `0.1.619`. Nothing about this touches an uploaded crate's validity.

  First run, `0.1.618` (PMAT-1487, the measurement that created this rule): the
  flagship — the **only** member of the 31 with no `lib` target and the **only**
  one that sets `documentation` — pointed that key at `https://docs.rs/xpile`,
  which has never rendered a page and structurally cannot, because docs.rs
  documents `lib` targets. `doc_status:false` at `0.1.615`, `0.1.616` and
  `0.1.617`; `true` for all 30 siblings. The live documentation site
  (`https://paiml.github.io/xpile/`, deployed from `main` by
  `.github/workflows/book.yml`) was already cited three times in the README that
  crates.io renders as the body of *the same page* — so the front page carried
  two documentation pointers that disagreed, and the one the registry labels
  "Documentation" was the dead one.

- **A13 — PUBLISHED PROSE STALE (never a reason to abort; a reason NOT to
  defer).** The registry is serving a `description` the tree has already
  retired. Every other rule in this list fires because something in the release
  is *wrong*; this one fires because something already published is wrong **and
  the batch is the repair**. It therefore inverts A12: where A12 post-tag says
  *the falsehood ships and you may only disclose it*, A13 says *the falsehood is
  already shipped and uploading removes it.*

  - **Before the tag (§4 step 6, arm 3) it is INFORMATIONAL.** A tree/registry
    divergence pre-tag is the normal, healthy state of any crate corrected since
    the last publish. It blocks nothing.
  - **After the tag, and on Friday, it BINDS A4 and A6 — the two rules that
    defer.** It never delays, retries or reverts a batch. It attaches a *cost*
    to not running one.

  ⚠️ **A4 and A6 are written as costless, and they are not.** Both read as safe
  outcomes — "the *k−1* already published are valid", "a tagged, unpublished
  release is an acceptable outcome" — and for *crate validity* that is true. But
  a deferral also leaves every A13 crate's falsehood live for another week, and
  a **partial** batch leaves it live on exactly the `31 − k` crates that did not
  upload: a partition nobody can name afterwards without re-measuring. If A4 or
  A6 fires, arm 3 must be re-run and **every crate still serving retired prose
  named in the release body and in §7.** Deferring is a decision with a
  published cost; take it with the cost visible.

  ⚠️ **A GATE OVER THE SOURCE OF A PUBLISHED ARTEFACT GOES GREEN AT MERGE; THE
  ARTEFACT CHANGES AT PUBLISH.** `crate_metadata_honesty.rs` (XPILE-CRATEMETA-001,
  PMAT-1465) found six published falsehoods, fixed the manifests, and went green
  — it reads the tree. It is green right now, and **all six are still live on
  crates.io.** Its own header records that the strings "are re-uploaded,
  verbatim, on every Friday publish": the mechanism was written down, and the
  slice was still closed at merge. **A slice that corrects a published claim is
  half-done at merge and completes at the next publish.**

  First run, `0.1.618` (PMAT-1489, the measurement that created this rule):
  **7 of 31** registry descriptions diverge from the tagged tree — `xpile`,
  `xpile-backend`, `xpile-contract-backend`, `xpile-contract-frontend`,
  `bashrs-backend`, `bashrs-frontend`, `ruchy-frontend`. All seven are
  corrections the tree already carries, so the batch clears all seven; six are
  PMAT-1465's named falsehoods and one (`bashrs-backend`) is an understated
  scope list. The two sharpest, quoted from what the registry serves **today**:
  `ruchy-frontend` — "Parses `.ruchy` and lowers to meta-HIR", when `.ruchy`
  input refuses and no Ruchy parser exists (PMAT-1346) — and `bashrs-frontend` —
  "sh/bash/zsh + Makefile/Dockerfile", when both are in that frontend's own
  `refused_claims()` (PMAT-1420).

  ⛔ **`v0.1.618` FIRES A13 WITH SEVEN CRATES, AND PUBLISHING IS WHAT RESOLVES
  IT — DO NOT STOP FRIDAY OVER IT, AND DO NOT LET IT ARGUE FOR A DEFERRAL.**
  Re-run arm 3 on the day: the set is a *measurement*, not a constant, and any
  crate corrected between the tag and Friday joins it.

  ⚠️ Nothing in CI covers this rule. The gate over this section checks a frozen
  literal that stops short of it (see the falsification recipe above), so A13 is
  kept by this file and by review only — and no gate anywhere reads the
  registry, which is the finding, not an oversight to be fixed by re-running
  `workspace-test`.

- **A14 — PUBLISHED API DOCS DEFECTIVE (record it; never touch the batch).**
  §5 step 7 found a defect on the `docs.rs` page of one or more crates. Like
  A11, and unlike A12, its disposition does **not** depend on when it fires:
  the page is rendered from the tagged tree's doc comments and every crates.io
  version is immutable, so **there is nothing a Friday action can repair and
  nothing a deferral can improve.** A14 must never delay, retry or partially
  revert a batch. Record the count and the affected crates in the release body
  and fix the comments for the next version.

  ⚠️ **A14 IS THE ONE RULE WHOSE SUBJECT NO GATE IN THIS REPOSITORY HAS EVER
  BUILT.** `.github/workflows/` contains zero occurrences of `cargo doc`,
  `rustdoc` and `RUSTDOCFLAGS` — verified by grep, not by memory — so no CI job
  has ever rendered the page this rule is about. The job **named `docs` builds
  something else**: `pmat validate-docs` (markdown link integrity across tracked
  `.md` files) and `pmat demo-score` (a README presentation grade). Both are
  real checks of real surfaces. Neither is the API documentation, and the name
  reads as though one of them were — which is why 58 defects accumulated with
  every lane green.

  First run, `0.1.618` (PMAT-1490, the measurement that created this rule):
  **58 defects across 13 of the 30 crates that have a published page** — 38
  `private_intra_doc_links`, 18 `broken_intra_doc_links`, 2
  `redundant_explicit_links`. Worst five: `xpile-ptx-codegen` 12,
  `xpile-wasm-codegen` 9, `xpile-wgsl-codegen` 8, `xpile-wasm-frontend` 7,
  `xpile-spirv-codegen` 7. The dominant class is a **public** doc comment
  pointing at a **private** item: rustdoc drops the anchor and leaves the
  markdown, so `xpile-wasm-frontend`'s page reads, verbatim, `saw it. See
  [refuse_ieee_div].` — an instruction to consult something the page does not
  contain, thirty-three times over.

  Confirmed on a **live published page**, not inferred from a local build:
  `https://docs.rs/xpile-wasm-frontend/0.1.617/xpile_wasm_frontend/` renders its
  own opening paragraph as *"specifically the image of `[xpile-wasm-codegen]` —
  back to canonical meta-HIR"*, brackets included, because a crate name with
  hyphens is not a Rust path. That has been the first thing a reader of that
  crate sees since 2026-07-26, and the batch republishes it.

  ⚠️ The remaining **12** warnings are all in `crates/xpile/src/main.rs`, and
  they are **not** in the 58: the flagship has no `lib` target, so docs.rs
  cannot build it (A12) and none of them reaches a reader. That set is also the
  only one containing `invalid_html_tags` — the CLI usage block writes
  `[--out <path>]`, which rustdoc emits as an unclosed HTML element, so a
  browser renders `<input>` as an actual **text field** and swallows `<t>` and
  `<path>` entirely, while markdown smart-punctuation turns every `--flag` into
  an en dash. **The only page whose text is genuinely mangled is the one page
  that does not exist** — so it is disclosed here and excluded from the count.

  ⛔ **`v0.1.618` FIRES A14 WITH 58 DEFECTS ON 13 CRATES, ALL PREDICTED AND ALL
  DISCLOSED. DO NOT STOP FRIDAY OVER IT.** The tag was cut 2026-07-30 at
  `6b5f6c02` and the doc comments are inside it; the Wednesday 18:00 freeze bars
  `crates/*/src`, so the repair lands in `0.1.619`.

- **A15 — THE RELEASE BODY UNDER-REPORTS THE PYTHON REFUSAL ROSTER (record it;
  never touch the batch).** The mandatory `### What still REFUSES` section gives
  the WASM lane an exhaustive contract-derived roster and the shell lane an
  exhaustive list plus its third-category disclosure, and gives the **Python**
  lane two rows — which are that cycle's additions, not the lane's roster. The
  frontend has **471 `FrontendError::Lower` refusal sites carrying 348 distinct
  message texts** (PMAT-1492, measured at `244091cc`; re-derive with the block
  published in that CHANGELOG entry). Under-reporting a refusal **over-claims a
  capability**: a reader told the tool refuses two exotic type-mismatch shapes
  concludes ordinary Python transpiles, and Python is the MVP path a first-time
  reader tests.

  Like A11 and A14, A15's disposition does **not** depend on when it fires, and
  the premise is stated in the ONE form that survives its own repair. ⚠️ **DO NOT
  verify this rule by diffing the tag against `main`** — PMAT-1492's fix lands on
  `main` AFTER the tag, so the two paragraphs DIFFER, and a `main`-side check
  reports the defect absent. Check the **TAG ONLY**:

  ```
  git show v0.1.618:CHANGELOG.md | sed -n '7416p'
  # → **The Python frontend** refuses an annotated comprehension whose element type
  ```

  An unqualified `refuses` with no scope marker is the defect. §5 step 3 extracts
  the body from `git show <tag>:CHANGELOG.md` (PMAT-1481 constraint), so that is
  the 2-row version the release page publishes no matter what has landed on
  `main` first. `0.1.618` carries the **defect**; `0.1.619` carries the **fix**.
  State that direction in the release body and nowhere invert it: a rule whose
  only action is disclosure vanishes at exit 0 if its direction is wrong
  (PMAT-1491) — and it vanishes just as completely if its verification step reads
  a tree the fix has already touched.

  ⚠️ **DO NOT REPAIR THIS BY GREPPING `src/` FOR REFUSAL STRINGS.** PMAT-1492's
  first draft did, and produced **two false refusals** — keyword arguments in
  calls (`g(x=a)`) and `with open(...) as fh: s = fh.read()` both transpile at
  **exit 0**, though `lib.rs` carries blanket-sounding strings for each. A
  disclosure rule's false positive is itself a published falsehood (PMAT-1489).
  Every row must come from running the shipped CLI, which is the standard the
  section already sets for itself.

  ⛔ **`v0.1.618` FIRES A15. PREDICTED AND DISCLOSED. DO NOT STOP FRIDAY OVER
  IT.** The freeze bars no part of the repair — it is CHANGELOG text — but the
  tag is cut, so the fix reaches readers at `0.1.619`.

## 7. Slip and partial-batch ledger

Every A4 stop and every A6 slip is recorded here, with the reason, on the day
it happens. An unrecorded slip is indistinguishable from a forgotten release.

| Date | Version | What happened | Reason |
|---|---|---|---|
| — | — | no slip or partial batch recorded to date | — |
