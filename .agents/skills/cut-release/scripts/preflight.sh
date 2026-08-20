#!/usr/bin/env bash
# Every release check that can fail on this machine, run before a ref exists.
#
# Read-only and safely repeatable: it fetches, inspects, and runs the gate. It
# writes nothing, so a failed run costs a rerun rather than a spent name.
#
# Each check maps to a way the pipeline refuses a tag, and each is free here.
# The tag is immutable and the day's name is spent the moment it is pushed, so
# discovering any of these afterwards costs the day.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
remote="${1:-origin}"

echo "==> fetching ${remote}"
git fetch "${remote}" --tags --prune

echo "==> the release target must be reachable from ${remote}/main"
head="$(git rev-parse HEAD)"
if ! git merge-base --is-ancestor "${head}" "${remote}/main"; then
    echo "FAIL: HEAD (${head:0:12}) is not reachable from ${remote}/main." >&2
    echo "      A PR branch is never a release source; wait for the PR to merge." >&2
    exit 1
fi
echo "    ok — ${head:0:12} is on ${remote}/main"

echo "==> the working tree must be clean"
if [ -n "$(git status --porcelain)" ]; then
    echo "FAIL: uncommitted changes; a release names a commit, not a desk." >&2
    git status --short >&2
    exit 1
fi
echo "    ok"

tag="$("${here}/next-release-tag.sh" "${remote}")"
echo "==> the name this cut may use: ${tag}"

echo "==> the manifest must equal that name"
version="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/version = "(.*)"/\1/')"
if [ "${version}" != "${tag}" ]; then
    echo "FAIL: [workspace.package].version is ${version}, the tag would be ${tag}." >&2
    echo "      Run: cargo run -p cli -- ops release-version --tag ${tag}" >&2
    echo "      cli/build.rs bakes this value into \`navigator --version\`, so a" >&2
    echo "      mismatch ships a binary naming a release its source never heard of." >&2
    exit 1
fi
echo "    ok — ${version}"

echo "==> Cargo.lock must agree with the manifest"
# deploy.yml builds the release with --locked in four places: the provenance step
# and all three CLI archive jobs. --locked refuses a lock the manifest has moved
# past, so a bump that wrote only Cargo.toml fails AFTER the tag is pushed — and
# the release-tags ruleset admits no bypass actor, so the name is spent. Reading
# the manifest alone cannot see this; --locked is what the pipeline actually runs.
if ! cargo metadata --locked --format-version 1 >/dev/null 2>&1; then
    echo "FAIL: Cargo.lock does not match Cargo.toml." >&2
    echo "      The archive jobs build with --locked and would refuse this lock," >&2
    echo "      after the tag is pushed — and a release tag cannot be moved." >&2
    echo "      Run: cargo run -p cli -- ops release-version --tag ${tag}" >&2
    exit 1
fi
echo "    ok"

echo "==> notices must travel with the distributed binary"
cargo run -p cli --quiet -- ops notices --check

echo "==> the workspace gate"
cargo nextest run --workspace
cargo test -p features

echo
echo "preflight passed. The cut is: ${tag}"
