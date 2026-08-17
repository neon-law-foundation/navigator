# Features

`features` is Navigator's executable specification: Cucumber scenarios that describe complete lawyer and client journeys
across intake, portals, documents, signatures, filings, and closing.

It serves product authors and maintainers who need proof that independently implemented crates compose into the legal
service promised to a user. Legal flows are feature-first: Gherkin states the journey and Rust exercises the real
application boundaries.

Run the suite with `cargo test -p features`; its custom Cucumber harness remains separate from the workspace nextest
run. See [editing workflows](../docs/editing-workflows.md) for the authoring contract.
