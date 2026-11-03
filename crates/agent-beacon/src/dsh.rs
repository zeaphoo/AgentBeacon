//! DeepSeek Harness（DSH）集成：写入一个 Cordis 插件，并在 DSH home 的
//! `cordis.patch.yml` 中插入加载项。
//!
//! DSH 通过 patch 行加载插件；插件文件使用绝对 `file://` URL，因此不需要
//! 往任何 profile 的 package.json 里加依赖。

use std::fs;
use std::path::{Path, PathBuf};

use crate::integrations::{
    is_managed, managed_version, write_atomic, Outcome, State, Status, Target,
};
use crate::VERSION;

const SOURCE: &str = include_str!("../assets/integrations/dsh.js");
const BEGIN: &str = "# >>> agent-beacon-hook-dsh";
const END: &str = "# <<< agent-beacon-hook-dsh";
const PLUGIN_FILE: &str = "agent-beacon-hook-dsh.js";

#[derive(Debug, Clone)]
pub struct DshPaths {
    pub home: PathBuf,
    pub plugin: PathBuf,
    pub patch: PathBuf,
}

pub fn available() -> bool {
    paths().map(|paths| paths.home.exists()).unwrap_or(false)
}

pub fn paths() -> Result<DshPaths, String> {
    let home = if let Some(dir) = std::env::var_os("AGENT_BEACON_DSH_HOME") {
        PathBuf::from(dir)
    } else if let Some(dir) = std::env::var_os("DSH_HOME") {
        PathBuf::from(dir)
    } else {
        dirs::home_dir()
            .ok_or_else(|| "无法解析用户主目录".to_string())?
            .join(".dsh")
    };
    let plugin_dir = if let Some(dir) = std::env::var_os("AGENT_BEACON_DSH_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::data_dir()
            .ok_or_else(|| "无法解析用户数据目录".to_string())?
            .join("agent-beacon")
            .join("integrations")
    };
    Ok(DshPaths {
        patch: home.join("cordis.patch.yml"),
        plugin: plugin_dir.join(PLUGIN_FILE),
        home,
    })
}

fn render() -> String {
    SOURCE.replace("__VERSION__", VERSION)
}

pub fn status() -> Result<Status, String> {
    status_at(&paths()?)
}

fn status_at(paths: &DshPaths) -> Result<Status, String> {
    let (installed_version, state) = match fs::read_to_string(&paths.plugin) {
        Ok(content) if !is_managed(&content) => (managed_version(&content), State::Unmanaged),
        Ok(content) => {
            let version = managed_version(&content);
            let patch = fs::read_to_string(&paths.patch).unwrap_or_default();
            let state = if version.as_deref() == Some(VERSION) && has_patch(&patch) {
                State::UpToDate
            } else {
                State::UpgradeAvailable
            };
            (version, state)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let patch = fs::read_to_string(&paths.patch).unwrap_or_default();
            if has_patch(&patch) {
                // patch 还在但插件文件丢失：install 可以修复。
                (None, State::UpgradeAvailable)
            } else {
                (None, State::NotInstalled)
            }
        }
        Err(error) => {
            return Err(format!("读取 {} 失败：{error}", paths.plugin.display()));
        }
    };
    Ok(Status {
        target: Target::Dsh,
        dir: paths.home.clone(),
        path: paths.plugin.clone(),
        installed_version,
        embedded_version: VERSION,
        state,
    })
}

pub fn install() -> Result<(Outcome, PathBuf), String> {
    install_at(&paths()?)
}

fn install_at(paths: &DshPaths) -> Result<(Outcome, PathBuf), String> {
    if !paths.home.exists() {
        return Err(format!(
            "未找到 DSH home：{}。请先安装并运行 DeepSeek Harness。",
            paths.home.display()
        ));
    }
    let previous = fs::read_to_string(&paths.plugin).ok();
    let previous_managed = previous.as_deref().is_some_and(is_managed);
    let previous_version = previous.as_deref().and_then(managed_version);
    let patch = fs::read_to_string(&paths.patch).unwrap_or_default();
    let had_patch = has_patch(&patch);

    write_atomic(&paths.plugin, &render())?;
    upsert_patch(&paths.patch, &patch_block(&paths.plugin)?)?;

    let outcome = if previous_version.as_deref() == Some(VERSION) && had_patch {
        Outcome::UpToDate
    } else if previous_managed || had_patch {
        Outcome::Upgraded
    } else {
        Outcome::Installed
    };
    Ok((outcome, paths.plugin.clone()))
}

pub fn upgrade() -> Result<(Outcome, PathBuf), String> {
    upgrade_at(&paths()?)
}

fn upgrade_at(paths: &DshPaths) -> Result<(Outcome, PathBuf), String> {
    if !paths.plugin.exists() {
        return Err(format!(
            "{} 尚未安装，请先执行 integrate install dsh",
            paths.plugin.display()
        ));
    }
    let existing = fs::read_to_string(&paths.plugin)
        .map_err(|error| format!("读取 {} 失败：{error}", paths.plugin.display()))?;
    if !is_managed(&existing) {
        return Err(format!(
            "{} 不是 agent-beacon 管理的文件，拒绝覆盖",
            paths.plugin.display()
        ));
    }
    let patch = fs::read_to_string(&paths.patch).unwrap_or_default();
    let up_to_date = managed_version(&existing).as_deref() == Some(VERSION) && has_patch(&patch);
    if up_to_date {
        return Ok((Outcome::UpToDate, paths.plugin.clone()));
    }
    write_atomic(&paths.plugin, &render())?;
    upsert_patch(&paths.patch, &patch_block(&paths.plugin)?)?;
    Ok((Outcome::Upgraded, paths.plugin.clone()))
}

pub fn uninstall() -> Result<PathBuf, String> {
    uninstall_at(&paths()?)
}

fn uninstall_at(paths: &DshPaths) -> Result<PathBuf, String> {
    let patch = fs::read_to_string(&paths.patch).unwrap_or_default();
    let had_patch = has_patch(&patch);

    let plugin_managed = match fs::read_to_string(&paths.plugin) {
        Ok(content) if !is_managed(&content) => {
            return Err(format!(
                "{} 不是 agent-beacon 管理的文件，拒绝删除",
                paths.plugin.display()
            ));
        }
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!("读取 {} 失败：{error}", paths.plugin.display()));
        }
    };

    if !plugin_managed && !had_patch {
        return Err("DSH 集成尚未安装".to_string());
    }

    if had_patch {
        let cleaned = remove_patch_block(&patch);
        write_atomic(&paths.patch, &cleaned)?;
    }
    if paths.plugin.exists() {
        fs::remove_file(&paths.plugin)
            .map_err(|error| format!("删除 {} 失败：{error}", paths.plugin.display()))?;
    }
    Ok(paths.plugin.clone())
}

pub fn upgrade_installed() -> Option<Result<Outcome, String>> {
    let paths = paths().ok()?;
    if !paths.home.exists() {
        return None;
    }
    let patch = fs::read_to_string(&paths.patch).unwrap_or_default();
    let has_managed_file = fs::read_to_string(&paths.plugin)
        .map(|content| is_managed(&content))
        .unwrap_or(false);
    if !has_patch(&patch) && !has_managed_file {
        return None;
    }
    Some(upgrade_at(&paths).map(|(outcome, _)| outcome))
}

fn has_patch(content: &str) -> bool {
    content.contains(BEGIN) && content.contains(END)
}

fn patch_block(plugin: &Path) -> Result<String, String> {
    let url = url::Url::from_file_path(plugin)
        .map_err(|_| format!("无法把 {} 转为 file URL", plugin.display()))?;
    // YAML 单引号字符串：内部单引号需要双写。
    let value = url.to_string().replace('\'', "''");
    Ok(format!(
        "{BEGIN} (managed by agent-beacon)\n- insert:\n    - id: agent-beacon-hook-dsh\n      name: '{value}'\n{END}\n"
    ))
}

fn upsert_patch(path: &Path, block: &str) -> Result<(), String> {
    let content = fs::read_to_string(path).unwrap_or_default();
    let next = if let Some(start) = content.find(BEGIN) {
        if let Some(relative_end) = content[start..].find(END) {
            let end = start + relative_end + END.len();
            let end = if content[end..].starts_with('\n') {
                end + 1
            } else {
                end
            };
            format!("{}{}{}", &content[..start], block, &content[end..])
        } else {
            let mut next = content;
            if !next.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(block);
            next
        }
    } else {
        let mut next = content;
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(block);
        next
    };
    write_atomic(path, &next)
}

fn remove_patch_block(content: &str) -> String {
    let Some(start) = content.find(BEGIN) else {
        return content.to_string();
    };
    let Some(relative_end) = content[start..].find(END) else {
        return content.to_string();
    };
    let end = start + relative_end + END.len();
    let end = if content[end..].starts_with('\n') {
        end + 1
    } else {
        end
    };
    let mut next = format!("{}{}", &content[..start], &content[end..]);
    while next.contains("\n\n\n") {
        next = next.replace("\n\n\n", "\n\n");
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn paths(tag: &str) -> DshPaths {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("agent-beacon-dsh-{tag}-{unique}"));
        fs::create_dir_all(root.join("home")).unwrap();
        fs::create_dir_all(root.join("plugin")).unwrap();
        DshPaths {
            home: root.join("home"),
            plugin: root.join("plugin").join(PLUGIN_FILE),
            patch: root.join("home").join("cordis.patch.yml"),
        }
    }

    #[test]
    fn install_upserts_patch_and_upgrades() {
        let paths = paths("roundtrip");
        fs::write(&paths.patch, "- id: user-row\n  name: '@example/user'\n").unwrap();

        let (outcome, plugin) = install_at(&paths).unwrap();
        assert_eq!(outcome, Outcome::Installed);
        assert!(plugin.exists());
        let patch = fs::read_to_string(&paths.patch).unwrap();
        assert!(patch.contains("agent-beacon-hook-dsh"));
        assert!(patch.contains("user-row"));
        assert!(patch.contains("file://"));
        assert_eq!(status_at(&paths).unwrap().state, State::UpToDate);
        let (again, _) = install_at(&paths).unwrap();
        assert_eq!(again, Outcome::UpToDate);
        let patch = fs::read_to_string(&paths.patch).unwrap();
        assert_eq!(patch.matches(BEGIN).count(), 1);

        let content = fs::read_to_string(&paths.plugin).unwrap();
        let old = content.replacen(
            &format!("@agent-beacon-version: {VERSION}"),
            "@agent-beacon-version: 0.0.0",
            1,
        );
        fs::write(&paths.plugin, old).unwrap();
        assert_eq!(status_at(&paths).unwrap().state, State::UpgradeAvailable);

        let (outcome, _) = upgrade_at(&paths).unwrap();
        assert_eq!(outcome, Outcome::Upgraded);
        assert_eq!(status_at(&paths).unwrap().state, State::UpToDate);

        uninstall_at(&paths).unwrap();
        assert!(!paths.plugin.exists());
        let patch = fs::read_to_string(&paths.patch).unwrap();
        assert!(!patch.contains("agent-beacon-hook-dsh"));
        assert!(patch.contains("user-row"));
        assert_eq!(status_at(&paths).unwrap().state, State::NotInstalled);
        let _ = fs::remove_dir_all(paths.home.parent().unwrap());
    }

    #[test]
    fn install_requires_dsh_home() {
        let paths = paths("missing-home");
        fs::remove_dir_all(&paths.home).unwrap();
        assert!(install_at(&paths).is_err());
    }
}
