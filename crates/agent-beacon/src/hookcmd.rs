//! `agent-beacon hook`：把各家 agent 的 hook 载荷转换成一次状态上报。
//!
//! 不同 agent 调用 hook 的方式不同：
//!
//! - Claude Code / Qoder / Gemini 家族：命令通过 stdin 收到 JSON 载荷；
//! - Codex：`notify` 把 JSON 载荷作为最后一个参数传入。
//!
//! 这里统一读取载荷、提取 session id 和事件名、按需推导状态，然后复用
//! `/api/v1/events` 上报。读不到可用状态时安静退出，不影响 agent。

use std::io::{IsTerminal, Read};

use serde_json::{json, Value};

const MAX_PAYLOAD_BYTES: u64 = 64 * 1024;

const SESSION_KEYS: &[&str] = &[
    "session_id",
    "sessionId",
    "conversation_id",
    "conversationId",
    "thread_id",
    "threadId",
    "thread-id",
    "turn-id",
    "session",
];

const EVENT_KEYS: &[&str] = &[
    "hook_event_name",
    "hookEventName",
    "event_name",
    "eventName",
    "event",
    "type",
];

const TOOL_KEYS: &[&str] = &["tool_name", "toolName", "tool"];

const MESSAGE_KEYS: &[&str] = &["message", "notification", "reason"];

/// 读取 hook 载荷：优先用命令行传入，其次读 stdin（仅当不是终端时）。
fn read_payload(argument: Option<&str>) -> Option<Value> {
    if let Some(raw) = argument {
        if let Ok(value) = serde_json::from_str::<Value>(raw) {
            return Some(value);
        }
    }
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return None;
    }
    let mut buffer = String::new();
    if stdin
        .lock()
        .take(MAX_PAYLOAD_BYTES)
        .read_to_string(&mut buffer)
        .is_err()
    {
        return None;
    }
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(trimmed).ok()
}

fn string_field<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a str> {
    let object = payload.as_object()?;
    for key in keys {
        if let Some(value) = object.get(*key).and_then(Value::as_str) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// 根据 agent 与事件名推导状态；无法判断时返回 `None`（不上报）。
pub fn state_for(agent: &str, event: &str) -> Option<&'static str> {
    let event = event.to_ascii_lowercase();
    let has = |needle: &str| event.contains(needle);

    if agent == "codex" {
        return if has("approval") || has("permission") {
            Some("waiting")
        } else if has("error") || has("fail") {
            Some("error")
        } else if has("turn-complete") || has("turn_complete") || has("complete") {
            Some("idle")
        } else {
            None
        };
    }

    if has("notification") || has("approval") || has("permission") || has("waiting") {
        return Some("waiting");
    }
    if has("error") || has("fail") {
        return Some("error");
    }
    if has("sessionstart") || has("session-start") || has("session_end") || has("sessionend") {
        return Some("idle");
    }
    if has("prompt") || has("beforeagent") || has("agent-start") || has("tool") {
        return Some("working");
    }
    if has("stop") || has("afteragent") || has("settled") || has("end") {
        return Some("idle");
    }
    None
}

#[derive(Debug, Default, Clone)]
pub struct HookRequest {
    pub agent: String,
    pub state: Option<String>,
    pub event: Option<String>,
    pub message: Option<String>,
    pub ttl: Option<u64>,
    pub payload: Option<String>,
}

/// 组装上报 body；返回 `None` 表示这次 hook 不需要上报。
pub fn build_body(request: &HookRequest) -> Option<Value> {
    let payload = read_payload(request.payload.as_deref()).unwrap_or(Value::Null);
    let payload_event = string_field(&payload, EVENT_KEYS).map(str::to_string);
    let event = request
        .event
        .clone()
        .or_else(|| payload_event.clone())
        .unwrap_or_default();

    let state = match request.state.as_deref() {
        Some("auto") | None => state_for(&request.agent, &event)?.to_string(),
        Some(state) => state.to_string(),
    };

    let mut body = json!({ "agent": request.agent, "state": state });
    if let Some(session) = string_field(&payload, SESSION_KEYS) {
        body["session"] = json!(session);
    }
    let tool = string_field(&payload, TOOL_KEYS);
    let message = request
        .message
        .clone()
        .or_else(|| string_field(&payload, MESSAGE_KEYS).map(str::to_string))
        .or_else(|| {
            // 工具事件用工具名，其他事件用事件名，避免 `Stop` 也显示上一次的工具名。
            let event_lower = event.to_ascii_lowercase();
            if event_lower.contains("tool") {
                tool.map(str::to_string)
            } else {
                None
            }
        })
        .or_else(|| (!event.is_empty()).then(|| event.clone()));
    if let Some(message) = message {
        body["message"] = json!(truncate(&message, 1024));
    }
    if let Some(ttl) = request.ttl {
        body["ttl_secs"] = json!(ttl);
    }
    Some(body)
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(agent: &str, state: Option<&str>, event: Option<&str>) -> HookRequest {
        HookRequest {
            agent: agent.to_string(),
            state: state.map(str::to_string),
            event: event.map(str::to_string),
            ..HookRequest::default()
        }
    }

    #[test]
    fn codex_maps_notify_events() {
        assert_eq!(state_for("codex", "agent-turn-complete"), Some("idle"));
        assert_eq!(state_for("codex", "approval-requested"), Some("waiting"));
        assert_eq!(state_for("codex", "something-else"), None);
    }

    #[test]
    fn claude_style_events_map_to_states() {
        assert_eq!(state_for("claude-code", "SessionStart"), Some("idle"));
        assert_eq!(
            state_for("claude-code", "UserPromptSubmit"),
            Some("working")
        );
        assert_eq!(state_for("claude-code", "PreToolUse"), Some("working"));
        assert_eq!(state_for("claude-code", "Notification"), Some("waiting"));
        assert_eq!(state_for("claude-code", "Stop"), Some("idle"));
        assert_eq!(state_for("gemini", "BeforeAgent"), Some("working"));
        assert_eq!(state_for("gemini", "AfterAgent"), Some("idle"));
    }

    #[test]
    fn explicit_state_wins_over_payload() {
        let body = build_body(&request("qoder", Some("working"), Some("Stop"))).unwrap();
        assert_eq!(body["state"], "working");
    }

    #[test]
    fn codex_payload_is_parsed_from_argument() {
        let mut request = request("codex", Some("auto"), None);
        request.payload = Some(
            r#"{"type":"approval-requested","turn-id":"abc","tool_name":"shell"}"#.to_string(),
        );
        let body = build_body(&request).unwrap();
        assert_eq!(body["state"], "waiting");
        assert_eq!(body["session"], "abc");
        // 审批事件不是工具事件，消息回落成事件名而不是工具名。
        assert_eq!(body["message"], "approval-requested");
    }

    #[test]
    fn tool_events_use_the_tool_name_as_message() {
        let mut request = request("claude-code", Some("working"), Some("PostToolUse"));
        request.payload = Some(r#"{"tool_name":"Bash"}"#.to_string());
        let body = build_body(&request).unwrap();
        assert_eq!(body["message"], "Bash");
    }

    #[test]
    fn payload_message_is_used_when_present() {
        let mut request = request("claude-code", Some("waiting"), Some("Notification"));
        request.payload =
            Some(r#"{"message":"Claude needs your permission to use Bash"}"#.to_string());
        let body = build_body(&request).unwrap();
        assert_eq!(body["message"], "Claude needs your permission to use Bash");
    }

    #[test]
    fn stop_events_fall_back_to_the_event_name() {
        let mut request = request("claude-code", Some("idle"), Some("Stop"));
        request.payload = Some(r#"{"tool_name":"Bash"}"#.to_string());
        let body = build_body(&request).unwrap();
        assert_eq!(body["message"], "Stop");
    }

    #[test]
    fn session_and_message_come_from_payload() {
        let mut request = request("claude-code", Some("working"), Some("PreToolUse"));
        request.payload = Some(
            r#"{"session_id":"s-42","tool_name":"Bash","hook_event_name":"PreToolUse"}"#
                .to_string(),
        );
        let body = build_body(&request).unwrap();
        assert_eq!(body["session"], "s-42");
        assert_eq!(body["message"], "Bash");
        assert_eq!(body["state"], "working");
    }

    #[test]
    fn unknown_event_without_state_is_skipped() {
        let mut request = request("codex", Some("auto"), None);
        request.payload = Some(r#"{"type":"unknown"}"#.to_string());
        assert!(build_body(&request).is_none());
    }

    #[test]
    fn ttl_is_forwarded() {
        let mut request = request("claude-code", Some("idle"), None);
        request.ttl = Some(30);
        let body = build_body(&request).unwrap();
        assert_eq!(body["ttl_secs"], 30);
    }
}
