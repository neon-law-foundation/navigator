# Contributing to Neon Law Navigator

Thanks for your interest in Neon Law Navigator. This project has an intentionally narrow contribution model.

## We accept issues; we do not accept pull requests

To keep the codebase coherent and the design decisions discussable in one place, **GitHub Issues are the only
contribution surface we accept from the community.**

Open an issue to:

- report a bug, with steps to reproduce and the expected vs. actual behavior; propose a feature, with the motivating use
  case and the rough shape of the change; ask a design question; flag a doc or markdown lint problem.

**Pull requests opened by external contributors will be closed without review.** This is not a comment on the quality of
the work — it is a process choice. We want the design conversation to happen on the issue *before* any code is written,
so that the implementation lands once and lands right.

Once an issue has been triaged and there is agreement on the approach, an AI coding agent (working on behalf of the
maintainers) will implement the change and open the pull request. The maintainers review and merge that PR. The original
issue author is credited in the PR description.

If you want to attach a code sketch to an issue, paste it inline as a fenced code block — that is fine and often
helpful. Just do not open a PR.

## Before you open an issue

- **Search first.** Check existing open and closed issues; the question or bug may already be tracked. **One issue per
  concern.** Split unrelated bugs and feature requests into separate issues so each can be discussed on its own thread.
- **Include versions and environment** for bug reports: Rust toolchain (`rustc --version`), OS, and whether you
  reproduced against KIND (`navigator start-dev-server`) or another environment.

## Code of Conduct

Participation in this project is governed by the [Code of Conduct](CODE_OF_CONDUCT.md). By opening an issue or otherwise
engaging with the project, you agree to abide by it.

## License

Neon Law Navigator is dual-licensed under your choice of the [Apache License, Version 2.0](LICENSE-APACHE) or the [MIT
license](LICENSE-MIT). This is the standard licensing convention across the Rust ecosystem: the Apache-2.0 grant carries
an explicit patent license, while the MIT option preserves compatibility with GPLv2 and other licenses whose terms the
Apache patent clause would otherwise conflict with.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work — including
code snippets, designs, and prose — is accepted under the same dual Apache-2.0 / MIT terms, with no additional terms or
conditions.
