# Views

`views` holds shared presentation data and server-side render helpers without owning routes, persistence, authorization,
or workflow decisions.

It serves the Rust web surfaces and the generated communications that remain server-rendered. This separation is
necessary so handlers own behavior while views stay deterministic, testable, and reusable across brands.

Browser assets and interactive presentation live outside this crate. See [workspace layout](../docs/workspace-layout.md)
and [design system](../docs/design.md) for the complete presentation boundary.
