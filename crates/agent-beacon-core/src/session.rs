//! 会话、状态聚合与事件校验。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DEFAULT_TTL_SECS, MAX_TTL_SECS, MIN_TTL_SECS};

/// 单个 agent 会话的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Idle,
    Working,
    Waiting,
    Error,
}

impl AgentState {
    pub const ALL: [AgentState; 4] = [
        AgentState::Idle,
        AgentState::Working,
        AgentState::Waiting,
        AgentState::Error,
    ];

    /// 数值越大越需要关注。
    pub fn rank(self) -> u8 {
        match self {
            AgentState::Idle => 0,
            AgentState::Working => 1,
            AgentState::Waiting => 2,
            AgentState::Error => 3,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AgentState::Idle => "idle",
            AgentState::Working => "working",
            AgentState::Waiting => "waiting",
            AgentState::Error => "error",
        }
    }
}

/// 所有活动会话聚合后的唯一状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aggregate {
    None,
    Idle,
    Working,
    Waiting,
    Error,
}

impl Aggregate {
    pub fn from_state(state: AgentState) -> Self {
        match state {
            AgentState::Idle => Aggregate::Idle,
            AgentState::Working => Aggregate::Working,
            AgentState::Waiting => Aggregate::Waiting,
            AgentState::Error => Aggregate::Error,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Aggregate::None => "none",
            Aggregate::Idle => "idle",
            Aggregate::Working => "working",
            Aggregate::Waiting => "waiting",
            Aggregate::Error => "error",
        }
    }
}

/// agent hook 上报的事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub agent: String,
    pub state: AgentState,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub ttl_secs: Option<u64>,
}

impl Event {
    /// 缺省会话标识为 `{agent}:default`，让同一 agent 的重复上报更新同一条会话。
    pub fn session_id(&self) -> String {
        match self.session.as_deref().map(str::trim) {
            Some(value) if !value.is_empty() => value.to_string(),
            _ => format!("{}:default", self.agent.trim()),
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        let agent = self.agent.trim();
        if agent.is_empty() {
            return Err(ValidationError::EmptyAgent);
        }
        if agent.chars().count() > 64 {
            return Err(ValidationError::AgentTooLong);
        }
        if let Some(session) = self.session.as_deref() {
            if session.chars().count() > 128 {
                return Err(ValidationError::SessionTooLong);
            }
        }
        if let Some(message) = self.message.as_deref() {
            if message.chars().count() > 1024 {
                return Err(ValidationError::MessageTooLong);
            }
        }
        if let Some(ttl) = self.ttl_secs {
            if !(MIN_TTL_SECS..=MAX_TTL_SECS).contains(&ttl) {
                return Err(ValidationError::TtlOutOfRange);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("agent must not be empty")]
    EmptyAgent,
    #[error("agent name must not exceed 64 characters")]
    AgentTooLong,
    #[error("session id must not exceed 128 characters")]
    SessionTooLong,
    #[error("message must not exceed 1024 characters")]
    MessageTooLong,
    #[error("ttl_secs must be between 30 and 86400")]
    TtlOutOfRange,
}

/// 对外暴露的会话视图。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionView {
    pub agent: String,
    pub session: String,
    pub state: AgentState,
    pub message: Option<String>,
    pub updated_at_ms: u64,
    pub expires_at_ms: u64,
}

/// 会话表。所有方法都显式接收 `now_ms`，测试无需真实等待。
#[derive(Debug, Default)]
pub struct SessionRegistry {
    sessions: HashMap<(String, String), SessionView>,
    default_ttl_secs: u64,
}

impl SessionRegistry {
    pub fn new(default_ttl_secs: u64) -> Self {
        Self {
            sessions: HashMap::new(),
            default_ttl_secs,
        }
    }

    pub fn with_default_ttl() -> Self {
        Self::new(DEFAULT_TTL_SECS)
    }

    pub fn default_ttl_secs(&self) -> u64 {
        self.default_ttl_secs
    }

    pub fn apply(&mut self, event: &Event, now_ms: u64) -> Result<SessionView, ValidationError> {
        event.validate()?;
        let ttl = event.ttl_secs.unwrap_or(self.default_ttl_secs);
        let view = SessionView {
            agent: event.agent.trim().to_string(),
            session: event.session_id(),
            state: event.state,
            message: event.message.clone().filter(|m| !m.is_empty()),
            updated_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(ttl.saturating_mul(1000)),
        };
        self.sessions
            .insert((view.agent.clone(), view.session.clone()), view.clone());
        Ok(view)
    }

    /// 删除过期会话，返回是否有变化。
    pub fn expire(&mut self, now_ms: u64) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|_, view| view.expires_at_ms > now_ms);
        before != self.sessions.len()
    }

    /// 聚合活动会话；优先级 `error > waiting > working > idle`，同优先级取最近更新。
    pub fn aggregate_at(&self, now_ms: u64) -> Aggregate {
        self.sessions
            .values()
            .filter(|view| view.expires_at_ms > now_ms)
            .max_by_key(|view| (view.state.rank(), view.updated_at_ms))
            .map(|view| Aggregate::from_state(view.state))
            .unwrap_or(Aggregate::None)
    }

    pub fn aggregate(&self) -> Aggregate {
        self.aggregate_at(crate::now_ms())
    }

    /// 活动会话，按最近更新倒序。
    pub fn sessions_at(&self, now_ms: u64) -> Vec<SessionView> {
        let mut views: Vec<SessionView> = self
            .sessions
            .values()
            .filter(|view| view.expires_at_ms > now_ms)
            .cloned()
            .collect();
        views.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        views
    }

    pub fn sessions(&self) -> Vec<SessionView> {
        self.sessions_at(crate::now_ms())
    }

    pub fn clear(&mut self) {
        self.sessions.clear();
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(agent: &str, state: AgentState) -> Event {
        Event {
            agent: agent.to_string(),
            state,
            session: None,
            message: None,
            ttl_secs: None,
        }
    }

    #[test]
    fn default_session_is_reused() {
        let mut registry = SessionRegistry::with_default_ttl();
        registry
            .apply(&event("claude", AgentState::Working), 1_000)
            .unwrap();
        registry
            .apply(&event("claude", AgentState::Error), 2_000)
            .unwrap();

        assert_eq!(registry.len(), 1);
        let sessions = registry.sessions_at(2_000);
        assert_eq!(sessions[0].session, "claude:default");
        assert_eq!(sessions[0].state, AgentState::Error);
    }

    #[test]
    fn sessions_are_isolated() {
        let mut registry = SessionRegistry::with_default_ttl();
        let mut a = event("codex", AgentState::Working);
        a.session = Some("a".into());
        let mut b = event("codex", AgentState::Waiting);
        b.session = Some("b".into());
        registry.apply(&a, 1_000).unwrap();
        registry.apply(&b, 1_000).unwrap();

        assert_eq!(registry.len(), 2);
        assert_eq!(registry.aggregate_at(2_000), Aggregate::Waiting);
    }

    #[test]
    fn invalid_events_are_rejected() {
        let mut registry = SessionRegistry::with_default_ttl();
        let mut empty = event("  ", AgentState::Working);
        assert_eq!(registry.apply(&empty, 0), Err(ValidationError::EmptyAgent));

        empty.agent = "x".into();
        empty.ttl_secs = Some(1);
        assert_eq!(
            registry.apply(&empty, 0),
            Err(ValidationError::TtlOutOfRange)
        );
    }

    #[test]
    fn expired_sessions_are_dropped() {
        let mut registry = SessionRegistry::with_default_ttl();
        registry
            .apply(&event("claude", AgentState::Working), 0)
            .unwrap();
        assert_eq!(registry.sessions_at(1_000).len(), 1);
        assert!(registry.expire(900_001));
        assert!(registry.is_empty());
        assert_eq!(registry.aggregate_at(900_001), Aggregate::None);
    }

    #[test]
    fn repeated_events_refresh_ttl() {
        let mut registry = SessionRegistry::with_default_ttl();
        registry
            .apply(&event("claude", AgentState::Working), 0)
            .unwrap();
        registry
            .apply(&event("claude", AgentState::Working), 800_000)
            .unwrap();
        assert_eq!(registry.sessions_at(900_001).len(), 1);
    }

    #[test]
    fn aggregate_follows_priority() {
        let mut registry = SessionRegistry::with_default_ttl();
        registry.apply(&event("a", AgentState::Idle), 1).unwrap();
        assert_eq!(registry.aggregate_at(2), Aggregate::Idle);
        registry.apply(&event("b", AgentState::Working), 1).unwrap();
        assert_eq!(registry.aggregate_at(2), Aggregate::Working);
        registry.apply(&event("c", AgentState::Waiting), 1).unwrap();
        assert_eq!(registry.aggregate_at(2), Aggregate::Waiting);
        registry.apply(&event("d", AgentState::Error), 1).unwrap();
        assert_eq!(registry.aggregate_at(2), Aggregate::Error);
        assert_eq!(registry.aggregate_at(u64::MAX - 1), Aggregate::None);
    }
}
