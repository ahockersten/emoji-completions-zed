use zed_extension_api as zed;

struct EmojiCompletionsExtension;

impl zed::Extension for EmojiCompletionsExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        _worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        // The emoji-language-server binary should be in the same directory as this extension
        // After installation via install.sh, it will be at:
        // ~/.config/zed/extensions/emoji-completions/emoji-language-server
        //
        // Zed will resolve the command name by searching in:
        // 1. The extension directory
        // 2. The system PATH
        Ok(zed::Command {
            command: "emoji-language-server".to_string(),
            args: vec![],
            env: Default::default(),
        })
    }
}

zed::register_extension!(EmojiCompletionsExtension);
