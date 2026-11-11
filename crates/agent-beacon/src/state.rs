//! 服务运行状态：会话、配置、手动覆盖、设备视图与 SSE 广播。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use agent_beacon_core::{
    aggregate_of, now_ms, Aggregate, Config, DeviceAssignment, DeviceProfile, DeviceProfileInfo,
    Effect, Event, SessionRegistry, SessionView, ValidationError,
};
use agent_beacon_lights::device::{
    new_registry, remove as remove_device, snapshot as device_snapshot, DeviceRegistry,
};
use agent_beacon_lights::{DesiredState, DeviceDesired, DeviceInfo};
use serde::Serialize;
use tokio::sync::{broadcast, watch, Notify};

use crate::config_store::ConfigStore;
use crate::journal::{HookKind, HookOutcome, HookRecord, Journal, DEFAULT_CAPACITY};
use crate::routing::{
    empty_effect, resolve_devices, AssignmentEngine, AssignmentResult, DeviceRouteView,
    ResolvedDevice,
};
use crate::VERSION;

#[derive(Debug, Clone, Serialize)]
pub struct ServiceInfo {
    pub version: &'static str,
    pub pid: u32,
    pub port: u16,
    pub started_at_ms: u64,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverrideView {
    pub effect: Effect,
    pub expires_at_ms: u64,
    pub device: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub aggregate: Aggregate,
    pub sessions: Vec<SessionView>,
    /// 内存中保留的最近会话记录条数（含已过期）。
    pub session_history_len: usize,
    pub devices: Vec<DeviceInfo>,
    pub routing: Vec<DeviceRouteView>,
    pub unassigned_agents: Vec<String>,
    pub profiles: Vec<DeviceProfileInfo>,
    pub effect: Effect,
    #[serde(rename = "override")]
    pub manual_override: Option<OverrideView>,
    pub service: ServiceInfo,
    pub config: Config,
}

struct ManualOverride {
    effect: Effect,
    expires_at_ms: u64,
    device: Option<String>,
}

struct Inner {
    config: Config,
    sessions: SessionRegistry,
    manual_override: Option<ManualOverride>,
    assignment: AssignmentEngine,
    routes: Vec<DeviceRouteView>,
    unassigned_agents: Vec<String>,
    /// 最近一次下发的期望状态，用于避免每 500ms 重复唤醒设备 actor。
    last_desired: DesiredState,
    /// 最近一次实际下发的聚合/灯效，用于诊断日志精确记录“变化前”。
    last_view: Transition,
}

pub struct BeaconState {
    inner: Mutex<Inner>,
    devices: DeviceRegistry,
    desired_tx: watch::Sender<DesiredState>,
    events: broadcast::Sender<Snapshot>,
    journal: Journal,
    started: Instant,
    started_at_ms: u64,
    port: u16,
    store: ConfigStore,
    scan: Notify,
    shutdown: watch::Sender<bool>,
}

impl BeaconState {
    pub fn new(config: Config, store: ConfigStore, port: u16) -> Arc<Self> {
        let initial = initial_desired(&config);
        let (desired_tx, _) = watch::channel(initial.clone());
        let (events, _) = broadcast::channel(64);
        let (shutdown, _) = watch::channel(false);
        let started_at_ms = now_ms();
        let last_view = Transition {
            aggregate: Aggregate::None,
            effect: config.mapping.effect_for(Aggregate::None),
        };
        let state = Arc::new(Self {
            inner: Mutex::new(Inner {
                config,
                sessions: SessionRegistry::with_default_ttl(),
                manual_override: None,
                assignment: AssignmentEngine::default(),
                routes: Vec::new(),
                unassigned_agents: Vec::new(),
                last_desired: initial,
                last_view,
            }),
            devices: new_registry(),
            desired_tx,
            events,
            journal: Journal::new(DEFAULT_CAPACITY),
            started: Instant::now(),
            started_at_ms,
            port,
            store,
            scan: Notify::new(),
            shutdown,
        });
        state.refresh();
        state
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn devices(&self) -> &DeviceRegistry {
        &self.devices
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn scan_notify(&self) -> &Notify {
        &self.scan
    }

    pub fn request_scan(&self) {
        self.scan.notify_waiters();
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<Snapshot> {
        self.events.subscribe()
    }

    pub fn subscribe_desired(&self) -> watch::Receiver<DesiredState> {
        self.desired_tx.subscribe()
    }

    /// 通知所有 SSE 流结束，让 graceful shutdown 不被长连接卡住。
    pub fn begin_shutdown(&self) {
        tracing::info!("shutting down: notifying SSE clients");
        {
            let inner = self.lock();
            let current = inner.last_view;
            self.journal.record(HookRecord::new(
                HookKind::Shutdown,
                current.aggregate,
                current.effect,
                current.aggregate,
                current.effect,
            ));
        }
        let _ = self.shutdown.send_replace(true);
    }

    pub fn subscribe_shutdown(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }

    pub fn config(&self) -> Config {
        self.lock().config.clone()
    }

    /// 最近的会话记录（含已过期），仅来自内存。
    pub fn recent_sessions(&self, limit: usize) -> Vec<SessionView> {
        self.lock().sessions.recent_sessions(limit)
    }

    /// 内存会话记录上限。
    pub fn session_history_capacity(&self) -> usize {
        self.lock().sessions.history_capacity()
    }

    /// 内存中保留的最近会话记录条数。
    pub fn session_history_len(&self) -> usize {
        self.lock().sessions.history_len()
    }

    pub fn store(&self) -> &ConfigStore {
        &self.store
    }

    pub fn snapshot(&self) -> Snapshot {
        let now = now_ms();
        let inner = self.lock();
        let aggregate = inner.sessions.aggregate_at(now);
        let sessions = inner.sessions.sessions_at(now);
        let manual_override = inner
            .manual_override
            .as_ref()
            .filter(|item| item.expires_at_ms > now)
            .map(|item| OverrideView {
                effect: item.effect,
                expires_at_ms: item.expires_at_ms,
                device: item.device.clone(),
            });
        Snapshot {
            aggregate,
            sessions,
            session_history_len: inner.sessions.history_len(),
            devices: device_snapshot(&self.devices),
            routing: inner.routes.clone(),
            unassigned_agents: inner.unassigned_agents.clone(),
            profiles: DeviceProfile::ALL
                .into_iter()
                .map(|profile| profile.info())
                .collect(),
            effect: effective_global_effect(
                &inner.config,
                aggregate,
                inner.manual_override.as_ref(),
                now,
            ),
            manual_override,
            service: ServiceInfo {
                version: VERSION,
                pid: std::process::id(),
                port: self.port,
                started_at_ms: self.started_at_ms,
                uptime_secs: self.started.elapsed().as_secs(),
            },
            config: inner.config.clone(),
        }
    }

    pub fn publish(&self) {
        let snapshot = self.snapshot();
        let _ = self.events.send(snapshot);
    }

    /// 重新计算分配、路由视图与每设备期望状态；返回是否有可见变化。
    fn refresh(&self) -> bool {
        let now = now_ms();
        let mut inner = self.lock();
        // 退出关灯后不再被 tick/事件重新点亮。
        if inner.last_desired.forced_off {
            return false;
        }
        let config = inner.config.clone();
        let sessions = inner.sessions.sessions_at(now);
        let devices = resolve_devices(&config, &self.devices);
        let result = inner.assignment.resolve(&sessions, &devices, &config, now);
        let global_aggregate = aggregate_of(sessions.iter());
        let routes = build_routes(
            &config,
            &devices,
            &result,
            &sessions,
            inner.manual_override.as_ref(),
            now,
        );
        let desired = build_desired(
            &config,
            &devices,
            &routes,
            global_aggregate,
            inner.manual_override.as_ref(),
            now,
        );

        let changed = inner.routes != routes
            || inner.unassigned_agents != result.unassigned_agents
            || inner.last_desired != desired;
        inner.routes = routes;
        inner.unassigned_agents = result.unassigned_agents;
        let desired_changed = inner.last_desired != desired;
        if desired_changed {
            inner.last_desired = desired.clone();
        }
        drop(inner);
        if desired_changed {
            let _ = self.desired_tx.send_replace(desired);
        }
        changed
    }

    /// 设备 actor 启动时注册；立即刷新一次，避免新设备先闪全局效果。
    pub fn register_device(&self, mut info: DeviceInfo) {
        let config = self.config();
        if let Some(binding) = crate::routing::binding_for_device(&config, &info.id, &info.port) {
            info.label = binding.label.clone();
        }
        if let Ok(mut devices) = self.devices.lock() {
            if let Some(existing) = devices.get(&info.id) {
                info.connected = info.connected || existing.connected;
                info.last_seen_ms = info.last_seen_ms.or(existing.last_seen_ms);
                info.effect = existing.effect;
                if info.label.is_none() {
                    info.label = existing.label.clone();
                }
            }
            devices.insert(info.id.clone(), info);
        }
        if self.refresh() {
            self.publish();
        }
    }

    pub fn unregister_device(&self, id: &str) {
        remove_device(&self.devices, id);
        if self.refresh() {
            self.publish();
        }
    }

    pub fn apply_event(&self, event: Event) -> Result<SessionView, ValidationError> {
        let now = now_ms();
        let mut inner = self.lock();
        let before = inner.last_view;
        let result = inner.sessions.apply(&event, now);
        match result {
            Ok(view) => {
                let after = transition(&inner, now);
                inner.last_view = after;
                self.journal.record(event_record(
                    &event,
                    Some(&view),
                    before,
                    after,
                    HookOutcome::Applied,
                    None,
                ));
                drop(inner);
                self.refresh();
                self.publish();
                Ok(view)
            }
            Err(error) => {
                self.journal.record(event_record(
                    &event,
                    None,
                    before,
                    before,
                    HookOutcome::Rejected,
                    Some(error.to_string()),
                ));
                Err(error)
            }
        }
    }

    /// 记录无法解析成 `Event` 的上报，便于排查 hook 版本/字段错误。
    pub fn record_rejected_hook(&self, error: impl Into<String>) {
        let inner = self.lock();
        let current = inner.last_view;
        self.journal.record(
            HookRecord::new(
                HookKind::Event,
                current.aggregate,
                current.effect,
                current.aggregate,
                current.effect,
            )
            .rejected(error),
        );
    }

    pub fn clear_sessions(&self) {
        let now = now_ms();
        let mut inner = self.lock();
        let before = inner.last_view;
        inner.sessions.clear();
        inner.assignment.clear();
        let after = transition(&inner, now);
        inner.last_view = after;
        self.journal.record(HookRecord::new(
            HookKind::SessionsCleared,
            before.aggregate,
            before.effect,
            after.aggregate,
            after.effect,
        ));
        drop(inner);
        self.refresh();
        self.publish();
    }

    pub fn set_config(&self, config: Config) -> Result<(), String> {
        self.store.save(&config)?;
        let now = now_ms();
        let mut inner = self.lock();
        let before = inner.last_view;
        inner.config = config;
        let devices = resolve_devices(&inner.config, &self.devices);
        if let Ok(mut registry) = self.devices.lock() {
            for device in devices.iter().filter(|device| device.registered) {
                if let Some(info) = registry.get_mut(&device.id) {
                    info.label = device.label.clone();
                }
            }
        }
        let detail = routing_summary(&inner.config);
        let after = transition(&inner, now);
        inner.last_view = after;
        self.journal.record(
            HookRecord::new(
                HookKind::Config,
                before.aggregate,
                before.effect,
                after.aggregate,
                after.effect,
            )
            .with_detail(detail),
        );
        drop(inner);
        self.refresh();
        self.publish();
        Ok(())
    }

    pub fn set_override(&self, effect: Effect, duration_secs: u64, device: Option<String>) {
        let now = now_ms();
        let mut inner = self.lock();
        let before = inner.last_view;
        let detail = match &device {
            Some(device) => format!(
                "effect={} duration={}s device={device}",
                effect.as_str(),
                duration_secs
            ),
            None => format!(
                "effect={} duration={}s all devices",
                effect.as_str(),
                duration_secs
            ),
        };
        inner.manual_override = Some(ManualOverride {
            effect,
            expires_at_ms: now.saturating_add(duration_secs.saturating_mul(1000)),
            device,
        });
        let after = transition(&inner, now);
        inner.last_view = after;
        self.journal.record(
            HookRecord::new(
                HookKind::Override,
                before.aggregate,
                before.effect,
                after.aggregate,
                after.effect,
            )
            .with_detail(detail),
        );
        drop(inner);
        self.refresh();
        self.publish();
    }

    pub fn clear_override(&self) {
        let now = now_ms();
        let mut inner = self.lock();
        let before = inner.last_view;
        inner.manual_override = None;
        let after = transition(&inner, now);
        inner.last_view = after;
        self.journal.record(HookRecord::new(
            HookKind::OverrideCleared,
            before.aggregate,
            before.effect,
            after.aggregate,
            after.effect,
        ));
        drop(inner);
        self.refresh();
        self.publish();
    }

    /// 清空粘性分配并按当前 session 全量重算。
    pub fn rebuild_routing(&self) {
        let now = now_ms();
        let mut inner = self.lock();
        let before = inner.last_view;
        inner.assignment.clear();
        let after = transition(&inner, now);
        inner.last_view = after;
        self.journal.record(
            HookRecord::new(
                HookKind::Config,
                before.aggregate,
                before.effect,
                after.aggregate,
                after.effect,
            )
            .with_detail("routing rebuilt"),
        );
        drop(inner);
        self.refresh();
        self.publish();
    }

    /// 到期清理；有变化时返回 true 并已广播。
    pub fn tick(&self) -> bool {
        let now = now_ms();
        let mut inner = self.lock();
        let mut changed = false;

        let before = inner.last_view;
        let expired = inner.sessions.expire_and_collect(now);
        if !expired.is_empty() {
            let after = transition(&inner, now);
            inner.last_view = after;
            let detail = expired
                .iter()
                .map(|view| format!("{}/{} {}", view.agent, view.session, view.state.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            self.journal.record(
                HookRecord::new(
                    HookKind::Expire,
                    before.aggregate,
                    before.effect,
                    after.aggregate,
                    after.effect,
                )
                .with_detail(format!("expired: {detail}")),
            );
            changed = true;
        }

        let override_expired = inner
            .manual_override
            .as_ref()
            .is_some_and(|item| item.expires_at_ms <= now);
        if override_expired {
            let before = inner.last_view;
            inner.manual_override = None;
            let after = transition(&inner, now);
            inner.last_view = after;
            self.journal.record(HookRecord::new(
                HookKind::OverrideExpired,
                before.aggregate,
                before.effect,
                after.aggregate,
                after.effect,
            ));
            changed = true;
        }

        drop(inner);
        let refreshed = self.refresh();
        if changed || refreshed {
            self.publish();
        }
        changed || refreshed
    }

    /// 退出前把灯全部关掉（不依赖映射）。
    pub fn force_off(&self) {
        let mut inner = self.lock();
        let desired = DesiredState {
            forced_off: true,
            ..DesiredState::default()
        };
        inner.last_desired = desired.clone();
        drop(inner);
        let _ = self.desired_tx.send_replace(desired);
    }

    pub fn health_json(&self) -> serde_json::Value {
        serde_json::json!({
            "status": "ok",
            "version": VERSION,
            "pid": std::process::id(),
            "port": self.port,
            "uptime_secs": self.started.elapsed().as_secs(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct Transition {
    aggregate: Aggregate,
    effect: Effect,
}

fn transition(inner: &Inner, now: u64) -> Transition {
    let aggregate = inner.sessions.aggregate_at(now);
    Transition {
        aggregate,
        effect: effective_global_effect(
            &inner.config,
            aggregate,
            inner.manual_override.as_ref(),
            now,
        ),
    }
}

fn event_record(
    event: &Event,
    view: Option<&SessionView>,
    before: Transition,
    after: Transition,
    outcome: HookOutcome,
    error: Option<String>,
) -> HookRecord {
    let agent = event.agent.trim();
    let mut record = HookRecord::new(
        HookKind::Event,
        before.aggregate,
        before.effect,
        after.aggregate,
        after.effect,
    );
    record.agent = (!agent.is_empty()).then(|| agent.to_string());
    record.session = Some(
        view.map(|view| view.session.clone())
            .unwrap_or_else(|| event.session_id()),
    );
    record.state = Some(event.state);
    record.message = event.message.clone().filter(|message| !message.is_empty());
    record.ttl_secs = event.ttl_secs;
    record.outcome = outcome;
    record.error = error;
    if let Some(message) = view.and_then(|view| view.message.clone()) {
        record.detail = Some(format!("message: {message}"));
    }
    record
}

fn initial_desired(config: &Config) -> DesiredState {
    DesiredState {
        devices: HashMap::new(),
        default: DeviceDesired {
            effect: config.mapping.effect_for(Aggregate::None),
            brightness: config.mlight.brightness,
            buzzer_on_alert: config.yh50.buzzer_on_alert,
        },
        forced_off: false,
    }
}

fn effective_global_effect(
    config: &Config,
    aggregate: Aggregate,
    manual_override: Option<&ManualOverride>,
    now: u64,
) -> Effect {
    if let Some(item) = manual_override {
        if item.expires_at_ms > now && item.device.is_none() {
            return item.effect;
        }
    }
    config.mapping.effect_for(aggregate)
}

fn build_routes(
    config: &Config,
    devices: &[ResolvedDevice],
    result: &AssignmentResult,
    all_sessions: &[SessionView],
    manual_override: Option<&ManualOverride>,
    now: u64,
) -> Vec<DeviceRouteView> {
    devices
        .iter()
        .map(|device| {
            let mut sessions = if device.assignment == DeviceAssignment::Mirror {
                all_sessions.to_vec()
            } else {
                result
                    .by_device
                    .get(&device.id)
                    .cloned()
                    .unwrap_or_default()
            };
            sessions.sort_by_key(|view| std::cmp::Reverse(view.updated_at_ms));
            let empty = sessions.is_empty();
            let aggregate = aggregate_of(sessions.iter());
            let auto_effect = if empty {
                empty_effect(config)
            } else {
                config.mapping.effect_for(aggregate)
            };
            let mut effect = auto_effect;
            let mut overridden = false;
            if let Some(item) = manual_override.filter(|item| item.expires_at_ms > now) {
                match &item.device {
                    None => {
                        effect = item.effect;
                        overridden = true;
                    }
                    Some(id) if id == &device.id => {
                        effect = item.effect;
                        overridden = true;
                    }
                    _ => {}
                }
            }
            let mut agents: Vec<String> = sessions.iter().map(|view| view.agent.clone()).collect();
            agents.sort();
            agents.dedup();
            DeviceRouteView {
                device: device.id.clone(),
                label: device.label.clone(),
                assignment: device.assignment,
                agents,
                sessions,
                aggregate,
                auto_effect,
                effect,
                overridden,
                connected: device.connected,
                last_seen_ms: device.last_seen_ms,
            }
        })
        .collect()
}

fn build_desired(
    config: &Config,
    devices: &[ResolvedDevice],
    routes: &[DeviceRouteView],
    global_aggregate: Aggregate,
    manual_override: Option<&ManualOverride>,
    now: u64,
) -> DesiredState {
    let route_by_id: HashMap<&str, &DeviceRouteView> = routes
        .iter()
        .map(|route| (route.device.as_str(), route))
        .collect();
    let mut desired_devices = HashMap::new();
    for device in devices.iter().filter(|device| device.registered) {
        let Some(route) = route_by_id.get(device.id.as_str()) else {
            continue;
        };
        desired_devices.insert(
            device.id.clone(),
            DeviceDesired {
                effect: route.effect,
                brightness: device.brightness.unwrap_or(config.mlight.brightness),
                buzzer_on_alert: device
                    .buzzer_on_alert
                    .unwrap_or(config.yh50.buzzer_on_alert),
            },
        );
    }
    DesiredState {
        devices: desired_devices,
        default: DeviceDesired {
            effect: effective_global_effect(config, global_aggregate, manual_override, now),
            brightness: config.mlight.brightness,
            buzzer_on_alert: config.yh50.buzzer_on_alert,
        },
        forced_off: false,
    }
}

fn routing_summary(config: &Config) -> String {
    format!(
        "configuration updated (strategy={}, devices={}, manual_lights={})",
        config.routing.strategy.as_str(),
        config.devices.len(),
        config.routing.manual.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_beacon_core::{AgentState, Config, DeviceProfile};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn state() -> Arc<BeaconState> {
        state_with(Config::default())
    }

    fn state_with(config: Config) -> Arc<BeaconState> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            + u128::from(COUNTER.fetch_add(1, Ordering::Relaxed));
        let dir = std::env::temp_dir().join(format!("beacon-state-{unique}"));
        let store = ConfigStore::new(dir.join("config.json"));
        store.save(&config).unwrap();
        BeaconState::new(config, store, 6980)
    }

    fn event(state: AgentState) -> Event {
        Event {
            agent: "test".into(),
            state,
            session: None,
            message: None,
            ttl_secs: None,
        }
    }

    #[test]
    fn snapshot_tracks_events_and_mapping() {
        let state = state();
        assert_eq!(state.snapshot().aggregate, Aggregate::None);
        state.apply_event(event(AgentState::Working)).unwrap();
        let snapshot = state.snapshot();
        assert_eq!(snapshot.aggregate, Aggregate::Working);
        assert_eq!(snapshot.effect, Effect::Busy);
        assert_eq!(snapshot.sessions.len(), 1);
    }

    #[test]
    fn journal_records_event_transitions() {
        let state = state();
        state.apply_event(event(AgentState::Working)).unwrap();
        let entries = state.journal().entries(10);
        let last = entries.last().expect("event should be journaled");
        assert_eq!(last.kind, HookKind::Event);
        assert_eq!(last.agent.as_deref(), Some("test"));
        assert_eq!(last.session.as_deref(), Some("test:default"));
        assert_eq!(last.state, Some(AgentState::Working));
        assert_eq!(last.aggregate_before, Aggregate::None);
        assert_eq!(last.aggregate_after, Aggregate::Working);
        assert_eq!(last.effect_before, Effect::Off);
        assert_eq!(last.effect_after, Effect::Busy);
        assert_eq!(last.outcome, HookOutcome::Applied);
    }

    #[test]
    fn journal_records_rejected_events() {
        let state = state();
        let mut invalid = event(AgentState::Working);
        invalid.ttl_secs = Some(1);
        assert!(state.apply_event(invalid).is_err());
        let entries = state.journal().entries(10);
        let last = entries.last().expect("rejected event should be journaled");
        assert_eq!(last.kind, HookKind::Event);
        assert_eq!(last.outcome, HookOutcome::Rejected);
        assert!(last.error.as_deref().unwrap().contains("ttl_secs"));
        assert_eq!(last.aggregate_before, last.aggregate_after);
    }

    #[test]
    fn override_wins_until_cleared() {
        let state = state();
        state.apply_event(event(AgentState::Working)).unwrap();
        state.set_override(Effect::Alert, 30, None);
        assert_eq!(state.snapshot().effect, Effect::Alert);
        state.clear_override();
        assert_eq!(state.snapshot().effect, Effect::Busy);
    }

    #[test]
    fn device_override_only_targets_one_device() {
        let state = state();
        state.apply_event(event(AgentState::Working)).unwrap();
        state.register_device(DeviceInfo::new(
            "serial:abc",
            "/dev/fake",
            DeviceProfile::Mlight3Led,
        ));
        state.set_override(Effect::Alert, 30, Some("serial:abc".into()));
        let desired = state.subscribe_desired().borrow().clone();
        assert_eq!(desired.for_device("serial:abc").effect, Effect::Alert);
        assert_eq!(desired.for_device("serial:other").effect, Effect::Busy);
        assert_eq!(state.snapshot().effect, Effect::Busy);
    }

    #[test]
    fn auto_devices_get_independent_effects() {
        let mut config = Config::default();
        for serial in ["A", "B"] {
            config.devices.push(agent_beacon_core::DeviceBinding {
                serial: Some(serial.into()),
                port: None,
                id: None,
                profile: DeviceProfile::Mlight3Led,
                label: None,
                brightness: None,
                buzzer_on_alert: None,
            });
        }
        config.routing.strategy = agent_beacon_core::RoutingStrategy::AutoSession;
        let state = state_with(config);
        for serial in ["A", "B"] {
            let mut info = DeviceInfo::new(
                format!("serial:{serial}"),
                format!("/dev/{serial}"),
                DeviceProfile::Mlight3Led,
            );
            info.connected = true;
            info.last_seen_ms = Some(now_ms());
            state.register_device(info);
        }
        state
            .apply_event(Event {
                agent: "claude".into(),
                state: AgentState::Working,
                session: Some("claude:1".into()),
                message: None,
                ttl_secs: None,
            })
            .unwrap();
        state
            .apply_event(Event {
                agent: "codex".into(),
                state: AgentState::Error,
                session: Some("codex:1".into()),
                message: None,
                ttl_secs: None,
            })
            .unwrap();

        let snapshot = state.snapshot();
        let mut effects: Vec<Effect> = snapshot
            .routing
            .iter()
            .filter(|route| route.assignment == DeviceAssignment::Auto)
            .map(|route| route.effect)
            .collect();
        effects.sort_by_key(|effect| effect.as_str());
        assert_eq!(effects, vec![Effect::Alert, Effect::Busy]);
        assert!(snapshot.unassigned_agents.is_empty());
    }

    #[test]
    fn missing_device_beyond_grace_moves_its_sessions() {
        let mut config = Config::default();
        for serial in ["A", "B"] {
            config.devices.push(agent_beacon_core::DeviceBinding {
                serial: Some(serial.into()),
                port: None,
                id: None,
                profile: DeviceProfile::Mlight3Led,
                label: None,
                brightness: None,
                buzzer_on_alert: None,
            });
        }
        config.routing.strategy = agent_beacon_core::RoutingStrategy::AutoSession;
        let state = state_with(config);
        for serial in ["A", "B"] {
            let mut info = DeviceInfo::new(
                format!("serial:{serial}"),
                format!("/dev/{serial}"),
                DeviceProfile::Mlight3Led,
            );
            info.connected = true;
            info.last_seen_ms = Some(now_ms());
            state.register_device(info);
        }
        for index in 0..2 {
            state
                .apply_event(Event {
                    agent: "claude".into(),
                    state: AgentState::Working,
                    session: Some(format!("claude:{index}")),
                    message: None,
                    ttl_secs: None,
                })
                .unwrap();
        }
        let before = state.snapshot();
        assert_eq!(
            before
                .routing
                .iter()
                .filter(|r| !r.sessions.is_empty())
                .count(),
            2
        );

        // B 断线且超过 grace，只迁移 B 上的 session。
        let old = now_ms().saturating_sub(crate::routing::MISSING_GRACE_MS + 1);
        {
            let mut registry = state.devices().lock().unwrap();
            let info = registry.get_mut("serial:B").unwrap();
            info.connected = false;
            info.last_seen_ms = Some(old);
        }
        state.refresh();
        let after = state.snapshot();
        let a = after
            .routing
            .iter()
            .find(|r| r.device == "serial:A")
            .unwrap();
        let b = after
            .routing
            .iter()
            .find(|r| r.device == "serial:B")
            .unwrap();
        assert_eq!(a.sessions.len(), 2);
        assert!(b.sessions.is_empty());
    }

    #[test]
    fn tick_expires_override() {
        let state = state();
        state.set_override(Effect::Alert, 1, None);
        let inner_now = now_ms();
        {
            let mut inner = state.lock();
            inner.manual_override.as_mut().unwrap().expires_at_ms = inner_now - 1;
        }
        assert!(state.tick());
        assert!(state.snapshot().manual_override.is_none());
        let entries = state.journal().entries(10);
        let last = entries.last().expect("override expiry should be journaled");
        assert_eq!(last.kind, HookKind::OverrideExpired);
        assert_eq!(last.effect_before, Effect::Alert);
        assert_eq!(last.effect_after, Effect::Off);
    }
}
