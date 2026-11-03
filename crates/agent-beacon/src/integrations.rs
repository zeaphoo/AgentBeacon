//! 内置 agent 集成插件：安装、升级、卸载。
//!
//! 插件源码通过 `include_str!` 编译进二进制；安装时把版本号写入文件头，
//! 服务启动时会自动升级已安装且受管的插件。

use std::fs;
use std::path::{Path, PathBuf};

use crate::VERSION;

const PI_SOURCE: &str = include_str!("../assets/integrations/agent-beacon-hook-pi.js");
const OPENCODE_SOURCE: &str = include_str!("../assets/integrations/agent-beacon-hook-opencode.js");

/// 旧版本安装的通用文件名，升级时自动迁移到带目标名的文件。
const LEGACY_FILE_NAME: &str = "agent-beacon.js";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Pi,
    OpenCode,
    Dsh,
}

impl Target {
    pub const ALL: [Target; 3] = [Target::Pi, Target::OpenCode, Target::Dsh];

    pub fn id(self) -> &'static str {
        match self {
            Target::Pi => "pi",
            Target::OpenCode => "opencode",
            Target::Dsh => "dsh",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Target::Pi => "Pi",
            Target::OpenCode => "OpenCode",
            Target::Dsh => "DeepSeek Harness",
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Target::Pi => "agent-beacon-hook-pi.js",
            Target::OpenCode => "agent-beacon-hook-opencode.js",
            Target::Dsh => "agent-beacon-hook-dsh.js",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Target::Pi => PI_SOURCE,
            Target::OpenCode => OPENCODE_SOURCE,
            Target::Dsh => "",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pi" => Some(Target::Pi),
            "opencode" => Some(Target::OpenCode),
            "dsh" => Some(Target::Dsh),
            _ => None,
        }
    }

    pub fn default_dir(self) -> Result<PathBuf, String> {
        match self {
            Target::Pi => {
                if let Some(dir) = std::env::var_os("AGENT_BEACON_PI_DIR") {
                    return Ok(PathBuf::from(dir));
                }
                Ok(home_dir()?.join(".pi").join("agent").join("extensions"))
            }
            Target::Dsh => crate::dsh::paths().map(|paths| paths.home),
            Target::OpenCode => {
                if let Some(dir) = std::env::var_os("AGENT_BEACON_OPENCODE_DIR") {
                    return Ok(PathBuf::from(dir));
                }
                let base = opencode_config_dir()?;
                let plugins = base.join("plugins");
                let legacy = base.join("plugin");
                if !plugins.exists() && legacy.exists() {
                    Ok(legacy)
                } else {
                    Ok(plugins)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Installed,
    Upgraded,
    UpToDate,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Installed => "installed",
            Outcome::Upgraded => "upgraded",
            Outcome::UpToDate => "up to date",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Status {
    pub target: Target,
    pub dir: PathBuf,
    pub path: PathBuf,
    pub installed_version: Option<String>,
    pub embedded_version: &'static str,
    pub state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    NotInstalled,
    UpToDate,
    UpgradeAvailable,
    Unmanaged,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::NotInstalled => "not installed",
            State::UpToDate => "up to date",
            State::UpgradeAvailable => "upgrade available",
            State::Unmanaged => "unmanaged file",
        }
    }
}

fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "cannot resolve user home directory".to_string())
}

/// OpenCode 文档使用 `~/.config/opencode`（含 macOS）；Windows 使用 `%APPDATA%`。
fn opencode_config_dir() -> Result<PathBuf, String> {
    if cfg!(windows) {
        return dirs::config_dir()
            .map(|dir| dir.join("opencode"))
            .ok_or_else(|| "cannot resolve config directory".to_string());
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir).join("opencode"));
    }
    Ok(home_dir()?.join(".config").join("opencode"))
}

fn render(target: Target) -> String {
    target.source().replace("__VERSION__", VERSION)
}

pub(crate) fn is_managed(content: &str) -> bool {
    content
        .lines()
        .take(3)
        .any(|line| line.starts_with("// @agent-beacon-integration:"))
}

pub(crate) fn managed_version(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.strip_prefix("// @agent-beacon-version:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn resolve_dir(target: Target, override_dir: Option<&Path>) -> Result<PathBuf, String> {
    match override_dir {
        Some(dir) => Ok(dir.to_path_buf()),
        None => target.default_dir(),
    }
}

pub fn status(target: Target, override_dir: Option<&Path>) -> Result<Status, String> {
    if target == Target::Dsh {
        return crate::dsh::status();
    }
    let dir = resolve_dir(target, override_dir)?;
    let path = dir.join(target.file_name());
    let legacy = dir.join(LEGACY_FILE_NAME);

    let (installed_version, state) = match fs::read_to_string(&path) {
        Ok(content) if !is_managed(&content) => (managed_version(&content), State::Unmanaged),
        Ok(content) => {
            let version = managed_version(&content);
            let state = if version.as_deref() == Some(VERSION) {
                State::UpToDate
            } else {
                State::UpgradeAvailable
            };
            (version, state)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let legacy_content = fs::read_to_string(&legacy).ok();
            match legacy_content.filter(|content| is_managed(content)) {
                Some(content) => (managed_version(&content), State::UpgradeAvailable),
                None => (None, State::NotInstalled),
            }
        }
        Err(error) => {
            return Err(format!("failed to read {}: {error}", path.display()));
        }
    };
    Ok(Status {
        target,
        dir,
        path,
        installed_version,
        embedded_version: VERSION,
        state,
    })
}

pub fn install(
    target: Target,
    override_dir: Option<&Path>,
    force: bool,
) -> Result<(Outcome, PathBuf), String> {
    if target == Target::Dsh {
        return crate::dsh::install();
    }
    let dir = resolve_dir(target, override_dir)?;
    let path = dir.join(target.file_name());
    let legacy = dir.join(LEGACY_FILE_NAME);
    let rendered = render(target);

    if path.exists() {
        let existing = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let managed = is_managed(&existing);
        if !managed && !force {
            return Err(format!(
                "{} already exists and is not managed by agent-beacon; use --force to overwrite",
                path.display()
            ));
        }
        if managed && managed_version(&existing).as_deref() == Some(VERSION) && !force {
            let _ = remove_legacy_managed(&dir);
            return Ok((Outcome::UpToDate, path));
        }
        write_atomic(&path, &rendered)?;
        let _ = remove_legacy_managed(&dir);
        let outcome = if managed {
            Outcome::Upgraded
        } else {
            Outcome::Installed
        };
        return Ok((outcome, path));
    }

    let legacy_managed = fs::read_to_string(&legacy)
        .map(|content| is_managed(&content))
        .unwrap_or(false);
    write_atomic(&path, &rendered)?;
    let _ = remove_legacy_managed(&dir);
    let outcome = if legacy_managed {
        Outcome::Upgraded
    } else {
        Outcome::Installed
    };
    Ok((outcome, path))
}

pub fn upgrade(target: Target, override_dir: Option<&Path>) -> Result<(Outcome, PathBuf), String> {
    if target == Target::Dsh {
        return crate::dsh::upgrade();
    }
    let dir = resolve_dir(target, override_dir)?;
    let path = dir.join(target.file_name());
    let legacy = dir.join(LEGACY_FILE_NAME);

    if !path.exists() {
        let legacy_content = fs::read_to_string(&legacy).map_err(|error| {
            format!(
                "{} is not installed; run integrate install first ({error})",
                path.display()
            )
        })?;
        if !is_managed(&legacy_content) {
            return Err(format!(
                "{} is not installed; run integrate install first",
                path.display()
            ));
        }
        write_atomic(&path, &render(target))?;
        remove_legacy_managed(&dir)?;
        return Ok((Outcome::Upgraded, path));
    }

    let existing = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if !is_managed(&existing) {
        return Err(format!(
            "{} is not managed by agent-beacon; refusing to overwrite",
            path.display()
        ));
    }
    if managed_version(&existing).as_deref() == Some(VERSION) {
        let _ = remove_legacy_managed(&dir);
        return Ok((Outcome::UpToDate, path));
    }
    write_atomic(&path, &render(target))?;
    let _ = remove_legacy_managed(&dir);
    Ok((Outcome::Upgraded, path))
}

pub fn uninstall(target: Target, override_dir: Option<&Path>) -> Result<PathBuf, String> {
    if target == Target::Dsh {
        return crate::dsh::uninstall();
    }
    let dir = resolve_dir(target, override_dir)?;
    let path = dir.join(target.file_name());
    let legacy = dir.join(LEGACY_FILE_NAME);
    let mut removed = None;

    if path.exists() {
        let existing = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        if !is_managed(&existing) {
            return Err(format!(
                "{} is not managed by agent-beacon; refusing to delete",
                path.display()
            ));
        }
        fs::remove_file(&path).map_err(|error| format!("failed to delete {}: {error}", path.display()))?;
        removed = Some(path);
    }

    if legacy.exists() {
        let existing = fs::read_to_string(&legacy)
            .map_err(|error| format!("failed to read {}: {error}", legacy.display()))?;
        if is_managed(&existing) {
            fs::remove_file(&legacy)
                .map_err(|error| format!("failed to delete {}: {error}", legacy.display()))?;
            removed = Some(legacy);
        }
    }

    removed.ok_or_else(|| format!("{} is not installed", dir.display()))
}

/// 服务启动时调用：只升级已安装且受管的插件，不主动安装。
pub fn upgrade_installed() -> Vec<(Target, Result<Outcome, String>)> {
    let mut results = Vec::new();
    if let Some(result) = crate::dsh::upgrade_installed() {
        results.push((Target::Dsh, result));
    }
    for target in [Target::Pi, Target::OpenCode] {
        let Ok(dir) = target.default_dir() else {
            continue;
        };
        let managed = [dir.join(target.file_name()), dir.join(LEGACY_FILE_NAME)]
            .iter()
            .any(|path| {
                fs::read_to_string(path)
                    .map(|content| is_managed(&content))
                    .unwrap_or(false)
            });
        if !managed {
            continue;
        }
        results.push((target, upgrade(target, None).map(|(outcome, _)| outcome)));
    }
    results
}

/// 删除旧版本安装的受管文件；非受管文件保留。
fn remove_legacy_managed(dir: &Path) -> Result<(), String> {
    let path = dir.join(LEGACY_FILE_NAME);
    if !path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if is_managed(&content) {
        fs::remove_file(&path)
            .map_err(|error| format!("failed to delete legacy plugin {}: {error}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("invalid path {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create directory {}: {error}", parent.display()))?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&tmp, content).map_err(|error| format!("failed to write {}: {error}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        format!("failed to replace {}: {error}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            + u128::from(COUNTER.fetch_add(1, Ordering::Relaxed));
        let dir = std::env::temp_dir().join(format!("agent-beacon-integration-{tag}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rendered_plugin_has_version_and_marker() {
        let content = render(Target::Pi);
        assert!(!content.contains("__VERSION__"));
        assert!(content.contains(&format!("@agent-beacon-version: {VERSION}")));
        assert!(is_managed(&content));
        assert!(content.contains("/api/v1/events"));
    }

    #[test]
    fn install_upgrade_uninstall_roundtrip() {
        let dir = temp_dir("roundtrip");
        let (outcome, path) = install(Target::Pi, Some(&dir), false).unwrap();
        assert_eq!(outcome, Outcome::Installed);
        assert!(path.exists());
        assert_eq!(
            status(Target::Pi, Some(&dir)).unwrap().state,
            State::UpToDate
        );

        let content = fs::read_to_string(&path).unwrap();
        let old = content.replacen(
            &format!("@agent-beacon-version: {VERSION}"),
            "@agent-beacon-version: 0.0.0",
            1,
        );
        fs::write(&path, old).unwrap();
        assert_eq!(
            status(Target::Pi, Some(&dir)).unwrap().state,
            State::UpgradeAvailable
        );

        let (outcome, _) = upgrade(Target::Pi, Some(&dir)).unwrap();
        assert_eq!(outcome, Outcome::Upgraded);
        assert_eq!(
            status(Target::Pi, Some(&dir)).unwrap().state,
            State::UpToDate
        );

        uninstall(Target::Pi, Some(&dir)).unwrap();
        assert_eq!(
            status(Target::Pi, Some(&dir)).unwrap().state,
            State::NotInstalled
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn migrates_legacy_managed_file() {
        let dir = temp_dir("migrate");
        let legacy = dir.join("agent-beacon.js");
        fs::write(&legacy, render(Target::Pi)).unwrap();

        let current = status(Target::Pi, Some(&dir)).unwrap();
        assert_eq!(current.state, State::UpgradeAvailable);
        assert_eq!(current.installed_version.as_deref(), Some(VERSION));

        let (outcome, path) = install(Target::Pi, Some(&dir), false).unwrap();
        assert_eq!(outcome, Outcome::Upgraded);
        assert_eq!(path, dir.join("agent-beacon-hook-pi.js"));
        assert!(path.exists());
        assert!(!legacy.exists());
        assert_eq!(
            status(Target::Pi, Some(&dir)).unwrap().state,
            State::UpToDate
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_to_overwrite_unmanaged_file() {
        let dir = temp_dir("unmanaged");
        let path = dir.join(Target::OpenCode.file_name());
        fs::write(&path, "export const Custom = {};\n").unwrap();

        let error = install(Target::OpenCode, Some(&dir), false).unwrap_err();
        assert!(error.contains("--force"));
        assert_eq!(
            status(Target::OpenCode, Some(&dir)).unwrap().state,
            State::Unmanaged
        );

        let (outcome, _) = install(Target::OpenCode, Some(&dir), true).unwrap();
        assert_eq!(outcome, Outcome::Installed);
        assert_eq!(
            status(Target::OpenCode, Some(&dir)).unwrap().state,
            State::UpToDate
        );
        let _ = fs::remove_dir_all(dir);
    }
}
