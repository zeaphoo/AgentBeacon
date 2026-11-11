//! 诊断日志：记录收到的 hook 事件，以及聚合状态/灯效为什么发生变化。
//!
//! 这是一个有界内存环形缓冲：服务不需要额外配置就会保留最近一段时间的
//! 事件与状态迁移，便于在灯效“闪一下”之后回看原因。日志通过
//! `GET /api/v1/hooks` 读取，也可以用 `agent-beacon hooks` 直接查看。

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};

use agent_beacon_core::{now_ms, AgentState, Aggregate, Effect};
use serde::Serialize;

/// 默认保留的记录条数。事件频率高时大约能覆盖最近几分钟到几十分钟。
pub const DEFAULT_CAPACITY: usize = 2_000;

/// 单条记录保留的原始上报长度上限（字符），避免超长载荷撑爆内存。
pub const MAX_RAW_CHARS: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    /// 收到并处理了一条 `/api/v1/events` 上报。
    Event,
    /// 会话 TTL 到期被清理。
    Expire,
    /// 手动覆盖生效。
    Override,
    /// 手动覆盖到期。
    OverrideExpired,
    /// 手动覆盖被取消。
    OverrideCleared,
    /// 配置被修改。
    Config,
    /// 所有会话被清空。
    SessionsCleared,
    /// 服务开始关闭。
    Shutdown,
}

impl HookKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HookKind::Event => "event",
            HookKind::Expire => "expire",
            HookKind::Override => "override",
            HookKind::OverrideExpired => "override_expired",
            HookKind::OverrideCleared => "override_cleared",
            HookKind::Config => "config",
            HookKind::SessionsCleared => "sessions_cleared",
            HookKind::Shutdown => "shutdown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutcome {
    Applied,
    Rejected,
}

impl HookOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            HookOutcome::Applied => "applied",
            HookOutcome::Rejected => "rejected",
        }
    }
}

/// 一条诊断记录。字段尽量与 `Event`/`Snapshot` 对齐，方便和 API 对照。
#[derive(Debug, Clone, Serialize)]
pub struct HookRecord {
    pub seq: u64,
    pub at_ms: u64,
    pub kind: HookKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<AgentState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// agent 的原始事件名，例如 `PreToolUse`、`tool_call`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u64>,
    pub outcome: HookOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub aggregate_before: Aggregate,
    pub aggregate_after: Aggregate,
    pub effect_before: Effect,
    pub effect_after: Effect,
    /// 人类可读的补充信息，例如过期会话列表或覆盖目标设备。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// 收到的原始上报（`/api/v1/events` 请求体），超长时截断。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

impl HookRecord {
    pub fn new(
        kind: HookKind,
        aggregate_before: Aggregate,
        effect_before: Effect,
        aggregate_after: Aggregate,
        effect_after: Effect,
    ) -> Self {
        Self {
            seq: 0,
            at_ms: 0,
            kind,
            agent: None,
            session: None,
            state: None,
            message: None,
            event: None,
            ttl_secs: None,
            outcome: HookOutcome::Applied,
            error: None,
            aggregate_before,
            aggregate_after,
            effect_before,
            effect_after,
            detail: None,
            raw: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn rejected(mut self, error: impl Into<String>) -> Self {
        self.outcome = HookOutcome::Rejected;
        self.error = Some(error.into());
        self
    }

    /// 保存原始上报文本（超过 `MAX_RAW_CHARS` 的部分会被截断）。
    pub fn with_raw(mut self, raw: Option<&str>) -> Self {
        self.raw = truncate_raw(raw);
        self
    }
}

/// 裁剪原始上报文本，去掉首尾空白并限制长度。
pub fn truncate_raw(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(MAX_RAW_CHARS).collect())
}

#[derive(Debug)]
struct Inner {
    next_seq: u64,
    entries: VecDeque<HookRecord>,
    capacity: usize,
}

/// 有界、线程安全的诊断记录缓冲。
#[derive(Debug)]
pub struct Journal {
    inner: Mutex<Inner>,
}

impl Default for Journal {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl Journal {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                next_seq: 0,
                entries: VecDeque::with_capacity(capacity.max(1)),
                capacity: capacity.max(1),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// 追加一条记录，返回分配到的序号。`at_ms` 为 0 时使用当前时间。
    pub fn record(&self, mut record: HookRecord) -> u64 {
        if record.at_ms == 0 {
            record.at_ms = now_ms();
        }
        let mut inner = self.lock();
        inner.next_seq = inner.next_seq.saturating_add(1);
        record.seq = inner.next_seq;
        inner.entries.push_back(record.clone());
        while inner.entries.len() > inner.capacity {
            inner.entries.pop_front();
        }
        let seq = record.seq;
        tracing::info!(
            seq,
            at_ms = record.at_ms,
            kind = record.kind.as_str(),
            outcome = record.outcome.as_str(),
            agent = record.agent.as_deref().unwrap_or(""),
            session = record.session.as_deref().unwrap_or(""),
            state = record.state.map(AgentState::as_str).unwrap_or(""),
            aggregate_before = record.aggregate_before.as_str(),
            aggregate_after = record.aggregate_after.as_str(),
            effect_before = record.effect_before.as_str(),
            effect_after = record.effect_after.as_str(),
            error = record.error.as_deref().unwrap_or(""),
            "hook journal"
        );
        seq
    }

    /// 最近的记录，按时间正序返回（最旧在前）。
    pub fn entries(&self, limit: usize) -> Vec<HookRecord> {
        let inner = self.lock();
        let limit = limit.clamp(1, inner.capacity);
        inner
            .entries
            .iter()
            .skip(inner.entries.len().saturating_sub(limit))
            .cloned()
            .collect()
    }

    pub fn capacity(&self) -> usize {
        self.lock().capacity
    }

    pub fn next_seq(&self) -> u64 {
        self.lock().next_seq
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        self.lock().entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> HookRecord {
        HookRecord::new(
            HookKind::Event,
            Aggregate::None,
            Effect::Off,
            Aggregate::Working,
            Effect::Busy,
        )
    }

    #[test]
    fn assigns_sequence_and_keeps_newest_entries() {
        let journal = Journal::new(2);
        assert_eq!(journal.record(record()), 1);
        assert_eq!(journal.record(record()), 2);
        assert_eq!(journal.record(record()), 3);
        let entries = journal.entries(10);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].seq, 2);
        assert_eq!(entries[1].seq, 3);
        assert_eq!(journal.next_seq(), 3);
    }

    #[test]
    fn limit_returns_latest_suffix() {
        let journal = Journal::new(10);
        for _ in 0..5 {
            journal.record(record());
        }
        let entries = journal.entries(2);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].seq, 4);
        assert_eq!(entries[1].seq, 5);
    }

    #[test]
    fn raw_payload_is_trimmed_and_truncated() {
        let long = "x".repeat(MAX_RAW_CHARS + 10);
        let kept = record().with_raw(Some(&format!("  {long}  "))).raw.unwrap();
        assert_eq!(kept.chars().count(), MAX_RAW_CHARS);
        assert!(record().with_raw(Some("   ")).raw.is_none());
        assert!(record().with_raw(None).raw.is_none());
    }

    #[test]
    fn clear_keeps_sequence_monotonic() {
        let journal = Journal::new(10);
        journal.record(record());
        journal.clear();
        assert!(journal.is_empty());
        assert_eq!(journal.record(record()), 2);
    }
}
