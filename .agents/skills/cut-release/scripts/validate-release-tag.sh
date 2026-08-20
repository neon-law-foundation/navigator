#!/usr/bin/env bash
# Validate a release version the OPERATOR chose. It derives nothing.
#
# The version is the operator's decision and this script never invents one: it
# is handed a name and either accepts it or says why `deploy.yml` would refuse
# it. Called with no argument it fails and asks for one, because a default here
# would quietly become a choice.
#
# The rules are deploy.yml's, transcribed rather than remembered — that workflow
# is the authority and it refuses anything else:
#
#   * SHAPE — `YY.M.D`, unpadded, optionally suffixed `-hotfix.N`
#   * DATE  — a plain tag's base must equal TODAY'S UTC date; a `-hotfix.N`
#     base must equal TOMORROW'S, because semver ranks a prerelease below its
#     own base (spec 11.3), so a hotfix hung off today would sort as OLDER
#     than the release it follows
#
# Offline and read-only: no remote, no clock writes, no state. Whether the name
# is still unspent is a question about the remote, so the two scripts that
# already fetch — preflight.sh and tag-and-push.sh — ask it themselves.
set -euo pipefail

tag="${1:-}"
if [ -z "${tag}" ]; then
    echo "usage: validate-release-tag.sh <version>" >&2
    echo "       e.g. 26.8.20, or 26.8.21-hotfix.3 for a second release in one UTC day." >&2
    echo "       The version is yours to choose; this script only checks it." >&2
    exit 2
fi

# SHAPE. The same anchored pattern deploy.yml applies, and deliberately tighter
# than the push filter's `[0-9]*.[0-9]*.[0-9]*` glob — fnmatch's `*` matches
# dots, so `26.8.17.13` and `26.8.x` sail through that glob and die here.
#
# `N` is an unpadded numeric release discriminator, not an hour, and is not
# bounded at 23. Semver forbids a leading zero in a numeric prerelease
# identifier, so `hotfix.03` is not a version at all.
if ! printf '%s' "${tag}" | grep -Eq '^[0-9]{2}\.(0|[1-9][0-9]?)\.(0|[1-9][0-9]?)(-hotfix\.(0|[1-9][0-9]*))?$'; then
    echo "FAIL: ${tag} is not a release version." >&2
    echo "      Expected YY.M.D — two-digit year, unpadded month and day (August is 8," >&2
    echo "      never 08) — optionally suffixed -hotfix.N with an unpadded N." >&2
    exit 1
fi

base="${tag%%-*}"
if [ "${base}" = "${tag}" ]; then prerelease=false; else prerelease=true; fi

# DATE. UTC, always: it is the zone `YY.M.D` has always been derived in, it has
# no DST discontinuity, and the runner clock is already UTC — so a local-zone
# check here would accept a name the workflow then refuses. `date -d` is GNU and
# `date -v` is BSD; a release is cut from either kind of machine. `10#` forces
# base-10 so `08`/`09` are not read as bad octal.
utc_day() { TZ=UTC date -d "$1" +'%y %m %d' 2>/dev/null || TZ=UTC date -v"$2" +'%y %m %d'; }
read -r y m d <<<"$(utc_day 'today' '+0d')"; today="$((10#$y)).$((10#$m)).$((10#$d))"
read -r y m d <<<"$(utc_day 'tomorrow' '+1d')"; tomorrow="$((10#$y)).$((10#$m)).$((10#$d))"

if [ "${prerelease}" = "true" ]; then
    expected="${tomorrow}"
    label="tomorrow's UTC date — a hotfix sorts below its own base, so it hangs off the NEXT day"
else
    expected="${today}"
    label="today's UTC date — one ordinary release per UTC calendar day"
fi

if [ "${base}" != "${expected}" ]; then
    echo "FAIL: ${tag} has base ${base}, but deploy.yml requires ${expected}." >&2
    echo "      That base must be ${label}." >&2
    if [ "${prerelease}" = "false" ] && [ "${base}" = "${tomorrow}" ]; then
        echo "      A plain tag cannot name a future day. Either cut ${today}, or make" >&2
        echo "      this a hotfix (${tomorrow}-hotfix.N) if today's name is already spent." >&2
    fi
    exit 1
fi

if [ "${prerelease}" = "true" ]; then
    echo "    ok — ${tag} is a valid hotfix prerelease on tomorrow's UTC base ${base}"
else
    echo "    ok — ${tag} is a valid release on today's UTC date"
fi
