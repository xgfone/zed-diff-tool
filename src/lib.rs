use std::fs;

use zed_extension_api::{self as zed, Result, settings::LspSettings};

const SERVER_REPO: &str = "xgfone/zed-diff-tool";
const SERVER_NAME: &str = "diff-tool-lsp";
const SERVER_ID: &str = "diff-tool";

struct DiffToolExtension {
    cached_server_path: Option<String>,
}

impl DiffToolExtension {
    fn server_path(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<String> {
        if let Some(path) = configured_server_path(worktree) {
            return Ok(path);
        }

        if let Some(path) = worktree.which(SERVER_NAME) {
            return Ok(path);
        }

        if let Some(path) = local_bundled_server_path() {
            return Ok(path);
        }

        if let Some(path) = &self.cached_server_path {
            if fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
                return Ok(path.clone());
            }
        }

        let path = self.download_server(language_server_id)?;
        self.cached_server_path = Some(path.clone());
        Ok(path)
    }

    fn download_server(&self, language_server_id: &zed::LanguageServerId) -> Result<String> {
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::latest_github_release(
            SERVER_REPO,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )
        .inspect_err(|error| {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Failed(error.clone()),
            );
        })?;

        let (asset_name, binary_name) = release_asset();
        let version_dir = format!("servers/{}", release.version);
        let binary_path = format!("{version_dir}/{binary_name}");

        if fs::metadata(&binary_path).is_ok_and(|metadata| metadata.is_file()) {
            make_executable_if_needed(&binary_path)?;
            cleanup_old_versions(&version_dir);
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::None,
            );
            return Ok(binary_path);
        }

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| {
                format!(
                    "No release asset named {asset_name}. Expected assets are named \
                     diff-tool-lsp-{platform}-{arch}{suffix}.",
                    platform = platform_name(),
                    arch = arch_name(),
                    suffix = archive_suffix(),
                )
            })
            .inspect_err(|error| {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Failed(error.clone()),
                );
            })?;

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::Downloading,
        );
        fs::remove_dir_all(&version_dir).ok();
        fs::create_dir_all(&version_dir)
            .map_err(|error| format!("failed to create {version_dir}: {error}"))?;

        zed::download_file(
            &asset.download_url,
            &version_dir,
            zed::DownloadedFileType::Zip,
        )
        .map_err(|error| format!("failed to download {asset_name}: {error}"))?;
        make_executable_if_needed(&binary_path)?;
        cleanup_old_versions(&version_dir);

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::None,
        );
        Ok(binary_path)
    }
}

impl zed::Extension for DiffToolExtension {
    fn new() -> Self {
        Self {
            cached_server_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        Ok(zed::Command {
            command: self.server_path(language_server_id, worktree)?,
            args: Vec::new(),
            env: worktree.shell_env(),
        })
    }
}

fn configured_server_path(worktree: &zed::Worktree) -> Option<String> {
    if let Ok(settings) = LspSettings::for_worktree(SERVER_ID, worktree)
        && let Some(binary) = settings.binary
        && let Some(path) = binary.path
    {
        return Some(path);
    }

    worktree
        .shell_env()
        .into_iter()
        .find_map(|(key, value)| (key == "ZED_DIFF_TOOL_LSP").then_some(value))
}

fn local_bundled_server_path() -> Option<String> {
    let binary_name = if matches!(zed::current_platform().0, zed::Os::Windows) {
        format!("{SERVER_NAME}.exe")
    } else {
        SERVER_NAME.to_string()
    };
    let path = format!("servers/local/{binary_name}");
    fs::metadata(&path)
        .is_ok_and(|metadata| metadata.is_file())
        .then_some(path)
}

fn release_asset() -> (String, String) {
    let binary_name = if matches!(zed::current_platform().0, zed::Os::Windows) {
        format!("{SERVER_NAME}.exe")
    } else {
        SERVER_NAME.to_string()
    };
    (
        format!(
            "{SERVER_NAME}-{platform}-{arch}{suffix}",
            platform = platform_name(),
            arch = arch_name(),
            suffix = archive_suffix()
        ),
        binary_name,
    )
}

fn platform_name() -> &'static str {
    match zed::current_platform().0 {
        zed::Os::Mac => "macos",
        zed::Os::Linux => "linux",
        zed::Os::Windows => "windows",
    }
}

fn arch_name() -> &'static str {
    match zed::current_platform().1 {
        zed::Architecture::Aarch64 => "aarch64",
        zed::Architecture::X8664 => "x86_64",
        zed::Architecture::X86 => "x86",
    }
}

fn archive_suffix() -> &'static str {
    ".zip"
}

fn make_executable_if_needed(path: &str) -> Result<()> {
    if !matches!(zed::current_platform().0, zed::Os::Windows) {
        zed::make_file_executable(path)?;
    }
    Ok(())
}

fn cleanup_old_versions(current_version_dir: &str) {
    if let Ok(entries) = fs::read_dir("servers") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.to_string_lossy() != current_version_dir
                && path.file_name().is_some_and(|name| name != "local")
            {
                fs::remove_dir_all(path).ok();
            }
        }
    }
}

zed::register_extension!(DiffToolExtension);
