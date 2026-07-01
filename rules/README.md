# rules

Validation engine for Neon Law Navigator markdown notations. Ships the M/F/S rule families (Markdown lint, Frontmatter
shape, structural checks) behind a single `RuleEngine` that walks a directory and returns a report. Pure: no I/O outside
the walker, no database, no async — easy to embed and easy to reuse outside Neon Law Navigator with a custom rulebook.

## Getting started

```bash
cargo test -p rules
```

That's the whole development loop. Every rule lives in its own file (`f103.rs`, `f104.rs`, …) with the test suite next
to it; the engine glues them together.

To consume the library, depend on it from another workspace crate and hand a `Vec<Box<dyn Rule>>` to `RuleEngine::new`.
The `navigator_default_rules()` and `navigator_markdown_only_rules()` constructors are the bundled starting points.

## Rule codes

A rule code is a **family letter followed by digits — never a trailing letter**: `M057`, not `M057a`. The families are
`M` (Markdown lint), `N` (Notation frontmatter/shape), `E` (event), `C` (content page), and `S` (structural, e.g. line
length). When one concern needs two rules — say a disk-resolution check and a web-portability check — give each its own
number (`M057` and `M061`), because a code carries exactly one severity and reads as one diagnostic. The
`rule_codes_are_a_letter_prefix_then_digits_only` test enforces this grammar across every shipped rule set.

## What's next

`cli` depends on this crate. To add a new notation rule, follow the N-family pattern — one file under `src/` with unit
tests next to the impl — and then wire it into three places so it ships and the stability guard keeps catching
accidental reorders:

1. `pub mod` + `pub use` for the new struct in `src/lib.rs`.
2. `Box::new(...)` in `navigator_default_rules()` in `src/engine.rs`, at the position the rule should hold in the
   canonical order.
3. Append the new code to `EXPECTED_DEFAULT_RULE_CODES` in `engine.rs`'s test module so
   `navigator_default_rule_codes_are_stable` keeps the rule set's presentation order honest.

The engine doesn't care which crate the rule was authored in, so a downstream consumer's custom rule lives in that
consumer's crate, not here.
