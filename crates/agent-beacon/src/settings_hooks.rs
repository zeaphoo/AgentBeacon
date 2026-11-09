//! 通过 agent 自己的 hooks 配置接入的 agent。
//!
//! 目前两种文件形态：
//!
//! * `settings.json` 家族（Claude Code、Qoder CLI、CodeBuddy、WorkBuddy、
//!   GitHub Copilot，以及 Gemini CLI 家族 Gemini / Qwen Code / iFlow）——配置目录里的
//!   `settings.json` 有一个 `hooks` 对象，键是生命周期事件名，值是一组
//!   `{ matcher, hooks: [{ type: "command", command }] }`。
//! * `config.json` 家族（ZCode）——`~/.zcode/cli/config.json` 的 `hooks` 同样是
//!   “事件名 -> 数组”，但数组元素本身就是 `{ type: "process", command, enabled, timeoutMs }`，
//!   命令按进程参数执行（不走 shell），所以标记用 `--marker` 参数而不是行尾注释。
//!
//! 安装时只新增/替换带 `agent-beacon-hook:<target>:<version>` 标记的命令，
//! 用户自己配置的其它 hook、以及同一个事件下的其它命令都会原样保留。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::integrations::{write_atomic, Outcome, State, Status, Target};
use crate::VERSION;

/// 一条 hook 定义：某个生命周期事件触发时上报某个状态。
#[derive(Debug, Clone, Copy)]
pub struct HookSpec {
    pub event: &'static str,
    pub matcher: Option<&'static str>,
    pub state: &'static str,
    pub message: Option<&'static str>,
    pub ttl: Option<u64>,
}

const fn spec(event: &'static str, state: &'static str) -> HookSpec {
    HookSpec {
        event,
        matcher: None,
        state,
        message: None,
        ttl: None,
    }
}

const fn matched(event: &'static str, state: &'static str) -> HookSpec {
    HookSpec {
        event,
        matcher: Some("*"),
        state,
        message: None,
        ttl: None,
    }
}

const fn waiting(event: &'static str) -> HookSpec {
    HookSpec {
        event,
        matcher: Some("*"),
        state: "waiting",
        message: Some("等待确认"),
        ttl: None,
    }
}

const fn ending(event: &'static str) -> HookSpec {
    HookSpec {
        event,
        matcher: None,
        state: "idle",
        message: None,
        ttl: Some(30),
    }
}

const fn failing(event: &'static str, matcher: Option<&'static str>) -> HookSpec {
    HookSpec {
        event,
        matcher,
        state: "error",
        message: None,
        ttl: Some(30),
    }
}

const fn confirm(event: &'static str) -> HookSpec {
    HookSpec {
        event,
        matcher: None,
        state: "waiting",
        message: Some("等待确认"),
        ttl: None,
    }
}

/// Claude Code：`~/.claude/settings.json`
const CLAUDE_SPECS: &[HookSpec] = &[
    spec("SessionStart", "idle"),
    spec("UserPromptSubmit", "working"),
    matched("PreToolUse", "working"),
    matched("PostToolUse", "working"),
    waiting("Notification"),
    spec("Stop", "idle"),
    spec("SubagentStop", "idle"),
    ending("SessionEnd"),
];

/// Qoder CLI：`~/.qoder/settings.json`，事件名与 Claude Code 对齐。
const QODER_SPECS: &[HookSpec] = &[
    spec("SessionStart", "idle"),
    spec("UserPromptSubmit", "working"),
    matched("PreToolUse", "working"),
    matched("PostToolUse", "working"),
    waiting("Notification"),
    spec("Stop", "idle"),
    ending("SessionEnd"),
];

/// Gemini CLI 家族：`~/.gemini/settings.json`、`~/.qwen/settings.json`、`~/.iflow/settings.json`
const GEMINI_SPECS: &[HookSpec] = &[
    spec("SessionStart", "idle"),
    spec("BeforeAgent", "working"),
    matched("BeforeTool", "working"),
    matched("AfterTool", "working"),
    spec("AfterAgent", "idle"),
    waiting("Notification"),
    ending("SessionEnd"),
];

/// GitHub Copilot CLI：`~/.copilot/settings.json`，与 Claude 同结构但事件名是 camelCase。
const COPILOT_SPECS: &[HookSpec] = &[
    spec("sessionStart", "idle"),
    spec("userPromptSubmitted", "working"),
    matched("preToolUse", "working"),
    matched("postToolUse", "working"),
    spec("agentStop", "idle"),
    failing("errorOccurred", None),
    ending("sessionEnd"),
];

/// ZCode：`~/.zcode/cli/config.json`，Claude 式事件名 + process 型命令。
const ZCODE_SPECS: &[HookSpec] = &[
    spec("SessionStart", "idle"),
    spec("UserPromptSubmit", "working"),
    matched("PreToolUse", "working"),
    confirm("PermissionRequest"),
    matched("PostToolUse", "working"),
    failing("PostToolUseFailure", Some("*")),
    spec("Stop", "idle"),
];

pub fn specs(target: Target) -> &'static [HookSpec] {
    match target {
        Target::ClaudeCode => CLAUDE_SPECS,
        // CodeBuddy 用 Claude 式 hooks；WorkBuddy 桌面版复用 CodeBuddy 的 hook 运行时。
        Target::Qoder | Target::CodeBuddy | Target::WorkBuddy => QODER_SPECS,
        Target::Gemini | Target::Qwen | Target::Iflow => GEMINI_SPECS,
        Target::GitHubCopilot => COPILOT_SPECS,
        Target::ZCode => ZCODE_SPECS,
        _ => &[],
    }
}

/// hooks 配置的文件形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// `{ event: [ { matcher?, hooks: [ { type, command } ] } ] }`
    Nested,
    /// `{ event: [ { type: "process", command, enabled, timeoutMs } ] }`
    Process,
}

fn shape(target: Target) -> Shape {
    match target {
        Target::ZCode => Shape::Process,
        _ => Shape::Nested,
    }
}

fn config_file(target: Target) -> &'static str {
    match target {
        Target::ZCode => "config.json",
        _ => "settings.json",
    }
}

/// 标记写在命令末尾的 shell 注释里，既能定位也能读出安装版本。
fn marker(target: Target) -> String {
    format!("agent-beacon-hook:{}:{VERSION}", target.id())
}

fn marker_prefix(target: Target) -> String {
    format!("agent-beacon-hook:{}:", target.id())
}

fn settings_path(target: Target, override_dir: Option<&Path>) -> Result<PathBuf, String> {
    let dir = match override_dir {
        Some(dir) => dir.to_path_buf(),
        None => target.default_dir()?,
    };
    Ok(dir.join(config_file(target)))
}

fn settings_dir(target: Target, override_dir: Option<&Path>) -> Result<PathBuf, String> {
    let path = settings_path(target, override_dir)?;
    Ok(path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".")))
}

/// agent 是否装过（配置目录存在）。
pub fn available(target: Target) -> bool {
    settings_dir(target, None)
        .map(|dir| dir.exists())
        .unwrap_or(false)
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

fn shell_quote(value: &str) -> String {
    if cfg!(windows) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn hook_command(target: Target, spec: &HookSpec) -> String {
    let mut command = format!(
        "{} hook --agent {} --state {} --event {}",
        shell_quote(&binary()),
        target.id(),
        spec.state,
        spec.event
    );
    if let Some(message) = spec.message {
        command.push_str(&format!(" --message {}", shell_quote(message)));
    }
    if let Some(ttl) = spec.ttl {
        command.push_str(&format!(" --ttl {ttl}"));
    }
    if shape(target) == Shape::Process {
        // process 型 hook 按参数数组执行，行尾 `#` 注释会被当成普通参数。
        command.push_str(&format!(" --marker {}", marker(target)));
        return command;
    }
    command.push_str(" # ");
    command.push_str(&marker(target));
    command
}

fn read_settings(path: &Path) -> Result<Map<String, Value>, String> {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => serde_json::from_str::<Value>(&text)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?
            .as_object()
            .cloned()
            .ok_or_else(|| format!("{} must contain a JSON object", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(format!("failed to read {}: {error}", path.display())),
    }
}

fn write_settings(path: &Path, settings: &Map<String, Value>) -> Result<(), String> {
    let had_trailing_newline = fs::read_to_string(path)
        .map(|text| text.ends_with('\n'))
        .unwrap_or(true);
    let mut text = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("failed to serialize {}: {error}", path.display()))?;
    if had_trailing_newline {
        text.push('\n');
    }
    write_atomic(path, &text)
}

/// 在 `hooks` 对象里安装（或刷新）本集成的命令，返回是否已经是当前版本。
fn apply_specs(settings: &mut Map<String, Value>, target: Target) -> Result<bool, String> {
    if shape(target) == Shape::Process {
        return apply_process_specs(settings, target);
    }
    let prefix = marker_prefix(target);
    let hooks_value = settings
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks_value
        .as_object_mut()
        .ok_or_else(|| "settings key `hooks` must be an object".to_string())?;

    let mut up_to_date = true;
    for spec in specs(target) {
        let entry = hooks
            .entry(spec.event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let groups = entry
            .as_array_mut()
            .ok_or_else(|| format!("hook event {} must be an array", spec.event))?;
        let command = hook_command(target, spec);

        let mut replaced = false;
        for group in groups.iter_mut() {
            let Some(group_object) = group.as_object_mut() else {
                continue;
            };
            let Some(commands) = group_object.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };
            for hook in commands.iter_mut() {
                let Some(object) = hook.as_object_mut() else {
                    continue;
                };
                let current = object.get("command").and_then(Value::as_str);
                let ours = current.is_some_and(|text| text.contains(&prefix));
                if !ours {
                    continue;
                }
                if !current.is_some_and(|text| text.contains(&marker(target))) {
                    up_to_date = false;
                }
                object.insert("type".to_string(), json!("command"));
                object.insert("command".to_string(), json!(command));
                if let Some(matcher) = spec.matcher {
                    group_object.insert("matcher".to_string(), json!(matcher));
                }
                replaced = true;
                break;
            }
            if replaced {
                break;
            }
        }
        if replaced {
            continue;
        }

        up_to_date = false;
        let mut hook = Map::new();
        hook.insert("type".to_string(), json!("command"));
        hook.insert("command".to_string(), json!(command));
        let mut group = Map::new();
        if let Some(matcher) = spec.matcher {
            group.insert("matcher".to_string(), json!(matcher));
        }
        group.insert("hooks".to_string(), Value::Array(vec![Value::Object(hook)]));
        groups.push(Value::Object(group));
    }
    Ok(up_to_date)
}

/// process 型 hooks（ZCode）：数组元素本身就是一条命令，没有 matcher 分组。
fn apply_process_specs(settings: &mut Map<String, Value>, target: Target) -> Result<bool, String> {
    let prefix = marker_prefix(target);
    let hooks_value = settings
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks_value
        .as_object_mut()
        .ok_or_else(|| "config key `hooks` must be an object".to_string())?;

    let mut up_to_date = true;
    for spec in specs(target) {
        let entry = hooks
            .entry(spec.event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let entries = entry
            .as_array_mut()
            .ok_or_else(|| format!("hook event {} must be an array", spec.event))?;
        let command = hook_command(target, spec);

        let mut replaced = false;
        for item in entries.iter_mut() {
            let Some(object) = item.as_object_mut() else {
                continue;
            };
            let current = object.get("command").and_then(Value::as_str);
            if !current.is_some_and(|text| text.contains(&prefix)) {
                continue;
            }
            if !current.is_some_and(|text| text.contains(&marker(target))) {
                up_to_date = false;
            }
            object.insert("type".to_string(), json!("process"));
            object.insert("command".to_string(), json!(command));
            object.insert("enabled".to_string(), json!(true));
            replaced = true;
            break;
        }
        if replaced {
            continue;
        }

        up_to_date = false;
        entries.push(json!({
            "type": "process",
            "command": command,
            "enabled": true,
            "timeoutMs": 10000,
        }));
    }
    Ok(up_to_date)
}

fn remove_process_specs(settings: &mut Map<String, Value>, target: Target) -> bool {
    let prefix = marker_prefix(target);
    let Some(Value::Object(hooks)) = settings.get_mut("hooks") else {
        return false;
    };

    let mut changed = false;
    let events: Vec<String> = hooks.keys().cloned().collect();
    for event in events {
        let Some(Value::Array(entries)) = hooks.get_mut(&event) else {
            continue;
        };
        let before = entries.len();
        entries.retain(|item| {
            let text = item.get("command").and_then(Value::as_str).unwrap_or("");
            !text.contains(&prefix)
        });
        if entries.len() != before {
            changed = true;
        }
        if entries.is_empty() {
            hooks.remove(&event);
        }
    }
    if hooks.is_empty() {
        settings.remove("hooks");
    }
    changed
}

/// 删除本集成的命令；没有改动时返回 false。
fn remove_specs(settings: &mut Map<String, Value>, target: Target) -> bool {
    if shape(target) == Shape::Process {
        return remove_process_specs(settings, target);
    }
    let prefix = marker_prefix(target);
    let Some(hooks_value) = settings.get_mut("hooks") else {
        return false;
    };
    let Some(hooks) = hooks_value.as_object_mut() else {
        return false;
    };

    let mut changed = false;
    let events: Vec<String> = hooks.keys().cloned().collect();
    for event in events {
        let Some(groups_value) = hooks.get_mut(&event) else {
            continue;
        };
        let Some(groups) = groups_value.as_array_mut() else {
            continue;
        };
        groups.retain_mut(|group| {
            let Some(group_object) = group.as_object_mut() else {
                return true;
            };
            let Some(commands) = group_object.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let before = commands.len();
            commands.retain(|hook| {
                let text = hook.get("command").and_then(Value::as_str).unwrap_or("");
                !text.contains(&prefix)
            });
            if commands.len() != before {
                changed = true;
            }
            !commands.is_empty()
        });
        if groups.is_empty() {
            hooks.remove(&event);
        }
    }
    if hooks.is_empty() {
        settings.remove("hooks");
    }
    changed
}

fn version_in(text: &str, prefix: &str) -> Option<String> {
    let index = text.find(prefix)?;
    let rest = &text[index + prefix.len()..];
    let version: String = rest
        .chars()
        .take_while(|ch| !ch.is_whitespace() && *ch != '"' && *ch != '\'')
        .collect();
    (!version.is_empty()).then_some(version)
}

fn installed_version(settings: &Map<String, Value>, prefix: &str) -> Option<String> {
    fn walk(value: &Value, prefix: &str) -> Option<String> {
        match value {
            Value::String(text) => version_in(text, prefix),
            Value::Array(items) => items.iter().find_map(|item| walk(item, prefix)),
            Value::Object(map) => map.values().find_map(|item| walk(item, prefix)),
            _ => None,
        }
    }

    settings.values().find_map(|value| walk(value, prefix))
}

pub fn status(target: Target, override_dir: Option<&Path>) -> Result<Status, String> {
    let path = settings_path(target, override_dir)?;
    let dir = settings_dir(target, override_dir)?;
    let settings = read_settings(&path)?;
    let prefix = marker_prefix(target);
    let installed = installed_version(&settings, &prefix);
    let state = match installed.as_deref() {
        None => State::NotInstalled,
        Some(version) if version == VERSION => State::UpToDate,
        Some(_) => State::UpgradeAvailable,
    };
    Ok(Status {
        target,
        dir,
        path,
        installed_version: installed,
        embedded_version: VERSION,
        state,
    })
}

pub fn install(
    target: Target,
    override_dir: Option<&Path>,
    _force: bool,
) -> Result<(Outcome, PathBuf), String> {
    let dir = settings_dir(target, override_dir)?;
    if !dir.exists() {
        return Err(format!(
            "{} config directory not found: {}. Run {} once first.",
            target.label(),
            dir.display(),
            target.label()
        ));
    }
    let path = settings_path(target, override_dir)?;
    let mut settings = read_settings(&path)?;
    let before = installed_version(&settings, &marker_prefix(target));
    apply_specs(&mut settings, target)?;
    write_settings(&path, &settings)?;
    let outcome = match before {
        None => Outcome::Installed,
        Some(version) if version == VERSION => Outcome::UpToDate,
        Some(_) => Outcome::Upgraded,
    };
    Ok((outcome, path))
}

pub fn upgrade(target: Target, override_dir: Option<&Path>) -> Result<(Outcome, PathBuf), String> {
    let path = settings_path(target, override_dir)?;
    let mut settings = read_settings(&path)?;
    let before = installed_version(&settings, &marker_prefix(target));
    if before.is_none() {
        return Err(format!(
            "{} is not installed; run integrate install {} first",
            path.display(),
            target.id()
        ));
    }
    apply_specs(&mut settings, target)?;
    write_settings(&path, &settings)?;
    let outcome = if before.as_deref() == Some(VERSION) {
        Outcome::UpToDate
    } else {
        Outcome::Upgraded
    };
    Ok((outcome, path))
}

pub fn uninstall(target: Target, override_dir: Option<&Path>) -> Result<PathBuf, String> {
    let path = settings_path(target, override_dir)?;
    let mut settings = read_settings(&path)?;
    if !remove_specs(&mut settings, target) {
        return Err(format!("{} is not installed", path.display()));
    }
    write_settings(&path, &settings)?;
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
        let dir = std::env::temp_dir().join(format!("agent-beacon-hooks-{tag}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_preserves_user_hooks_and_is_idempotent() {
        let dir = temp_dir("merge");
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{
  "theme": "dark",
  "hooks": {
    "SessionStart": [
      { "hooks": [ { "type": "command", "command": "echo hello" } ] }
    ],
    "UserPromptSubmit": [
      { "hooks": [ { "type": "command", "command": "echo prompt" } ] }
    ]
  }
}"#,
        )
        .unwrap();

        let (outcome, _) = install(Target::ClaudeCode, Some(&dir), false).unwrap();
        assert_eq!(outcome, Outcome::Installed);
        assert_eq!(
            status(Target::ClaudeCode, Some(&dir)).unwrap().state,
            State::UpToDate
        );

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("echo hello"));
        assert!(text.contains("echo prompt"));
        assert!(text.contains("\"theme\": \"dark\""));
        assert!(text.contains("hook --agent claude-code --state waiting"));
        assert!(text.contains(&marker(Target::ClaudeCode)));

        // 重复安装：命令不会翻倍。
        let (outcome, _) = install(Target::ClaudeCode, Some(&dir), false).unwrap();
        assert_eq!(outcome, Outcome::UpToDate);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text.matches("hook --agent claude-code").count(),
            CLAUDE_SPECS.len()
        );
        assert_eq!(text.matches("echo hello").count(), 1);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn upgrade_rewrites_stale_version_and_uninstall_cleans_up() {
        let dir = temp_dir("upgrade");
        let path = dir.join("settings.json");
        install(Target::Qoder, Some(&dir), false).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let stale = text.replace(&marker(Target::Qoder), "agent-beacon-hook:qoder:0.0.1");
        fs::write(&path, stale).unwrap();
        assert_eq!(
            status(Target::Qoder, Some(&dir)).unwrap().state,
            State::UpgradeAvailable
        );

        let (outcome, _) = upgrade(Target::Qoder, Some(&dir)).unwrap();
        assert_eq!(outcome, Outcome::Upgraded);
        assert_eq!(
            status(Target::Qoder, Some(&dir)).unwrap().state,
            State::UpToDate
        );

        uninstall(Target::Qoder, Some(&dir)).unwrap();
        assert_eq!(
            status(Target::Qoder, Some(&dir)).unwrap().state,
            State::NotInstalled
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("agent-beacon-hook:qoder"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn codebuddy_and_workbuddy_use_claude_style_events() {
        for target in [Target::CodeBuddy, Target::WorkBuddy] {
            let dir = temp_dir("buddy");
            install(target, Some(&dir), false).unwrap();
            let text = fs::read_to_string(dir.join("settings.json")).unwrap();
            assert!(text.contains("SessionStart"));
            assert!(text.contains(&format!("hook --agent {}", target.id())));
            assert!(text.contains(&marker(target)));
            assert_eq!(status(target, Some(&dir)).unwrap().state, State::UpToDate);
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn github_copilot_uses_camel_case_events() {
        let dir = temp_dir("copilot");
        install(Target::GitHubCopilot, Some(&dir), false).unwrap();
        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        for event in [
            "sessionStart",
            "userPromptSubmitted",
            "preToolUse",
            "postToolUse",
            "agentStop",
            "errorOccurred",
            "sessionEnd",
        ] {
            assert!(text.contains(&format!("\"{event}\"")), "missing {event}");
        }
        // Claude 式事件名不能混进来，否则 Copilot 会静默忽略这些 hook。
        assert!(!text.contains("\"SessionStart\""));
        assert!(!text.contains("UserPromptSubmit\""));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn zcode_writes_process_entries_into_config_json() {
        let dir = temp_dir("zcode");
        let path = dir.join("config.json");
        fs::write(
            &path,
            r#"{
  "hooks": {
    "SessionStart": [
      { "type": "process", "command": "echo hi", "enabled": true, "timeoutMs": 5000 }
    ]
  },
  "theme": "dark"
}"#,
        )
        .unwrap();

        let (outcome, written) = install(Target::ZCode, Some(&dir), false).unwrap();
        assert_eq!(outcome, Outcome::Installed);
        assert_eq!(written, path);

        let settings: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let entries = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(entries.len(), 2, "user entry must be preserved");
        assert_eq!(entries[0]["command"], "echo hi");
        let ours = entries
            .iter()
            .find(|entry| {
                entry["command"]
                    .as_str()
                    .is_some_and(|text| text.contains("hook --agent zcode"))
            })
            .expect("our process hook must be installed");
        assert_eq!(ours["type"], "process");
        assert_eq!(ours["enabled"], true);
        assert!(ours["command"]
            .as_str()
            .unwrap()
            .contains("--marker agent-beacon-hook:zcode:"));
        // process 型命令按参数数组执行，不能出现 shell 注释。
        assert!(!ours["command"].as_str().unwrap().contains(" # "));
        // matcher 只属于嵌套式 hooks，不能混进 process 条目所在的事件里。
        assert!(settings["hooks"]["SessionStart"][0]
            .get("matcher")
            .is_none());
        assert_eq!(settings["theme"], "dark");
        assert_eq!(
            status(Target::ZCode, Some(&dir)).unwrap().state,
            State::UpToDate
        );

        let (outcome, _) = install(Target::ZCode, Some(&dir), false).unwrap();
        assert_eq!(outcome, Outcome::UpToDate);
        let settings: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            settings["hooks"]["SessionStart"].as_array().unwrap().len(),
            2
        );

        uninstall(Target::ZCode, Some(&dir)).unwrap();
        let settings: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let entries = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!settings.to_string().contains("hook --agent zcode"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn gemini_family_uses_its_own_events() {
        let dir = temp_dir("gemini");
        install(Target::Gemini, Some(&dir), false).unwrap();
        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("BeforeAgent"));
        assert!(text.contains("AfterTool"));
        assert!(text.contains("hook --agent gemini"));
        assert!(!text.contains("UserPromptSubmit"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn uninstall_restores_the_original_content() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("settings.json");
        let original = r#"{
  "ide": {
    "hasSeenNudge": true
  },
  "hooks": {
    "Stop": [
      { "hooks": [ { "type": "command", "command": "notify-send done" } ] }
    ]
  }
}"#;
        fs::write(&path, original).unwrap();
        install(Target::Gemini, Some(&dir), false).unwrap();
        uninstall(Target::Gemini, Some(&dir)).unwrap();
        // 写入会统一成 2 空格缩进，但键顺序与内容保持一致。
        let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let expected: Value = serde_json::from_str(original).unwrap();
        assert_eq!(restored, expected);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            !text.ends_with('\n'),
            "trailing newline state must be preserved"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn install_requires_existing_config_dir() {
        let dir = temp_dir("missing").join("nested");
        let error = install(Target::ClaudeCode, Some(&dir), false).unwrap_err();
        assert!(error.contains("not found"));
    }

    #[test]
    fn uninstall_without_install_errors() {
        let dir = temp_dir("noop");
        assert!(uninstall(Target::Iflow, Some(&dir)).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
