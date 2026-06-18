# compass

Downstream CLI that shares Navigator's rule engine but ships its own rulebook (Navigator's M/F/S families plus a
separate Compass C-family). The crate exists to prove that `rules` is reusable outside Navigator — same engine,
different bundle of rules, a single-purpose binary on top.

## Getting started

```bash
cargo run -p compass -- validate <dir>

# Or install
cargo install --path compass
compass --help
```

The interface mirrors `navigator validate`: walk a directory, apply every rule, exit non-zero on any violation. Unlike
`cli`, there's no database side — Compass operates purely on the file tree.

## What's next

If a Compass-specific rule comes up, add it under `compass/src/` and register it in the compass rulebook.
Navigator-specific rules should still go in `rules/` and be opted into by Compass if they generalize. The rule engine
itself never needs to change — it takes any `Vec<Box<dyn Rule>>` and runs it.
