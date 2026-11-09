//! Hermes Agent 集成：在 `~/.hermes/plugins/agent-beacon/` 写一个插件目录。
//!
//! Hermes Agent 的插件是“目录 + `plugin.yaml` 清单 + 一个 python 模块”，
//! 模块导出 `register(ctx)`，用 `ctx.register_hook(<event>, handler)` 订阅生命周期事件。
//! 这里写入的插件只做一件事：在事件回调里调用
//! `agent-beacon hook --agent hermes --state <state> --event <event>`。
//!
//! 与其他集成一样，文件头带 `# @agent-beacon-integration:` / `# @agent-beacon-version:`
//! 标记，卸载时只删带标记的文件，用户自己的插件不会被碰。

use std::fs;
use std::path::{Path, PathBuf};

use crate::integrations::{write_atomic, Outcome, State, Status, Target};
use crate::VERSION;

const PLUGIN_DIR_NAME: &str = "agent-beacon";
const MANIFEST: &str = "plugin.yaml";
const MODULE: &str = "agent_beacon.py";

const MODULE_SOURCE: &str = r#"# @agent-beacon-integration: hermes
# @agent-beacon-version: __VERSION__
# Managed by agent-beacon. Run `agent-beacon integrate upgrade` to update.
"""AgentBeacon integration for Hermes Agent: report lifecycle states."""

import subprocess

BINARY = __BINARY__
MARKER = "agent-beacon-hook:hermes:__VERSION__"

# 事件名 -> 上报状态。Hermes 的 hook 名与 AgentBeacon 的状态是两套词汇表。
EVENT_STATES = {
    "on_session_start": "idle",
    "pre_llm_call": "working",
    "pre_tool_call": "working",
    "post_tool_call": "working",
    "pre_approval_request": "waiting",
    "post_approval_response": "working",
    "api_request_error": "error",
    "on_session_end": "idle",
    "subagent_start": "working",
    "subagent_stop": "working",
}


def _report(state, event):
    try:
        subprocess.run(
            [
                BINARY,
                "hook",
                "--agent",
                "hermes",
                "--state",
                state,
                "--event",
                event,
                "--marker",
                MARKER,
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            timeout=5,
        )
    except Exception:
        # 灯不亮也不能影响 Hermes 正常工作。
        pass


def register(ctx):
    for event, state in EVENT_STATES.items():
        def handler(_state=state, _event=event, **_kwargs):
            _report(_state, _event)

        ctx.register_hook(event, handler)
"#;

const MANIFEST_SOURCE: &str = r#"# @agent-beacon-integration: hermes
# @agent-beacon-version: __VERSION__
name: agent-beacon
version: '__VERSION__'
description: AgentBeacon status indicator hooks
python: __MODULE__
"#;

fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "cannot resolve user home directory".to_string())
}

/// Hermes 的 home：`AGENT_BEACON_HERMES_DIR` > `HERMES_HOME` > `%LOCALAPPDATA%\.hermes` > `~/.hermes`。
fn hermes_home() -> Result<PathBuf, String> {
    for key in ["AGENT_BEACON_HERMES_DIR", "HERMES_HOME"] {
        if let Some(dir) = std::env::var_os(key) {
            if !dir.is_empty() {
                return Ok(PathBuf::from(dir));
            }
        }
    }
    if cfg!(windows) {
        if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
            if !dir.is_empty() {
                return Ok(PathBuf::from(dir).join(".hermes"));
            }
        }
    }
    Ok(home_dir()?.join(".hermes"))
}

/// 插件目录：`<hermes home>/plugins/agent-beacon`。
pub fn plugin_dir(override_dir: Option<&Path>) -> Result<PathBuf, String> {
    match override_dir {
        Some(dir) => Ok(dir.to_path_buf()),
        None => Ok(hermes_home()?.join("plugins").join(PLUGIN_DIR_NAME)),
    }
}

/// Hermes 是否装过：只看它自己的 home，与插件是否安装无关。
pub fn available() -> bool {
    hermes_home().map(|dir| dir.exists()).unwrap_or(false)
}

fn binary() -> String {
    if let Some(bin) = std::env::var_os("AGENT_BEACON_BIN") {
        let text = bin.to_string_lossy().to_string();
        if !text.is_empty() {
            return text;
        }
    }
    std::env::current_exe()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| "agent-beacon".to_string())
}

fn python_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn render_module() -> String {
    MODULE_SOURCE
        .replace("__BINARY__", &python_string(&binary()))
        .replace("__VERSION__", VERSION)
}

fn render_manifest() -> String {
    MANIFEST_SOURCE
        .replace("__MODULE__", MODULE)
        .replace("__VERSION__", VERSION)
}

fn is_managed(content: &str) -> bool {
    content
        .lines()
        .take(3)
        .any(|line| line.starts_with("# @agent-beacon-integration:"))
}

fn managed_version(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.strip_prefix("# @agent-beacon-version:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

pub fn status(override_dir: Option<&Path>) -> Result<Status, String> {
    let dir = plugin_dir(override_dir)?;
    let path = dir.join(MODULE);
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
            let manifest_managed = fs::read_to_string(dir.join(MANIFEST))
                .map(|content| is_managed(&content))
                .unwrap_or(false);
            if manifest_managed {
                (None, State::UpgradeAvailable)
            } else {
                (None, State::NotInstalled)
            }
        }
        Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
    };
    Ok(Status {
        target: Target::Hermes,
        dir,
        path,
        installed_version,
        embedded_version: VERSION,
        state,
    })
}

pub fn install(override_dir: Option<&Path>) -> Result<(Outcome, PathBuf), String> {
    write(override_dir, false)
}

pub fn upgrade(override_dir: Option<&Path>) -> Result<(Outcome, PathBuf), String> {
    write(override_dir, true)
}

fn write(
    override_dir: Option<&Path>,
    require_installed: bool,
) -> Result<(Outcome, PathBuf), String> {
    let dir = plugin_dir(override_dir)?;
    let path = dir.join(MODULE);
    let before = fs::read_to_string(&path)
        .ok()
        .filter(|content| is_managed(content))
        .and_then(|content| managed_version(&content));
    if require_installed && before.is_none() {
        return Err(format!(
            "{} is not installed; run integrate install hermes first",
            path.display()
        ));
    }
    write_atomic(&path, &render_module())?;
    write_atomic(&dir.join(MANIFEST), &render_manifest())?;
    let outcome = match before {
        None => Outcome::Installed,
        Some(version) if version == VERSION => Outcome::UpToDate,
        Some(_) => Outcome::Upgraded,
    };
    Ok((outcome, path))
}

pub fn uninstall(override_dir: Option<&Path>) -> Result<PathBuf, String> {
    let dir = plugin_dir(override_dir)?;
    let path = dir.join(MODULE);
    let content = fs::read_to_string(&path).map_err(|error| {
        format!(
            "{} is not installed; run integrate install hermes first ({error})",
            path.display()
        )
    })?;
    if !is_managed(&content) {
        return Err(format!(
            "{} is not managed by agent-beacon; refusing to delete",
            path.display()
        ));
    }
    fs::remove_file(&path)
        .map_err(|error| format!("failed to delete {}: {error}", path.display()))?;

    let manifest = dir.join(MANIFEST);
    if fs::read_to_string(&manifest)
        .map(|text| is_managed(&text))
        .unwrap_or(false)
    {
        let _ = fs::remove_file(&manifest);
    }
    // 目录空了我们建的就顺手收掉；用户还放了别的文件就保留。
    let _ = fs::remove_dir(&dir);
    Ok(path)
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
        let dir = std::env::temp_dir().join(format!("agent-beacon-hermes-{tag}-{unique}"));
        dir
    }

    #[test]
    fn install_writes_managed_plugin_and_is_idempotent() {
        let dir = temp_dir("install");
        let (outcome, path) = install(Some(&dir)).unwrap();
        assert_eq!(outcome, Outcome::Installed);
        assert_eq!(path, dir.join(MODULE));

        let module = fs::read_to_string(&path).unwrap();
        assert!(module.contains(&format!("# @agent-beacon-version: {VERSION}")));
        assert!(module.contains("ctx.register_hook"));
        assert!(module.contains("--agent"));
        assert!(module.contains("hermes"));
        assert!(!module.contains("__VERSION__"));

        let manifest = fs::read_to_string(dir.join(MANIFEST)).unwrap();
        assert!(manifest.contains("python: agent_beacon.py"));
        assert!(manifest.contains(&format!("version: '{VERSION}'")));

        assert_eq!(status(Some(&dir)).unwrap().state, State::UpToDate);
        let (outcome, _) = install(Some(&dir)).unwrap();
        assert_eq!(outcome, Outcome::UpToDate);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn upgrade_refreshes_stale_version_and_uninstall_removes_only_ours() {
        let dir = temp_dir("upgrade");
        install(Some(&dir)).unwrap();
        let path = dir.join(MODULE);
        let stale = fs::read_to_string(&path).unwrap().replace(
            &format!("agent-beacon-version: {VERSION}"),
            "agent-beacon-version: 0.0.1",
        );
        fs::write(&path, stale).unwrap();
        assert_eq!(status(Some(&dir)).unwrap().state, State::UpgradeAvailable);

        let (outcome, _) = upgrade(Some(&dir)).unwrap();
        assert_eq!(outcome, Outcome::Upgraded);
        assert_eq!(status(Some(&dir)).unwrap().state, State::UpToDate);

        let extra = dir.join("user-plugin.py");
        fs::write(&extra, "# mine\n").unwrap();
        uninstall(Some(&dir)).unwrap();
        assert!(extra.exists());
        assert_eq!(status(Some(&dir)).unwrap().state, State::NotInstalled);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_to_delete_unmanaged_module() {
        let dir = temp_dir("unmanaged");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MODULE), "print('mine')\n").unwrap();
        assert_eq!(status(Some(&dir)).unwrap().state, State::Unmanaged);
        assert!(uninstall(Some(&dir)).unwrap_err().contains("refusing"));
        let _ = fs::remove_dir_all(dir);
    }
}
