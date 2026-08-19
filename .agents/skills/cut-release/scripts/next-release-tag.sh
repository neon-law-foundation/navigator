#!/usr/bin/env bash
# Print the one release tag that is cuttable right now, and nothing else.
#
# The rule is deploy.yml's, transcribed rather than remembered — that workflow
# is the authority and it refuses anything else:
#
#   * a PLAIN `YY.M.D` tag must equal TODAY'S UTC date
#   * a `-hotfix.N` PRERELEASE base must equal TOMORROW'S UTC date, because
#     semver ranks a prerelease below its own base (spec 11.3), so a hotfix
#     hung off today would sort as OLDER than the release it follows
#
# So the choice is not a judgement call: today's plain name is free or it is
# not. This script answers that from the remote, which is the only place a
# spent name is authoritative — a local tag may be stale in either direction.
#
# Idempotent: it reads, it never writes. Run it as often as you like; it prints
# the same answer until the remote or the UTC clock changes, and the answer is
# always a name that does not yet exist.
set -euo pipefail

remote="${1:-origin}"

# Every tag the remote already carries. `^{}` peel lines are dereferenced
# annotated tags — the same names, so they collapse out with `sort -u`.
taken="$(git ls-remote --tags "${remote}" \
    | sed -e 's|.*refs/tags/||' -e 's|\^{}$||' \
    | sort -u)"

# UTC, always. `date -d` is GNU and `date -v` is BSD; a release is cut from
# either kind of machine, so try both rather than assuming the runner's.
utc_day() { TZ=UTC date -d "$1" +'%y %-m %-d' 2>/dev/null || TZ=UTC date -v"$2" +'%y %-m %-d'; }
read -r y m d <<<"$(utc_day 'today' '+0d')"; today="${y}.${m}.${d}"
read -r y m d <<<"$(utc_day 'tomorrow' '+1d')"; tomorrow="${y}.${m}.${d}"

if ! grep -qxF "${today}" <<<"${taken}"; then
    # Today's ordinary release is unspent: that is the whole answer.
    echo "${today}"
    exit 0
fi

# Today is spent, so this is a prerelease hung off tomorrow. The UTC hour is
# the discriminator because it is monotonic within the day and needs no state;
# if that hour is already taken, walk forward to the first free one. Unpadded:
# semver forbids a leading zero in a numeric prerelease identifier, so
# `hotfix.08` is not a version at all.
hour="$(TZ=UTC date +'%-H')"
for n in $(seq "${hour}" 23) $(seq 0 "${hour}"); do
    candidate="${tomorrow}-hotfix.${n}"
    if ! grep -qxF "${candidate}" <<<"${taken}"; then
        echo "${candidate}"
        exit 0
    fi
done

echo "no release name is free: ${today} is spent and every ${tomorrow}-hotfix.N is taken" >&2
exit 1
