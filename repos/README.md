# repos

The per-Project bare-git-repo store. Every Project is one **append-only, single-branch (`main`)** bare git repository,
served Rust-native from `web`. This crate owns where those repos physically live on the volume and how they are created
with the append-only invariant baked in, so a misconfigured client cannot violate it. The full design is in
[`docs/git-project-repos.md`](../docs/git-project-repos.md).

The store shells to the `git` binary rather than reimplement pack negotiation — lean on the mature upstream. The same
binary backs the smart-HTTP transport in `web::git_http`; this crate is consumed by `web` only.

## The append-only invariant

Each bare repo is created with three guards — two via config, one via a hook — so every write path is covered:

- `receive.denyNonFastForwards = true` — no history rewrite / force push.
- `receive.denyDeletes = true` — no ref deletion.
- a `pre-receive` hook that rejects any ref other than `refs/heads/main` — no second branch, no tags.

The only operation that bypasses this is the admin **governed expunge** (privilege clawback / sealing / lawful
deletion), which acts on the bare repo directly via `expunge_path` and is never a push.

## What it provides

- `RepoStore` — `new` / `from_env` (`NAVIGATOR_GIT_REPO_ROOT`), then `ensure`, `exists`, and `path_for` a Project's repo
  by `Uuid`.
- `commit_as` — write a tree as a named `Author`, advancing `main` fast-forward-only.
- `head_oid` / `read_head_tree` — read the current commit and its file tree (the source of truth for the portal listing
  and the document surfaces migrating onto repo commits).
- `expunge_path` → `ExpungeOutcome` — the governed-deletion escape hatch.

## Getting started

```bash
# Creates throwaway bare repos under a tempdir and asserts the append-only guards hold.
cargo test -p repos
```
