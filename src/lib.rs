use zed_extension_api::{self as zed, Result};

struct EmojiCompletionsExtension;

impl zed::Extension for EmojiCompletionsExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        _worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        // The language server is bundled with the extension
        // Paths are relative to the extension's directory
        let server_path = "emoji-language-server/server.mjs";

        let node_path = zed::node_binary_path()?;

        // Debug output
        eprintln!("=== Emoji Completions Debug ===");
        eprintln!("Language server ID: {:?}", language_server_id);
        eprintln!("Node path: {}", node_path);
        eprintln!("Server path: {}", server_path);
        eprintln!("Args: [{}, --stdio]", server_path);
        eprintln!("===============================");

        Ok(zed::Command {
            command: node_path,
            args: vec![server_path.to_string(), "--stdio".to_string()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(EmojiCompletionsExtension);
