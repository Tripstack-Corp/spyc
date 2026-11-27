## Summary

-

## Test plan

-

---

<!-- The repo's conventions — see CONTRIBUTING.md / AGENTS.md. -->

- [ ] `make check` passes (fmt-check + clippy + test; supply-chain checks run in `audit.yml`)
- [ ] No version bump: `main` stays `N.M.0-CURRENT` until the release PR
- [ ] Each new or changed test was watched failing: break what it covers, see it report, put it back
- [ ] A PR in a campaign, or one touching the jar, sessions, handovers or the release, has a refuter's verdict under `## Refuter` (CONTRIBUTING.md → "Campaigns")
- [ ] Docs updated in the same commit as the change touches them: `docs/features/`, `docs/KEYBINDINGS.md`, `src/ui/help.rs`, `CONFIGURATION.md` or `docs/config/`
- [ ] The PR title is `type(scope): subject`; under squash merge it becomes the CHANGELOG entry
