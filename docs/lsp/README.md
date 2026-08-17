# Navigator editor integration

`navigator-lsp` brings Navigator's Markdown and Notation diagnostics to any editor that supports the Language Server
Protocol. It provides the same rule results and safe fixes as `navigator validate`, over local JSON-RPC with no
telemetry.

It serves attorneys and engineers who review or edit repository Markdown outside the web editor. The integration is
necessary so local feedback, pull-request review, and CI enforce one rulebook rather than discovering structural errors
at different stages.

Install the `navigator-lsp` binary, then configure the editor to launch it for Markdown files. The server contract lives
in the [`lsp` crate](../../lsp/README.md); editor-specific configuration belongs in the editor or extension.
