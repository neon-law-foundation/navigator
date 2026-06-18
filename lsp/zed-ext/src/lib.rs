// Zed extension that registers `navigator-lsp` for Markdown buffers.
//
// The binary is resolved in three steps, most specific first:
//   1. an explicit user override in Zed settings —
//        "lsp": { "navigator-lsp": { "binary": { "path": "...", "arguments": [...] } } }
//   2. else `navigator-lsp` found on the worktree's PATH (`cargo install
//      --path lsp`, or any copy on $PATH);
//   3. else the bare name, left for the OS to resolve.
//
// Built with `zed_extension_api`; targets `wasm32-wasip1`.

use zed_extension_api::{
    self as zed, settings::LspSettings, Command, LanguageServerId, Result, Worktree,
};

const SERVER_NAME: &str = "navigator-lsp";

struct NavigatorLsp;

impl zed::Extension for NavigatorLsp {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        // A user `binary` override (path + optional arguments) wins over
        // PATH discovery. Missing settings are not an error — they just
        // mean "fall back to PATH."
        let binary = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|settings| settings.binary);

        let command = binary
            .as_ref()
            .and_then(|b| b.path.clone())
            .or_else(|| worktree.which(SERVER_NAME))
            .unwrap_or_else(|| SERVER_NAME.to_string());

        let args = binary.and_then(|b| b.arguments).unwrap_or_default();

        Ok(Command {
            command,
            args,
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(NavigatorLsp);
