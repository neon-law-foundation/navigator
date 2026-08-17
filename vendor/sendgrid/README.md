# SendGrid API contract

This directory holds the repository-pinned SendGrid Mail API contract used to generate Navigator's narrow Rust mail
adapter. Ordinary builds use the reviewed generated source and never fetch an upstream schema.

It serves maintainers who need deterministic, auditable regeneration of the `SendMail` operation without accepting
unreviewed API drift at build time. The contract comes from the MIT-licensed `twilio/sendgrid-oai` repository at commit
`dbe0e0d2e67a0c3c55b222d54df2acda2b9027a7`; its SHA-256 is
`4ba24d0feb8a6b347f60528161d1cf0e7591987d2b3a64a6533685dc2b39d19f`.

Regeneration remains an explicit `navigator dev sendgrid-openapi --regenerate` operation.
