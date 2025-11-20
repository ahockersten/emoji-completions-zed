use zed_extension_api::{self as zed, Result};

struct EmojiCompletionsExtension;

impl zed::Extension for EmojiCompletionsExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        _worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let server_path = "emoji-language-server/server.mjs";
        let node_path = zed::node_binary_path()?;

        Ok(zed::Command {
            command: node_path,
            args: vec![server_path.to_string(), "--stdio".to_string()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(EmojiCompletionsExtension);
