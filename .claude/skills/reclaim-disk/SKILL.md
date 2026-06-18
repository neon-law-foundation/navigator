---
name: reclaim-disk
description: >
  General host-disk reclaim for the workspace — the first stop when the disk is full or a build dies with "No space left
  on device". Measures first (`df -h` + the biggest consumers), then reclaims in safest-first stages from whichever hog
  is actually largest. The two usual hogs are the Rust `target/` directory (which routinely balloons past 100 GB on this
  workspace — `target/debug/incremental` alone is often the single biggest reclaim, and a full `cargo clean` frees it
  all at the cost of one slow rebuild) and Docker (images, build cache, volumes, and leaked `postgres:17-alpine`
  testcontainers). It owns the cargo/`target` + `~/.cargo` reclaim itself and delegates the Docker side to
  [[docker-cleanup]]. Also sweeps `~/.cargo/registry` caches, stray `target/tmp`, and old `target/release` artifacts.
  Trigger when the user says "reclaim disk", "free up space", "disk is full", "clean up the workspace", "the target dir
  is huge", or when any `cargo`/`rustc`/`ld` step fails with `No space left on device (os error 28)` / a linker bus
  error from a full disk. Always print `df -h` and the top consumers before and after so the user sees what came back,
  and lead the closing message with the gigabytes reclaimed.
---

# reclaim-disk

The umbrella disk-reclaim skill. Docker is not the only thing that fills a Rust developer's disk — on this workspace the
`target/` tree is usually the bigger hog. This skill **measures first**, then reclaims from whichever consumer is
actually largest, safest stage first. For the Docker portion it hands off to [[docker-cleanup]] rather than duplicating
those stages.

## When to invoke

- User says "reclaim disk", "free up space", "disk is full", "clean up the workspace", "the `target` dir is huge".
- A `cargo build` / `cargo test` / `rustc` / `ld` step fails with `No space left on device (os error 28)`, a
  `query-cache.bin` write failure, or a linker `collect2: fatal error: ld terminated with signal 7 [Bus error]` — those
  bus errors are usually the disk filling mid-link, not a code bug.
- `df -h` shows the root filesystem at or near 100%.

## When NOT to invoke

- The "disk full" is actually an inode exhaustion or a single runaway log — measure first (below); if `target/` and
  Docker are both small, this skill is the wrong tool.
- A build is failing for a real compile reason and the disk has plenty of headroom — read the actual error instead.

## Step 0 — measure first, always

Never reclaim blind. Find the hog before you delete anything:

```bash
df -h .                                              # how full, and how much you need back
du -sh target ~/.cargo 2>/dev/null | sort -rh        # the two usual Rust hogs
du -sh target/* 2>/dev/null | sort -rh | head        # which target subdir (debug is almost always it)
du -sh target/debug/incremental 2>/dev/null          # incremental cache — frequently 100 GB+ on its own
docker system df 2>/dev/null                          # is Docker also a hog? (then also run docker-cleanup)
```

Decide from the numbers which stages below to run, and in what order. Lead with the biggest reclaim that is also the
safest.

## Stage R1 — incremental compile cache (safe, usually the biggest single win)

`target/debug/incremental` is rebuilt on demand; deleting it only forgoes incremental-compilation speedups on the next
build, never correctness. On this workspace it routinely sits at 100 GB+.

```bash
rm -rf target/debug/incremental        # frees the cache; cargo recreates it next build
```

This is the move when the disk is full *right now* and you need headroom to let the current `cargo test`/`build` finish
— it frees the most disk for the least future cost (deps stay compiled; only the incremental cache is gone).

## Stage R2 — registry + git caches (safe)

Downloaded crate tarballs, extracted sources, and git checkouts under `~/.cargo` are all re-fetched on demand:

```bash
du -sh ~/.cargo/registry/cache ~/.cargo/registry/src ~/.cargo/git 2>/dev/null
rm -rf ~/.cargo/registry/cache/*       # downloaded .crate tarballs — re-downloaded as needed
rm -rf ~/.cargo/registry/src/*         # extracted sources — re-extracted from the tarballs
# Leave ~/.cargo/registry/index alone unless desperate; re-fetching it is slow.
```

Smaller than R1 in absolute terms but free of any rebuild cost.

## Stage R3 — full target clean (safe, but costs a full rebuild)

The nuclear option for the Rust side. Frees the *entire* `target/` tree, but the next build recompiles the whole
workspace from scratch (slow — many minutes on this workspace):

```bash
cargo clean                            # removes all of target/ (debug + release + artifacts)
# Targeted alternative — clears one crate's artifacts without a full-workspace rebuild:
cargo clean -p web -p views
# Or just drop stale release artifacts you aren't running:
rm -rf target/release target/tmp
```

Prefer R1 (+ R2) first; reach for a full `cargo clean` only when R1 didn't free enough, or when you specifically want a
clean rebuild.

## Stage D — Docker (delegate to docker-cleanup)

If Step 0 shows Docker is also a hog — many GB in images/build cache/volumes, or a swarm of leaked `postgres:17-alpine`
testcontainers (the Rust `testcontainers` crate has no Ryuk reaper, so every `cargo test` binary orphans one) — run the
[[docker-cleanup]] skill. It owns the Docker stages (orphan sweep → images → build cache → volumes) with their own
safety profiles; don't re-implement them here. The leaked-testcontainer sweep is the common overlap: a full workspace
test run both fills `target/` and orphans containers, so a real cleanup usually runs **this skill's R1 +
docker-cleanup's Stage 0** together.

## Reporting

Print `df -h .` and the top consumers before and after, and put the reclaimed total in the closing line — the user is
doing this to get disk back, so lead with the number.

```text
| Stage                     | Before   | After   | Reclaimed |
| target/debug/incremental  | 129 GB   | 0 B     | ~129 GB   |
| ~/.cargo registry caches  | 12 GB    | 0 B     | ~12 GB    |
| Docker (via docker-cleanup) | …      | …       | …         |
| Root filesystem free      | 1.4 GB   | 142 GB  | +140 GB   |
```

## The durable fix is upstream of this skill

Recurring `target/` blowup and leaked testcontainers are maintenance symptoms, not destiny. The container leak's real
fix belongs in `store/src/test_support.rs` (reap the container, or wire up `testcontainers` reuse) — see
[[docker-cleanup]]. For `target/`, consider a smaller incremental footprint or periodic `cargo clean` in CI rather than
letting a dev disk drift to 100%. Until those land, this sweep is the recurring reclaim.
