#!/usr/bin/env bash
# Prune superseded prerelease .debs from spyc's flat apt repo.
#
# apt tracks the rc stream (see RELEASE_ENGINEERING.md §8), so every `-rc` tag
# publishes a deb. Those accumulate forever otherwise: the 2.0.x line alone left
# nine rc versions x two arches in the published index long after 2.0.0 shipped.
#
# Policy:
#   * A prerelease (tilde in the version, e.g. `2.0.0~rc.11`) is DELETED once its
#     final release is present. dpkg sorts `2.0.0~rc.11` BELOW `2.0.0`, so such a
#     deb can never be selected again — it is pure index weight.
#   * Otherwise the KEEP most recent prereleases of an in-flight version are kept
#     and older ones deleted. Mirrors release.yml's "3 most recent -rc
#     pre-releases" retention so the two channels agree.
#   * Stable releases are NEVER pruned — users legitimately pin an old version.
#
# Run this AFTER copying the newly built debs in and BEFORE regenerating the
# index, so `Packages` / `Release` / `InRelease` are rebuilt and re-signed over
# the pruned set. Deleting debs without reindexing leaves the signed index
# advertising files that no longer exist, which fails apt's hash check.
#
# Usage: prune-apt-repo.sh [--dry-run] [--keep N] <repo-dir>
set -euo pipefail

KEEP=3
DRY_RUN=0
REPO=""

while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run) DRY_RUN=1; shift ;;
        --keep) KEEP="${2:?--keep needs a number}"; shift 2 ;;
        -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
        -*) echo "unknown flag: $1" >&2; exit 2 ;;
        *) REPO="$1"; shift ;;
    esac
done

[ -n "$REPO" ] || { echo "usage: $(basename "$0") [--dry-run] [--keep N] <repo-dir>" >&2; exit 2; }
[ -d "$REPO" ] || { echo "not a directory: $REPO" >&2; exit 2; }
case "$KEEP" in ''|*[!0-9]*) echo "--keep must be a number (got '$KEEP')" >&2; exit 2 ;; esac

cd "$REPO"

# Every version present, and which of them are stable. Filenames are
# `spyc_<version>_<arch>.deb`; the version field never contains `_`.
versions=$(ls spyc_*_*.deb 2>/dev/null | sed -n 's/^spyc_\(.*\)_[^_]*\.deb$/\1/p' | sort -u || true)
if [ -z "$versions" ]; then
    echo "prune: no debs in $REPO — nothing to do"
    exit 0
fi

stable=$(printf '%s\n' "$versions" | grep -v '~' || true)
prereleases=$(printf '%s\n' "$versions" | grep '~' || true)

if [ -z "$prereleases" ]; then
    echo "prune: no prerelease debs — nothing to do"
    exit 0
fi

doomed=""

# 1. Prereleases whose final release has shipped.
for ver in $prereleases; do
    base="${ver%%~*}"
    if printf '%s\n' "$stable" | grep -qxF "$base"; then
        doomed="$doomed $ver"
    fi
done

# 2. Of what remains (versions still in flight), keep the KEEP newest per base.
#    `sort -V` orders `rc.9` before `rc.11` (digit runs compare numerically),
#    which is what matters within a single base version.
in_flight=""
for ver in $prereleases; do
    if [[ " $doomed " == *" $ver "* ]]; then
        continue
    fi
    in_flight="$in_flight $ver"
done

for base in $(printf '%s\n' $in_flight | sed 's/~.*//' | sort -u); do
    same_base=$(printf '%s\n' $in_flight | grep -F "${base}~" | sort -V)
    total=$(printf '%s\n' "$same_base" | grep -c . || true)
    # Drop all but the newest KEEP. `head -n -N` is GNU-only, so compute the
    # count instead and keep this runnable on macOS as well as the CI runner.
    excess=$((total - KEEP))
    [ "$excess" -gt 0 ] || continue
    for ver in $(printf '%s\n' "$same_base" | head -n "$excess"); do
        doomed="$doomed $ver"
    done
done

if [ -z "${doomed// /}" ]; then
    echo "prune: nothing superseded (kept $(printf '%s\n' "$versions" | wc -l | tr -d ' ') version(s))"
    exit 0
fi

removed=0
for ver in $doomed; do
    for f in spyc_"$ver"_*.deb; do
        [ -e "$f" ] || continue
        if [ "$DRY_RUN" -eq 1 ]; then
            echo "prune: would remove $f"
        else
            rm -f -- "$f"
            echo "prune: removed $f"
        fi
        removed=$((removed + 1))
    done
done

verb=$([ "$DRY_RUN" -eq 1 ] && echo "would remove" || echo "removed")
echo "prune: $verb $removed file(s); kept $(ls spyc_*_*.deb 2>/dev/null | wc -l | tr -d ' ')"
