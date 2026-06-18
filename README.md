# Neon Law Navigator

Our [mission](https://www.neonlaw.com/foundation/mission) is to improve access to justice. Neon Law Navigator enables
lawyers to finish more legal projects in the United States of America.

## How it works

Navigator contains a shared core of rules and implementations as a command-line executable, an MCP server, and a
website. If you are a lawyer, you are encouraged to use Navigator to supplement your existing AI conversations with
reliable legal workflows.

After you install the navigator and a client contacts your firm, a [Project](docs/glossary.md#project) is created for
their [Matter](docs/glossary.md#matter). Every project contains a git repository of its files and [notation
templates](docs/notation.md#templates) which frequently use terms from our [glossary](docs/glossary.md). Notation
templates are markdown documents that define the intake questions and workflows required that solve legal problems.

For example, the [Nevada trust](templates/nest/nevada.md) notation template defines the questions required for filling
out an estate, the workflows like notarization that are required, and where that data is used in the notation template
body. When you work with your client, you create a [notation](docs/notation.md#notations) from a notation template. For
coders, a notation is a workflow executed with a durable execution engine.

We encourage writing notation templates with [Zed](https://zed.dev) and our [LSP](docs/lsp/README.md). It's different
from Word, but once you get used to it, you may find it as productive as we do. Treating legal text like code opens a
plethora of automations that save time, and we see its impact in scaling legal services.

## Getting started

To run Navigator on your machine, run the following and review its output. The invocation will spawn a Rust process and
a KIND cluster with common Linux Foundation technologies.

```bash
cargo run -p cli -- start-dev-server
```

If you want to deploy Navigator to the cloud, we recommend [Google Cloud](https://cloud.google.com) with
[Doppler](https://doppler.com), [Restate](https://restate.dev), [Twilio](https://twilio.com),
[DNSimple](https://dnsimple.com), and [Mercury](https://mercury.com). If those are set up, you can then run the
following, then visit what you put in Doppler as the value of `NAVIGATOR_BASE_URL`.

```bash
doppler run -- cargo run -p cli -- deploy
```

For each command, the error messages will tell you what you need. Loop that back to your LLM of choice, like Claude.

## License

Licensed under either of the following at your option.

- Apache License, Version 2.0 ([local copy](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([local copy](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

### Trademarks

The license above covers the **code**: fork it, modify it, ship it, even sell it under Apache-2.0 or MIT. It does
**not** grant any right to the **names and marks** of **Neon Law**. **"Neon Law"** is a registered trademark of Shook
Law PLLC, U.S. Reg. No. 6,325,650.

The reason is non-deception, not control: a fork wearing the firm's name could imply that Neon Law or its attorneys
stand behind software no attorney reviewed. So **adopt your own name,** and if you are interested, help us enable custom
branding for your organization.

## Not Legal Advice

Nothing here is legal advice. Using it does not create an attorney-client relationship. We are not legally responsible
for any actions you take with Neon Law Navigator unless it's agreed and signed in writing.
