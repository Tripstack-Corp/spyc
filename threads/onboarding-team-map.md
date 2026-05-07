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
