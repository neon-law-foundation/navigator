---
name: public-repositories
description: >
  The posture every public Neon Law Foundation repository carries on github.com — issues in Linear, contributions
  closed, `AGPL-3.0-only` over the whole tree, squash-only merges behind a signed and review-gated ruleset. Trigger when
  creating a repository in the `neon-law-foundation` organization, when asked whether a repository is configured
  correctly, when adding a `LICENSE` or `NOTICE`, when a repository page reports the wrong licence, or when a Linear
  issue proposes governance work across repositories. Read it before reaching for `gh api -X PATCH` on repository
  settings, and before writing a licence file by hand — the shape is fixed and copying it is the whole job.
---

# The public repository posture

The Neon Law Foundation publishes six repositories on github.com, all in the `neon-law-foundation` organization:

| Repository | What it holds |
| --- | --- |
| `navigator` | The Rust workspace: the delivery stack and the firm's website |
| `navigator-ux` | The shared React component library Project portals build against |
| `navigator-sample-project-litigation` | The disputes Project application, mounted by `donut-litigation` |
| `navigator-sample-project-transactional` | The company-counsel Project application, mounted by `widget-works` |
| `navigator-sample-project-estate` | The estate Project application, mounted by `montgomery-estate` |
| `homebrew-navigator` | The Homebrew tap for the `navigator` CLI |

`navigator` is the reference. A new repository copies its shape; a repository that has drifted is brought back to it.

## Planning lives in Linear

Linear covers the Foundation's software and nothing else. Every repository above has GitHub Issues, Projects, and the
wiki **off**, so Linear is the one place an issue exists and there is no second board to reconcile.

Two consequences worth knowing before you look for something:

- An issue reference in a commit or PR body points at Linear (`ENG-NN`), and `gh issue` has nothing to return.
- A GitHub issue form in the tree cannot be reached. If one exists, it is a file to remove, not an intake path.

## Contributions are closed

Contributions are closed, and the mechanism is documentary rather than a toggle: `CONTRIBUTING.md` states it, and issues
are off. GitHub has no setting that disables pull requests, so there is nothing to switch and no auto-close workflow to
add.

**Forking stays on.** It is not settable on a public repository anyway — the API refuses `allow_forking` with a 422 —
and it should stay on regardless, because forking is how the rights AGPL § 13 grants get exercised. A closed
contribution path and an open licence are the intended combination.

Anyone asking to contribute goes to `contact@neonlaw.org`.

## One licence: `AGPL-3.0-only`

Every repository is `AGPL-3.0-only` over its whole tree, `templates/` included. `-only`, never `-or-later`: the terms a
repository publishes under are the terms in its own licence file. See [`docs/licensing.md`](../../../docs/licensing.md).

The file shape is two files, and the split is load-bearing:

- **`LICENSE`** — the AGPL text exactly as the Free Software Foundation publishes it, with nothing added. GitHub's
  licence detector, `cargo deny`, SBOM generators, and a reviewer's scanner all read this file, and **any preamble makes
  them report `other` instead of `agpl-3.0`**. A repository whose page does not say Affero has almost always had prose
  prepended here.
- **`NOTICE`** — the copyright line, `SPDX-License-Identifier: AGPL-3.0-only`, and everything the Foundation says in
  its own words: the § 13 network clause, the government-forms carve-out, and the trademark note.

Declare the same identifier everywhere a manifest can hold one — `Cargo.toml`, `package.json`, a `README` SPDX line — so
a reader gets one answer whichever file they open.

Copyright is the Foundation's, which produces the software. NEON LAW is a registered mark, U.S. Reg. No. 6,325,650,
owned by Shook Law PLLC, the firm that operates Navigator and trades as Neon Law. The licence grants copyright, not
trademark; keep the two separate in anything you write.

## The five governance files

```text
LICENSE                 # pristine FSF text
NOTICE                  # the Foundation's own statements
CONTRIBUTING.md         # contributions closed; contact@neonlaw.org; inbound = outbound
SECURITY.md             # support@neonlaw.org
.github/CODEOWNERS      # who reviews
```

## Repository settings

Verified on all six, 2026-08-19:

| Setting | Value | Why |
| --- | --- | --- |
| `has_issues`, `has_projects`, `has_wiki` | `false` | Linear is the one planning surface |
| `allow_squash_merge` | `true` | One commit per pull request |
| `allow_merge_commit`, `allow_rebase_merge` | `false` | A merge commit only has to be unwound later |
| `delete_branch_on_merge` | `true` | The topic branch is spent once it squashes |
| `allow_auto_merge` | `true` | Auto-merge lands a PR once checks pass and threads resolve |
| `secret_scanning` | `enabled` | A credential in a public history is disclosed the moment it lands |

Read the live state before changing anything:

```bash
gh api repos/neon-law-foundation/<repo> --jq '{has_issues, has_projects, has_wiki, allow_squash_merge, allow_merge_commit, allow_rebase_merge, delete_branch_on_merge, allow_auto_merge, license: .license.key, scanning: .security_and_analysis.secret_scanning.status}'
```

Bring a repository to the posture:

```bash
gh api -X PATCH repos/neon-law-foundation/<repo> \
  -F has_issues=false -F has_projects=false -F has_wiki=false \
  -F allow_squash_merge=true -F allow_merge_commit=false -F allow_rebase_merge=false \
  -F delete_branch_on_merge=true -F allow_auto_merge=true
```

Secret scanning is a nested object and needs its own call:

```bash
gh api -X PATCH repos/neon-law-foundation/<repo> --input - <<'JSON'
{"security_and_analysis":{"secret_scanning":{"status":"enabled"}}}
JSON
```

## The default-branch ruleset

One active ruleset on `~DEFAULT_BRANCH`, carrying five rules:

| Rule | Effect |
| --- | --- |
| `deletion` | The default branch cannot be deleted |
| `non_fast_forward` | No force-push |
| `required_signatures` | Every commit is signed — an unsigned commit cannot enter the merge queue |
| `pull_request` | `allowed_merge_methods: ["squash"]`, `required_review_thread_resolution: true` |
| `required_status_checks` | The repository's own gate |

**Name the required check after a job that actually reports on `pull_request`.** A required context no workflow produces
leaves every PR waiting forever on a check that will never arrive.

- `navigator`, `navigator-ux`, and each `navigator-sample-project-*` define a job named `ci`.
- `homebrew-navigator` requires `is the formula seeded` and `audit the formula`.

Read the workflow before writing the ruleset:

```bash
gh api repos/neon-law-foundation/<repo>/commits/main/check-runs --jq '[.check_runs[].name]'
```

`navigator` carries a second ruleset, `release-tags`, over its tag namespace, because it is the repository that cuts
releases.

Create a ruleset:

```bash
gh api -X POST repos/neon-law-foundation/<repo>/rulesets --input - <<'JSON'
{
  "name": "production",
  "target": "branch",
  "enforcement": "active",
  "bypass_actors": [],
  "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
  "rules": [
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    { "type": "required_signatures" },
    { "type": "pull_request",
      "parameters": { "allowed_merge_methods": ["squash"], "dismiss_stale_reviews_on_push": false,
        "require_code_owner_review": false, "require_last_push_approval": false,
        "required_approving_review_count": 0, "required_review_thread_resolution": true } },
    { "type": "required_status_checks",
      "parameters": { "do_not_enforce_on_create": false, "strict_required_status_checks_policy": false,
        "required_status_checks": [ { "context": "ci" } ] } }
  ]
}
JSON
```

Editing an existing ruleset is a `PUT` of the complete object — a partial body drops the rules it omits, so `GET` it
first and send the whole thing back with your change applied.

Set `enforcement` to `active`. A ruleset left at `disabled` reads as protection on the settings page while enforcing
nothing, which is worse than having none.

## Two things to check before you trust a repository is configured

1. **Does its page say `agpl-3.0`?** `license.key` reporting `other` means `LICENSE` is not pristine. Check that
   before reading anything else — it is the failure that hides in plain sight.
2. **Is the ruleset `active`, and does its required check exist?** Those two are what actually gate a merge; the rest
   of the posture is hygiene.

## Related

- [`docs/licensing.md`](../../../docs/licensing.md) — the grant, the forms carve-out, and the trademark split.
- [`docs/gitops.md`](../../../docs/gitops.md) — branch, PR, release, and deploy flow.
- [`CONTRIBUTING.md`](../../../CONTRIBUTING.md) — the text to adapt for a new repository.
- [[triage-projects]] — reconciles Linear against `main` across this set.
