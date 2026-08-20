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

# Resolve THIS release's run BY THE TAG, never by recency. A tag push sets the
# run's `headBranch` to the tag name, so the tag is an exact filter — and since
# the version is now an argument, it is one this script already holds.
#
# `--limit 1` was not an exact filter, and the difference is a false green. In
# the seconds after a push the new run usually does not exist yet, so `--limit 1`
# returned the PREVIOUS run; when that one had already finished, this script
# printed someone else's `has already completed with 'success'` and exited 0. On
# 26.8.21-hotfix.10 it reported a `kind-ci/**` run from five hours earlier while
# the real release run had not yet been created. The publish is what the tag
# already did, so watching the run is the ONLY verification left — and it was
# exactly the step that silently verified the wrong thing.
run_id=""
for _ in $(seq 1 30); do
    run_id="$(gh run list --workflow=deploy.yml --branch "${tag}" \
        --json databaseId --jq '.[0].databaseId // empty')"
    [ -n "${run_id}" ] && break
    sleep 5
done

if [ -z "${run_id}" ]; then
    echo "warning: no deploy.yml run for ${tag} appeared within 150s." >&2
    echo "         THE TAG IS PUSHED and the publish is under way — this is a" >&2
    echo "         watch failure, not a release failure. Find the run under" >&2
    echo "         the ${tag} ref in the Actions tab and watch it there." >&2
    exit 1
fi

echo "watching deploy.yml run ${run_id} for ${tag}"
# --exit-status so a red release exits non-zero. Without it `gh run watch`
# reports a completed-and-failed run and still exits 0, which is the same false
# green by a different route.
gh run watch "${run_id}" --exit-status
