# onboarding-team-map — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-team-map
Created: 2026-05-07T07:40:20.271623+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:40:20.271623+00:00
Role: pm
Type: Note
Title: Onboarding: team map from CODEOWNERS and recent committers

Spec: pm

Purpose: Surface human accountability across spyc. The short answer: spyc is a single-developer project. There is no `CODEOWNERS` file; one human (Derek Marshall) owns every path that's been touched in the last six months.

Observed:
- `CODEOWNERS` is absent. Searched at `.github/CODEOWNERS`, `.bitbucket/CODEOWNERS`, repo root, and via `find . -maxdepth 3 -iname 'CODEOWNERS'` — no match. There is no `.github/` directory in this repo at all (CI lives in `bitbucket-pipelines.yml`).
- Recent contributor enumeration via `git shortlog -sn --use-mailmap --since="6 months ago" HEAD`:
  ```
       321  Derek Marshall
  ```
  Cross-checked with the fallback `git log --use-mailmap --since="6 months ago" --format='%an' | sort | uniq -c | sort -rn` — same single result. 321 commits, one author, six months.
- Pulse aggregates: n/a — premium daemon not available in this deployment. `watercooler_pulse_snapshot(code_path=".")` returned `{"status": "unavailable", "reason": "disabled"}`.
- Vulnerability / inbound contact channel: `derek.marshall@tripstack.com` (`SECURITY.md:7`, `SECURITY.md:115`). PR workflow: Bitbucket PRs into `main`, "one approval required before merge", squash-merge preferred (`CONTRIBUTING.md:18-58`).
- Signing posture: "No commit signing requirement. Bitbucket does not enforce `enforced_signed_commits` on this repo. A compromised dev account could push unsigned commits indistinguishable from real ones, bounded by branch restrictions (PR-only merge into `main`, build-status check, single-user write list)" (`SECURITY.md:99-103`). The "single-user write list" line is itself the maintainer's own characterization of the team shape.
- Project structure mention vs. file shape: `CONTRIBUTING.md:160-172` lists `src/app.rs` as a single file, but the actual layout is the directory `src/app/` with `mod.rs` and `state.rs` (verified by `ls src/`); `AGENTS.md:38` correctly references `src/app/mod.rs`. This does not affect ownership, but it's the only place "Project structure" docs land in `CONTRIBUTING.md`, so any future contributor reading that section in isolation gets a stale layout — flag in `onboarding-risk-register`.

Inferred:
- All paths in the repo are effectively owned by Derek Marshall. — confidence: high — basis: 321/321 commits in the last 6 months by a single author and the maintainer's own "single-user write list" framing (`SECURITY.md:99-103`).
- A `CODEOWNERS` file is genuinely not warranted at the current team size. — confidence: medium — basis: solo project with PR-required merge into `main` (`CONTRIBUTING.md:36-58`); the *signal* a `CODEOWNERS` would carry (auto-request review) has no audience to address. Revisit when the contributor count crosses one or when the GitHub move (`LAUNCH_PREP.md:21-25`) lands and an org-level reviewer pool exists.

Drift findings:
- Found: no `CODEOWNERS` file present; absence noted as the primary finding rather than skipping the topic. (Cross-check #1: paths-that-do-not-exist — n/a, no CODEOWNERS to validate.)
- None found after checking #2 (recent committers absent from CODEOWNERS): there is exactly one recent committer (Derek Marshall, 321 commits, last 6 months) and no CODEOWNERS file, so by construction no one is "absent." Marking `[done — finding recorded]` because the substantive finding is "single-author project with no CODEOWNERS."
- None found after checking #3 (CODEOWNERS-listed owners with no commits in last 6 months): n/a — no CODEOWNERS file. Marking `[n/a — no CODEOWNERS]`.

Next query: `watercooler_search(query="ownership accountability single-developer", thread_topic="onboarding-team-map", code_path=".")`

Related:
- `onboarding-overview` — front door + reading order.
- `onboarding-risk-register` — surfaces the `CONTRIBUTING.md` "Project structure" stale-layout drift caught here.
- `onboarding-product-charter` — the "single-developer project" framing comes from the same maintainer-authored sources.

Provenance:
- Files inspected: `.github/CODEOWNERS` (absent), `.bitbucket/` (absent), `CONTRIBUTING.md:18-58,160-172`, `SECURITY.md:7,99-103,115`, `AGENTS.md:38`.
- Commands run: `find . -maxdepth 3 -iname 'CODEOWNERS'`, `git shortlog -sn --use-mailmap --since="6 months ago" HEAD`, `git log --use-mailmap --since="6 months ago" --format='%an' | sort | uniq -c | sort -rn`, `ls -1 .github 2>/dev/null` (no output), `ls -1 .bitbucket 2>/dev/null` (no output), `ls src/`.
- Watercooler tools called: `watercooler_pulse_snapshot(code_path=".")` → `{"status": "unavailable", "reason": "disabled"}`.
- Sibling entry_ids: `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`, `onboarding-product-charter = 01KR0P18MCE1H57Q5ZTAGKAJNH`.

<!-- Entry-ID: 01KR0P2PC2GBFJ9Z0GPGXZ48Y4 -->

---
Entry: Claude Code (caleb) 2026-06-09T05:19:04.785934+00:00
Role: pm
Type: Note
Title: Onboarding refresh: team map at #311 — still single-developer, no CODEOWNERS, 820 commits/6mo

Spec: pm

Purpose: Refresh of onboarding-team-map from #37 (321 commits/6mo) to #311 (v1.56.0). The #37-era entry (01KR0P2PC2GBFJ9Z0GPGXZ48Y4) stands; this entry re-runs the ownership checks at #311. Short answer is unchanged: spyc is a single-developer project, there is still no CODEOWNERS, one human (Derek Marshall / caleb) owns every recently-touched path.

Observed:
- `CODEOWNERS` is still absent. `find . -name CODEOWNERS -not -path './target/*'` returns nothing; no `.github/CODEOWNERS`, `docs/CODEOWNERS`, or root `CODEOWNERS`. There is no `.github/` workflows dir at all — CI is `bitbucket-pipelines.yml` only.
- Recent committers (since 2025-12-09, ~6mo). `git shortlog -sn --use-mailmap` is unsupported in this git config (errors on `--use-mailmap`); `git -c log.mailmap=true shortlog -sn --since="2025-12-09" HEAD` and the fallback `git log --use-mailmap --since="2025-12-09" --format='%an' | sort | uniq -c | sort -rn` both give:
  ```
   820  Derek Marshall
     3  caleb howard
     1  Jonathon Robinson
  ```
  820 commits dominated by one author. All-time author identities (mailmap not collapsing the two Derek emails): `Derek Marshall <derek.marshall@tripstack.com>` (803), `Derek Marshall <derekmarshall@Dereks-MacBook-Pro.local>` (17), `caleb howard <caleb.howard@koananalytics.com>` (3), `Jonathon Robinson <jonathon.robinson@tripstack.com>` (1). Derek Marshall and caleb are the same maintainer (per CLAUDE.md context); the topology is effectively single-developer.
- Authorship topology (Bitbucket vs GitHub). Commits land as Bitbucket squash-merges from PRs into `main` (Bitbucket is canonical; "squash-merge preferred", one approval). The GitHub remotes (`origin`=git@github.com:calebjacksonhoward/spyc.git, `tripstack-corp`=Tripstack-Corp) are stale mirrors, not the authorship source — so GitHub contributor graphs will not match `git shortlog`. Deep detail: `history-three-repo-lineage` (Bitbucket origin / personal-GH bridge / Tripstack-Corp future home).
- Pulse aggregates: n/a — premium daemon not available. `watercooler_pulse_snapshot(code_path=".")` returned `{"status":"unavailable","reason":"disabled"}` (with the absolute path it returned `invalid_code_path`; with "." it is `disabled`).

Inferred:
- All paths are effectively owned by the single maintainer (Derek Marshall / caleb). — confidence: high — basis: 820/824 recent commits by Derek Marshall, the rest by caleb (same person) plus one Jonathon Robinson commit.
- A CODEOWNERS file is still not warranted at this team size. — confidence: medium — basis: solo project with PR-required squash-merge into `main`; the auto-request-review signal a CODEOWNERS carries has no second reviewer to address. Revisit when contributor count crosses one durably, or when the GitHub move makes an org reviewer pool real.

Drift findings (3 ownership checks):
1. CODEOWNERS path existence — [n/a — no CODEOWNERS]. No CODEOWNERS file exists anywhere in the repo, so there are no owned paths to validate against the tree. This absence is the primary finding and is unchanged from #37.
2. Recent committers absent from CODEOWNERS — [done — finding recorded]. Recent committers (Derek Marshall 820, caleb howard 3, Jonathon Robinson 1) cannot be "absent from CODEOWNERS" because no CODEOWNERS exists. Substantive finding: single-maintainer repo (Derek/caleb) with one stray external commit (Jonathon Robinson, 1) — not enough to warrant ownership rules.
3. CODEOWNERS owners with no recent commits — [n/a — no CODEOWNERS]. No owners are declared, so none can be stale.
- Staleness vs #37 seed: the recent-commit count moved 321 → 820 over the ~6mo window (commit velocity roughly doubled), but the ownership conclusion is identical. The #37 entry's `CONTRIBUTING.md` "Project structure lists src/app.rs as a single file" drift was a risk-register item and is not re-audited here.

Next query: `watercooler_search(query="three repo lineage bitbucket github mirror authorship", thread_topic="history-three-repo-lineage", code_path=".")`

Related:
- `onboarding-overview` — front door + reading order (refreshed this run).
- `onboarding-product-charter` — the "single-developer / internal-becoming-public" framing shares these sources.
- the history/insight corpus — `history-three-repo-lineage` carries the Bitbucket-canonical vs GitHub-mirror authorship topology in full.

Provenance:
- Commands run: `find . -name CODEOWNERS -not -path './target/*'` (none); `git -c log.mailmap=true shortlog -sn --since="2025-12-09" HEAD`; `git log --use-mailmap --since="2025-12-09" --format='%an' | sort | uniq -c | sort -rn`; `git log --format='%an <%ae>' | sort | uniq -c | sort -rn`; `git remote -v`.
- Watercooler tools: `watercooler_pulse_snapshot(code_path=".")` → disabled.
- Prior seed entry read in full: onboarding-team-map 01KR0P2PC2GBFJ9Z0GPGXZ48Y4.
- History thread consulted: history-three-repo-lineage.

<!-- Entry-ID: 01KTND3QQSRA9WMDX1E3ZZBTHF -->

---
Entry: Claude Code (caleb) 2026-06-09T05:25:20.329285+00:00
Role: pm
Type: Note
Title: Onboarding refresh: correction — the "8 git-subprocess sites remain" caveat was wrong (all are test fixtures)

Spec: pm

Purpose: correct a factual caveat in the preceding team-map refresh entry, which noted "8 `Command::new(\"git\")` sites remain" implying production subprocess git. Verification shows all 8 are test-scoped; production is 100% gix. (This does not affect the team-map's ownership findings, which stand.)

Observed:
- All 8 `Command::new("git")` sites are under `#[cfg(test)]` / `mod tests` — verified per-site (`src/git/status.rs:484,506`; `src/git/diff_model/mod.rs:219`; `src/git/discovery.rs:52`; `src/git/worktree.rs:370`; `src/git/blame.rs:113`; `src/app/state/tests/mod.rs:302,373`). `src/git/mod.rs:12-14` documents the `no_subprocess_git_in_production` guard test.
- The team-map's actual subject is unaffected: single-developer repo (Derek Marshall = caleb; 820 commits/6mo), no CODEOWNERS — those findings stand unchanged.

Inferred:
- The caveat was carried over from the coordinator's raw grep (no cfg-context) — confidence: high — basis: the audit above. Recorded here only so a future reader doesn't take the team-map entry's git aside as a production fact.

Drift findings:
- Found — corrected: preceding team-map refresh entry's git-subprocess aside overstated exposure; production has zero git-subprocess calls. The cross-checked ownership findings (CODEOWNERS absence, single-maintainer) are unchanged and authoritative.

Next query: `watercooler_search(query="gix production guard test", thread_topic="onboarding-risk-register", code_path=".")`

Related:
- onboarding-risk-register — full correction with the per-site audit lives there.
- onboarding-architecture — the correct production-100%-gix reading.

Provenance:
- `grep -rn 'Command::new("git")' src` + per-site cfg audit (this session); `src/git/mod.rs:12-14`.
- Corrects the git aside in onboarding-team-map refresh entry 01KTND3QQSRA9WMDX1E3ZZBTHF.

<!-- Entry-ID: 01KTNDF6QH7D1FQPRXTDM8SBW9 -->
