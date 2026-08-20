#!/usr/bin/env bash
# Sign the release tag the operator named, push it, and watch the run.
#
#   tag-and-push.sh <version> [remote]
#
# The version is an argument and never a derivation: this script signs the name
# it was handed, so the name that passed preflight is the name that publishes.
# Pass the same one.
#
# Pushing the tag IS the publish, and the `release-tags` ruleset restricts
# deletion, update, and non-fast-forward with no bypass actor — so this script
# refuses every case where a rerun would mean something different from the
# first run, rather than discovering it at the push.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tag="${1:-}"
remote="${2:-origin}"

if [ -z "${tag}" ]; then
    echo "usage: tag-and-push.sh <version> [remote]" >&2
    echo "       Pass the same version preflight.sh accepted." >&2
    exit 2
fi

# Shape and base date, the same two rules deploy.yml applies. Checking them here
# too costs nothing and is the last place a typo is still free: one keystroke
# past preflight would otherwise spend a name on a tag the workflow refuses.
# Whether the name is unspent is NOT asked here — the remote logic below draws
# that distinction properly, because a rerun on an already-pushed tag is fine.
"${here}/validate-release-tag.sh" "${tag}"

git fetch "${remote}" --tags --prune
head="$(git rev-parse HEAD)"

if ! git merge-base --is-ancestor "${head}" "${remote}/main"; then
    echo "refusing: HEAD is not reachable from ${remote}/main." >&2
    exit 1
fi

# Already pushed? Then this is a rerun, and the only safe outcome is to watch
# the run that already exists. A tag cannot be moved, so a remote tag on a
# different commit is a hard stop rather than something to force.
remote_sha="$(git ls-remote --tags "${remote}" "refs/tags/${tag}^{}" | cut -f1)"
if [ -n "${remote_sha}" ]; then
    if [ "${remote_sha}" = "${head}" ]; then
        echo "${tag} is already pushed at ${head:0:12} — nothing to do but watch."
    else
        echo "refusing: ${tag} exists on ${remote} at ${remote_sha:0:12}, not ${head:0:12}." >&2
        echo "          A tag is immutable. Cut the next name instead." >&2
        exit 1
    fi
else
    if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
        local_sha="$(git rev-parse "refs/tags/${tag}^{}")"
        [ "${local_sha}" = "${head}" ] || {
            echo "refusing: local ${tag} points at ${local_sha:0:12}, not ${head:0:12}." >&2
            exit 1
        }
    else
        # Signed: commit verification recognises the nick@neonlaw.com identity.
        git tag -s "${tag}" -m "${tag}"
    fi
    git push "${remote}" "${tag}"
    echo "pushed ${tag} at ${head:0:12}"
fi

run_id="$(gh run list --workflow=deploy.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
gh run watch "${run_id}"
