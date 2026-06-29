# Neon Law Navigator

Our [mission](https://www.neonlaw.com/foundation/mission) is to improve access to justice. Neon Law Navigator is an
open-source operating system for a modern law practice, built around versioned legal templates, durable workflows,
attorney-reviewed automation, and agent-accessible tooling.

## How it works

Neon Law Navigator contains a shared core of rules and implementations as a command-line executable, an MCP server, and
a website. If you are a lawyer, you are encouraged to use Neon Law Navigator to supplement your existing AI
conversations with reliable legal workflows.

After you install the navigator and a client contacts your firm, a [Project](docs/glossary.md#project) is created for
their [Matter](docs/glossary.md#matter). Every project contains a git repository of its files and
[Templates](docs/notation.md#template), the markdown blueprints that define the intake questions and workflows required
to solve legal problems.

For example, the [Nevada entity-formation](templates/forms/united_states/nevada/state/nv__llc_formation.md) Template
defines the questions required for filling out an entity formation, the workflows that are required, and where that data
is used in the Template body. When you work with your client, you create a [Notation](docs/notation.md#notation) from a
Template. For coders, a Notation is a workflow executed with a durable execution engine.

Templates live on two shelves. [`templates/forms/`](templates/forms/) mirrors the public assets bucket for government
forms (`forms/united_states/nevada/state/nv__llc_formation.pdf`), and each sibling `.md` declares the jurisdiction-first
form code plus the government `origin_url`. [`templates/neon_law/`](templates/neon_law/) holds the firm's product work:
product retainers, engagement letters, product-specific documents, and shared closing letters. Start with [notation
authoring](docs/notation-authoring.md) and the [`templates` README](templates/README.md) before adding a template.

We encourage writing Templates with [Zed](https://zed.dev) and our [LSP](docs/lsp/README.md). It's different from Word,
but once you get used to it, you may find it as productive as we do. Treating legal text like code opens a plethora of
automations that save time, and we see its impact in scaling legal services.

## Install the CLI

On Apple Silicon macOS, install the `navigator` CLI from our Homebrew tap:

```bash
brew install neon-law-foundation/tap/navigator
```

A new `YY.MM.DD` release publishes every day, so `brew upgrade` always pulls the latest build. The published binaries
are Apple-Silicon only; on other platforms — or to hack on the workspace — build from source with `cargo` as below. The
tap and its formulae live at [neon-law-foundation/homebrew-tap](https://github.com/neon-law-foundation/homebrew-tap).

The `navigator-lsp` language server is **not** on the tap. It ships as a published Zed extension: install **Navigator
LSP** from Zed's extension marketplace (`zed: extensions` → search **Navigator LSP** → **Install**) and it pulls the
matching `navigator-lsp` binary from the latest GitHub Release automatically. Building the binary from source — and
pointing any other LSP-aware editor at it — is covered in the [LSP docs](docs/lsp/README.md).

## Getting started

To run Neon Law Navigator on your machine, run the following and review its output. The invocation will spawn a Rust
process and a KIND cluster with common Linux Foundation technologies.

```bash
cargo run -p cli -- start-dev-server
```

If you want to deploy Neon Law Navigator to the cloud, we recommend [Google Cloud](https://cloud.google.com) with
[Doppler](https://doppler.com), [Restate](https://restate.dev), [Twilio](https://twilio.com),
[DNSimple](https://dnsimple.com), and [Mercury](https://mercury.com). GitHub Actions builds and publishes the dated
container images to [ghcr.io](https://ghcr.io); you then roll a published image onto your GKE cluster with one command
and visit your `NAVIGATOR_PRIMARY_DOMAIN`:

```bash
doppler run -- cargo run --release -p cli -- ship --tag YY.MM.DD
```

The full edit → merge → release → deploy lifecycle is documented in [GitOps](docs/gitops.md). Cluster setup lives in
[GKE production](docs/gke-prod.md); a from-scratch fork install is in [OSS install](docs/oss-install.md). For each
command, the error messages will tell you what you need. Loop that back to your LLM of choice, like Claude.

## Contributing

Contributions are welcome under the [Contributor License and Feedback Agreement](CONTRIBUTING.md).

## License

Licensed under either of the following at your option.

- Apache License, Version 2.0 ([local copy](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>) MIT license
  ([local copy](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

## Trademarks

The license above covers the **code**: fork it, modify it, ship it, even sell it under Apache-2.0 or MIT. It does
**not** grant any right to the **names and marks** of **Neon Law**. **"Neon Law"** is a registered trademark of Shook
Law PLLC, U.S. Reg. No. 6,325,650.

The reason is non-deception, not control: a fork wearing the firm's name could imply that Neon Law or its attorneys
stand behind software no attorney reviewed. So **adopt your own name,** and if you are interested, help us enable custom
branding for your organization.

## No Legal Advice

Nothing here is legal advice. Using it does not create an attorney-client relationship. We are not legally responsible
for any actions you take with Neon Law Navigator unless it's agreed and signed in writing.
