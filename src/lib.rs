use std::fs;
use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct EmojiCompletionsExtension {
    cached_binary_path: Option<String>,
}

#[derive(Clone)]
struct EmojiServerBinary {
    path: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

impl EmojiCompletionsExtension {
    fn language_server_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<EmojiServerBinary> {
        let args = Vec::new();
        let (platform, arch) = zed::current_platform();
        let env = match platform {
            zed::Os::Mac | zed::Os::Linux => worktree.shell_env(),
            zed::Os::Windows => Vec::new(),
        };

        // Check if user has configured a custom binary path
        if let Ok(LspSettings {
            binary: Some(binary),
            ..
        }) = LspSettings::for_worktree("emoji-language-server", worktree)
        {
            if let Some(path) = binary.path {
                return Ok(EmojiServerBinary {
                    path: path.clone(),
                    args,
                    env,
                });
            }
        }

        // Check if binary is in PATH
        if let Some(path) = worktree.which("emoji-language-server") {
            return Ok(EmojiServerBinary { path, args, env });
        }

        // Check if we have a cached binary path
        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).map_or(false, |stat| stat.is_file()) {
                return Ok(EmojiServerBinary {
                    path: path.clone(),
                    args,
                    env,
                });
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release =
            zed::github_release_by_tag_name("ahockersten/emoji-completions-zed", VERSION)?;

        let arch_str: &str = match arch {
            zed::Architecture::Aarch64 => "aarch64",
            zed::Architecture::X86 => "i686",
            zed::Architecture::X8664 => "x86_64",
        };

        let os: &str = match platform {
            zed::Os::Mac => "darwin",
            zed::Os::Linux => "linux",
            zed::Os::Windows => "windows",
        };

        let extension = match platform {
            zed::Os::Windows => ".exe",
            _ => "",
        };

        let asset_name: String = format!("emoji-language-server-{}-{}{}", os, arch_str, extension);

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| {
                format!(
                    "Asset {} not found in release {}",
                    asset_name, release.version
                )
            })?;

        let binary_path = asset_name.clone();

        if !fs::metadata(&binary_path).map_or(false, |stat| stat.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(
                &asset.download_url,
                &binary_path,
                zed::DownloadedFileType::Uncompressed,
            )
            .map_err(|e| format!("failed to download file: {e}"))?;

            zed::make_file_executable(&binary_path)?;
        }

        self.cached_binary_path = Some(binary_path.clone());
        Ok(EmojiServerBinary {
            path: binary_path,
            args,
            env,
        })
    }
}

impl zed::Extension for EmojiCompletionsExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = self.language_server_binary(language_server_id, worktree)?;
        Ok(zed::Command {
            command: binary.path,
            args: binary.args,
            env: binary.env,
        })
    }
}

zed::register_extension!(EmojiCompletionsExtension);
