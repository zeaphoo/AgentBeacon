//! Codex CLI 集成：在 `~/.codex/config.toml` 里写入 `notify` 命令。
//!
//! Codex 目前只有这一个人工可控的钩子：`notify` 会在 agent 回合结束
//! （`agent-turn-complete`）和等待审批（`approval-requested`）时被调用，
//! 事件载荷作为最后一个参数传入。它没有“开始工作”事件，所以灯会在
//! 下一次上报前保持上一个状态，靠 `agent-beacon hook --state auto` 映射。
//!
//! 写入用注释标记包围，便于识别、升级和卸载；用户自己的 `notify`
//! 在没有 `--force` 时不会被覆盖。

use std::fs;
use std::path::{Path, PathBuf};

use crate::integrations::{write_atomic, Outcome, State, Status, Target};
use crate::VERSION;

const BEGIN: &str = "# >>> agent-beacon-hook:codex";
const END: &str = "# <<< agent-beacon-hook:codex";
const KEY: &str = "notify";

pub fn config_path(override_dir: Option<&Path>) -> Result<PathBuf, String> {
    let dir = match override_dir {
        Some(dir) => dir.to_path_buf(),
        None => Target::Codex.default_dir()?,
    };
    Ok(dir.join("config.toml"))
}

pub fn available() -> bool {
    config_path(None)
        .map(|path| path.parent().map(Path::exists).unwrap_or(false))
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

fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn notify_value() -> String {
    let args = [
        binary(),
        "hook".to_string(),
        "--agent".to_string(),
        "codex".to_string(),
        "--state".to_string(),
        "auto".to_string(),
    ];
    let items: Vec<String> = args.iter().map(|arg| toml_string(arg)).collect();
    format!("[{}]", items.join(", "))
}

fn block() -> String {
    format!(
        "{BEGIN} {VERSION} (managed by agent-beacon)\n{KEY} = {}\n{END}\n",
        notify_value()
    )
}

fn has_block(content: &str) -> bool {
    content.contains(BEGIN) && content.contains(END)
}

fn block_version(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix(BEGIN))
        .map(|rest| rest.trim().to_string())
        .map(|rest| {
            rest.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .filter(|value| !value.is_empty())
}

/// 找到根级 `notify` 所在行的起点和值区间终点。
fn find_root_value(content: &str, key: &str) -> Option<(usize, usize)> {
    let mut offset = 0usize;
    let mut in_table = false;
    while offset < content.len() {
        let line_end = content[offset..]
            .find('\n')
            .map(|index| offset + index + 1)
            .unwrap_or(content.len());
        let line = &content[offset..line_end];
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_table = true;
            offset = line_end;
            continue;
        }
        if in_table {
            offset = line_end;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(key) {
            if rest.trim_start().starts_with('=') {
                let eq = line.find('=')?;
                let mut start = offset + eq + 1;
                while start < content.len()
                    && content[start..]
                        .chars()
                        .next()
                        .is_some_and(|ch| ch == ' ' || ch == '\t')
                {
                    start += 1;
                }
                let end = value_end(content, start);
                return Some((offset, end));
            }
        }
        offset = line_end;
    }
    None
}

fn value_end(content: &str, start: usize) -> usize {
    let mut index = start;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut structured = false;
    while index < content.len() {
        let ch = content[index..].chars().next().unwrap();
        if let Some(mark) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' && mark == '"' {
                escaped = true;
            } else if ch == mark {
                quote = None;
            }
            index += ch.len_utf8();
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '[' | '{' => {
                structured = true;
                depth += 1;
            }
            ']' | '}' => {
                depth -= 1;
                index += ch.len_utf8();
                if structured && depth <= 0 {
                    return index;
                }
                continue;
            }
            '\n' if !structured => return trim_end_index(content, start, index),
            '#' if !structured => return trim_end_index(content, start, index),
            _ => {}
        }
        index += ch.len_utf8();
        if structured && depth <= 0 {
            return index;
        }
    }
    trim_end_index(content, start, content.len())
}

fn trim_end_index(content: &str, start: usize, end: usize) -> usize {
    let mut end = end;
    while end > start {
        let prev = content[..end].chars().next_back().unwrap();
        if prev == ' ' || prev == '\t' || prev == '\r' {
            end -= prev.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn strip_block(content: &str) -> String {
    let Some(start) = content.find(BEGIN) else {
        return content.to_string();
    };
    let Some(relative_end) = content[start..].find(END) else {
        return content.to_string();
    };
    let mut end = start + relative_end + END.len();
    // 块本身以换行结尾；安装时块后面还留了一个空行分隔，两者一起删掉。
    for _ in 0..2 {
        if content[end..].starts_with('\n') {
            end += 1;
        }
    }
    let mut next = format!("{}{}", &content[..start], &content[end..]);
    if start == 0 {
        next = next.trim_start_matches('\n').to_string();
    }
    next
}

/// 把某段配置逐行注释掉，保留原内容便于用户恢复。
fn comment_lines(content: &str, start: usize, end: usize) -> String {
    let mut next = String::with_capacity(content.len() + 8);
    next.push_str(&content[..start]);
    for line in content[start..end].split_inclusive('\n') {
        if line.trim().is_empty() {
            next.push_str("#\n");
        } else {
            next.push_str("# ");
            next.push_str(line);
        }
    }
    next.push_str(&content[end..]);
    next
}

fn upsert(path: &Path, force: bool) -> Result<(), String> {
    let content = fs::read_to_string(path).unwrap_or_default();
    let existing = strip_block(&content);
    if let Some((start, end)) = find_root_value(&existing, KEY) {
        if !force {
            return Err(format!(
                "{} already sets `{KEY}`; use --force to replace it (agent-beacon will comment out the old value)",
                path.display()
            ));
        }
        let commented = comment_lines(&existing, start, end);
        let mut next = String::new();
        next.push_str(&block());
        next.push_str(&commented);
        return write_atomic(path, &next);
    }
    let mut next = String::new();
    next.push_str(&block());
    if !existing.is_empty() && !existing.starts_with('\n') {
        // 根级键必须写在所有表头之前，直接放在文件开头最安全。
        next.push('\n');
    }
    next.push_str(&existing);
    write_atomic(path, &next)
}

pub fn status(override_dir: Option<&Path>) -> Result<Status, String> {
    let path = config_path(override_dir)?;
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
    };
    let installed = if has_block(&content) {
        block_version(&content)
    } else {
        None
    };
    let state = match installed.as_deref() {
        None => State::NotInstalled,
        Some(version) if version == VERSION => State::UpToDate,
        Some(_) => State::UpgradeAvailable,
    };
    Ok(Status {
        target: Target::Codex,
        dir,
        path,
        installed_version: installed,
        embedded_version: VERSION,
        state,
    })
}

pub fn install(override_dir: Option<&Path>, force: bool) -> Result<(Outcome, PathBuf), String> {
    let path = config_path(override_dir)?;
    let Some(dir) = path.parent() else {
        return Err(format!("invalid path {}", path.display()));
    };
    if !dir.exists() {
        return Err(format!(
            "Codex config directory not found: {}. Run Codex once first.",
            dir.display()
        ));
    }
    let before = status(override_dir)?.installed_version;
    upsert(&path, force)?;
    let outcome = match before {
        None => Outcome::Installed,
        Some(version) if version == VERSION => Outcome::UpToDate,
        Some(_) => Outcome::Upgraded,
    };
    Ok((outcome, path))
}

pub fn upgrade(override_dir: Option<&Path>) -> Result<(Outcome, PathBuf), String> {
    let path = config_path(override_dir)?;
    let current = status(override_dir)?;
    let Some(version) = current.installed_version else {
        return Err(format!(
            "{} is not installed; run integrate install codex first",
            path.display()
        ));
    };
    upsert(&path, true)?;
    let outcome = if version == VERSION {
        Outcome::UpToDate
    } else {
        Outcome::Upgraded
    };
    Ok((outcome, path))
}

pub fn uninstall(override_dir: Option<&Path>) -> Result<PathBuf, String> {
    let path = config_path(override_dir)?;
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if !has_block(&content) {
        return Err(format!("{} is not installed", path.display()));
    }
    write_atomic(&path, &strip_block(&content))?;
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
        let dir = std::env::temp_dir().join(format!("agent-beacon-codex-{tag}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_inserts_notify_before_tables() {
        let dir = temp_dir("install");
        let path = dir.join("config.toml");
        fs::write(
            &path,
            "model = \"gpt-5\"\n\n[sandbox]\nmode = \"workspace-write\"\n",
        )
        .unwrap();

        let (outcome, _) = install(Some(&dir), false).unwrap();
        assert_eq!(outcome, Outcome::Installed);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(BEGIN));
        assert!(text.contains("notify = ["));
        assert!(text.contains("\"hook\", \"--agent\", \"codex\", \"--state\", \"auto\""));
        assert!(text.contains("[sandbox]"));
        assert_eq!(status(Some(&dir)).unwrap().state, State::UpToDate);

        let (again, _) = install(Some(&dir), false).unwrap();
        assert_eq!(again, Outcome::UpToDate);
        assert_eq!(fs::read_to_string(&path).unwrap().matches(BEGIN).count(), 1);

        let text = fs::read_to_string(&path).unwrap();
        let stale = text.replace(VERSION, "0.0.1");
        fs::write(&path, stale).unwrap();
        assert_eq!(status(Some(&dir)).unwrap().state, State::UpgradeAvailable);
        let (outcome, _) = upgrade(Some(&dir)).unwrap();
        assert_eq!(outcome, Outcome::Upgraded);

        uninstall(Some(&dir)).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains(BEGIN));
        assert!(!text.contains("notify"));
        assert!(text.contains("[sandbox]"));
        assert_eq!(status(Some(&dir)).unwrap().state, State::NotInstalled);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_foreign_notify_without_force() {
        let dir = temp_dir("foreign");
        let path = dir.join("config.toml");
        fs::write(&path, "notify = [\"say\", \"done\"]\n").unwrap();

        let error = install(Some(&dir), false).unwrap_err();
        assert!(error.contains("--force"));

        install(Some(&dir), true).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(BEGIN));
        assert!(text.contains("# notify = [\"say\", \"done\"]"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn multi_line_notify_is_detected() {
        let dir = temp_dir("multiline");
        let path = dir.join("config.toml");
        fs::write(&path, "[profile]\nnotify = [\n  \"say\",\n  \"done\",\n]\n").unwrap();
        // 表内的 notify 不属于根级配置，可以直接安装。
        install(Some(&dir), false).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(BEGIN));
        assert!(text.contains("[profile]"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn uninstall_restores_the_original_file_byte_for_byte() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("config.toml");
        let original = "model = \"gpt-5\"\n\n[sandbox]\nmode = \"workspace-write\"\n";
        fs::write(&path, original).unwrap();
        install(Some(&dir), false).unwrap();
        uninstall(Some(&dir)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn install_requires_codex_dir() {
        let dir = temp_dir("missing").join("nested");
        assert!(install(Some(&dir), false).is_err());
    }
}
