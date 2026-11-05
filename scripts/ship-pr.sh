#!/usr/bin/env bash
# Ship a branch the way CONTRIBUTING.md's "Merging a PR" describes: its red
# commit as one PR, then the fix as a second, each squash-merged on CLEAN.
#
#   scripts/ship-pr.sh WORKTREE BRANCH RED_BRANCH RED_TITLE RED_BODY TITLE BODY
#
# BRANCH is checked out in WORKTREE, its last two commits the red tests
# (HEAD~1) and the fix (HEAD). The script rebases both onto origin/main, runs
# `make lint` on the red commit, pushes it as RED_BRANCH and opens a PR titled
# RED_TITLE (body from the file RED_BODY), and merges it. It then rebases the
# fix onto the new main, lints it, pushes BRANCH as a PR titled TITLE (body
# from the file BODY), and merges that. With RED_BRANCH `-` it ships HEAD
# alone as one PR, and RED_TITLE and RED_BODY are ignored.
#
# Each PR merges only on mergeStateStatus CLEAN, in a bounded loop; BEHIND gets
# `gh pr update-branch`, and DIRTY stops. A check GitHub cancelled without ever
# assigning a runner is re-run, at most three times. A PR that already exists
# is reused, so a stopped run can be started again.
#
# Environment:
#   CARGO_TARGET_DIR       the lane's target dir, for the local `make lint`.
#   SHIP_LOGS              where lint logs go (default: a new temp dir).
#   SHIP_LOCAL_MACOS_LINT  1 = also merge on UNSTABLE when the only check not
#                          passing is "Lint macOS" and the PR's head passed
#                          `make lint` on this Mac. Only with the owner's OK,
#                          when GitHub's macOS runners are out.
#
# Teardown stays manual: `remove_worktree`, then `git branch -D` once the
# branch's own files match main.
set -u

if [ "$#" -ne 7 ]; then
  sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi
W=$1 FIX=$2 RED=$3 RED_TITLE=$4 RED_BODY=$5 FIX_TITLE=$6 FIX_BODY=$7
LOGS=${SHIP_LOGS:-$(mktemp -d "${TMPDIR:-/tmp}/ship-pr.XXXXXX")}
LOCAL_MACOS_LINT=${SHIP_LOCAL_MACOS_LINT:-0}
if [ "$LOCAL_MACOS_LINT" = 1 ] && [ "$(uname -s)" != Darwin ]; then
  echo "SHIP_LOCAL_MACOS_LINT=1 stands in for the macOS lint, so it needs a Mac"
  exit 2
fi
cd "$W" || exit 2
echo "lint logs: $LOGS"

# The commits that passed `make lint` here.
LINTED=""

# Run `make lint` at <sha>, then return to BRANCH.
lint_at() {
  local sha=$1 rc log
  log="$LOGS/lint-$(git rev-parse --short "$sha").log"
  git checkout -q --detach "$sha" || return 1
  make lint > "$log" 2>&1
  rc=$?
  git checkout -q "$FIX" || { echo "could not return to $FIX"; exit 10; }
  echo "LINT $(git rev-parse --short "$sha") exit $rc ($log)"
  [ "$rc" -eq 0 ] && LINTED="$LINTED $sha"
  return "$rc"
}

# Whether PR <branch> may merge on UNSTABLE under SHIP_LOCAL_MACOS_LINT: its
# only check not passing is the macOS lint, and its head passed `make lint`.
macos_lint_only() {
  local b=$1 head others
  [ "$LOCAL_MACOS_LINT" = 1 ] || return 1
  head=$(gh pr view "$b" --json headRefOid --jq .headRefOid)
  others=$(gh pr checks "$b" --json name,bucket \
    --jq '[.[] | select(.bucket != "pass" and .bucket != "skipping") | .name] | join(",")')
  [ "$others" = "Lint macOS (fmt, clippy)" ] || return 1
  case " $LINTED " in *" $head "*) return 0 ;; esac
  echo "head $head was not linted here"
  return 1
}

# Re-run the checks of PR <branch> that GitHub cancelled. `gh pr checks`'s
# text output calls a cancel "fail"; its JSON `bucket` says `cancel`.
rerun_cancelled() {
  local b=$1 runs r
  runs=$(gh pr checks "$b" --json bucket,link \
    --jq '.[] | select(.bucket == "cancel") | .link' 2>/dev/null |
    sed -E 's#.*/actions/runs/([0-9]+)/.*#\1#' | sort -u)
  [ -n "$runs" ] || return 1
  for r in $runs; do
    echo "RERUN cancelled run $r"
    gh run rerun "$r" --failed
  done
}

# Whether PR <branch> has merged, as a refused merge can find it already has.
merged() {
  [ "$(gh pr view "$1" --json state --jq .state 2>/dev/null)" = MERGED ]
}

# Merge PR <branch> once it may: about 45 minutes, then give up.
merge_loop() {
  local b=$1 st reruns=0
  sleep 20
  for _ in $(seq 1 90); do
    st=$(gh pr view "$b" --json mergeStateStatus --jq .mergeStateStatus 2>/dev/null)
    echo "$(date +%H:%M:%S) $b $st"
    case "$st" in
      CLEAN)
        # A refusal ("Base branch was modified": main moved under it) is
        # checked again, as BEHIND or CLEAN.
        if gh pr merge "$b" --squash || merged "$b"; then echo "MERGED $b"; return 0; fi
        sleep 20 ;;
      BEHIND) gh pr update-branch "$b"; sleep 20 ;;
      DIRTY) echo "DIRTY $b: resolve the conflict, then run this again"; return 1 ;;
      UNSTABLE)
        if macos_lint_only "$b"; then
          if gh pr merge "$b" --squash || merged "$b"; then
            echo "MERGED $b (macOS lint run here)"
            return 0
          fi
          sleep 20
          continue
        fi
        if [ "$reruns" -lt 3 ] && rerun_cancelled "$b"; then
          reruns=$((reruns + 1))
          sleep 60
        else
          sleep 30
        fi ;;
      *) sleep 30 ;;
    esac
  done
  echo "TIMEOUT $b"
  return 1
}

# Open a PR for <branch>, or reuse the one it has.
open_pr() {
  local b=$1 title=$2 body=$3
  gh pr create --head "$b" --base main --title "$title" --body-file "$body" ||
    gh pr view "$b" --json number > /dev/null
}

[ "$(git rev-parse --abbrev-ref HEAD)" = "$FIX" ] || { echo "$W is not on $FIX"; exit 2; }
[ -z "$(git status --porcelain)" ] || { echo "$W has uncommitted changes"; exit 2; }
# --no-ff re-signs every commit, so an unsigned one is signed before it's pushed.
git fetch -q origin || { echo "fetch failed"; exit 3; }
git rebase -q --no-ff origin/main ||
  { echo "rebase onto origin/main failed"; git rebase --abort; exit 3; }
git log --oneline -3

if [ "$RED" != "-" ]; then
  red_sha=$(git rev-parse HEAD~1)
  lint_at "$red_sha" || { echo "the red commit fails make lint"; exit 11; }
  git push -q -f origin "${red_sha}:refs/heads/${RED}" || exit 3
  open_pr "$RED" "$RED_TITLE" "$RED_BODY" || exit 4
  merge_loop "$RED" || exit 5
  git fetch -q origin || { echo "fetch failed"; exit 6; }
  git rebase -q origin/main ||
    { echo "rebase onto the merged red failed"; git rebase --abort; exit 6; }
  git log --oneline -2
fi

lint_at "$(git rev-parse HEAD)" || { echo "the fix fails make lint"; exit 12; }
git push -q -f -u origin "$FIX" || exit 7
open_pr "$FIX" "$FIX_TITLE" "$FIX_BODY" || exit 8
merge_loop "$FIX" || exit 9
gh pr view "$FIX" --json state,mergeCommit --jq '"FINAL \(.state) \(.mergeCommit.oid)"'
