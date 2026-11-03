//! 服务运行状态：会话、配置、手动覆盖、设备视图与 SSE 广播。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use agent_beacon_core::{
    now_ms, Aggregate, Config, Effect, Event, SessionRegistry, SessionView, ValidationError,
};
use agent_beacon_lights::device::{new_registry, snapshot as device_snapshot, DeviceRegistry};
use agent_beacon_lights::{DesiredState, DeviceInfo};
use serde::Serialize;
use tokio::sync::{broadcast, watch, Notify};

use crate::config_store::ConfigStore;
use crate::VERSION;

#[derive(Debug, Clone, Serialize)]
pub struct ServiceInfo {
    pub version: &'static str,
    pub pid: u32,
    pub port: u16,
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
    pub devices: Vec<DeviceInfo>,
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
}

pub struct BeaconState {
    inner: Mutex<Inner>,
    devices: DeviceRegistry,
    desired_tx: watch::Sender<DesiredState>,
    events: broadcast::Sender<Snapshot>,
    started: Instant,
    port: u16,
    store: ConfigStore,
    scan: Notify,
}

impl BeaconState {
    pub fn new(config: Config, store: ConfigStore, port: u16) -> Arc<Self> {
        let (desired_tx, _) = watch::channel(initial_desired(&config));
        let (events, _) = broadcast::channel(64);
        Arc::new(Self {
            inner: Mutex::new(Inner {
                config,
                sessions: SessionRegistry::with_default_ttl(),
                manual_override: None,
            }),
            devices: new_registry(),
            desired_tx,
            events,
            started: Instant::now(),
            port,
            store,
            scan: Notify::new(),
        })
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

    pub fn config(&self) -> Config {
        self.lock().config.clone()
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
            devices: device_snapshot(&self.devices),
            effect: effective_global(&inner, now),
            manual_override,
            service: ServiceInfo {
                version: VERSION,
                pid: std::process::id(),
                port: self.port,
                uptime_secs: self.started.elapsed().as_secs(),
            },
            config: inner.config.clone(),
        }
    }

    pub fn publish(&self) {
        let snapshot = self.snapshot();
        let _ = self.events.send(snapshot);
    }

    pub fn apply_event(&self, event: Event) -> Result<SessionView, ValidationError> {
        let view = {
            let mut inner = self.lock();
            let view = inner.sessions.apply(&event, now_ms())?;
            refresh_desired(&self.desired_tx, &inner);
            view
        };
        self.publish();
        Ok(view)
    }

    pub fn clear_sessions(&self) {
        {
            let mut inner = self.lock();
            inner.sessions.clear();
            refresh_desired(&self.desired_tx, &inner);
        }
        self.publish();
    }

    pub fn set_config(&self, config: Config) -> Result<(), String> {
        self.store.save(&config)?;
        {
            let mut inner = self.lock();
            inner.config = config;
            refresh_desired(&self.desired_tx, &inner);
        }
        self.publish();
        Ok(())
    }

    pub fn set_override(&self, effect: Effect, duration_secs: u64, device: Option<String>) {
        {
            let mut inner = self.lock();
            inner.manual_override = Some(ManualOverride {
                effect,
                expires_at_ms: now_ms().saturating_add(duration_secs.saturating_mul(1000)),
                device,
            });
            refresh_desired(&self.desired_tx, &inner);
        }
        self.publish();
    }

    pub fn clear_override(&self) {
        {
            let mut inner = self.lock();
            inner.manual_override = None;
            refresh_desired(&self.desired_tx, &inner);
        }
        self.publish();
    }

    /// 到期清理；有变化时返回 true 并已广播。
    pub fn tick(&self) -> bool {
        let changed = {
            let mut inner = self.lock();
            let now = now_ms();
            let mut changed = inner.sessions.expire(now);
            if let Some(item) = &inner.manual_override {
                if item.expires_at_ms <= now {
                    inner.manual_override = None;
                    changed = true;
                }
            }
            if changed {
                refresh_desired(&self.desired_tx, &inner);
            }
            changed
        };
        if changed {
            self.publish();
        }
        changed
    }

    /// 退出前把灯全部关掉（不依赖映射）。
    pub fn force_off(&self) {
        let _ = self.desired_tx.send_replace(DesiredState {
            effect: Effect::Off,
            ..DesiredState::default()
        });
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

fn initial_desired(config: &Config) -> DesiredState {
    DesiredState {
        effect: config.mapping.effect_for(Aggregate::None),
        overrides: HashMap::new(),
        brightness: config.mlight.brightness,
        buzzer_on_alert: config.yh50.buzzer_on_alert,
    }
}

fn effective_global(inner: &Inner, now: u64) -> Effect {
    if let Some(item) = &inner.manual_override {
        if item.expires_at_ms > now && item.device.is_none() {
            return item.effect;
        }
    }
    inner
        .config
        .mapping
        .effect_for(inner.sessions.aggregate_at(now))
}

fn refresh_desired(tx: &watch::Sender<DesiredState>, inner: &Inner) {
    let now = now_ms();
    let aggregate = inner.sessions.aggregate_at(now);
    let mut effect = inner.config.mapping.effect_for(aggregate);
    let mut overrides = HashMap::new();
    if let Some(item) = &inner.manual_override {
        if item.expires_at_ms > now {
            match &item.device {
                Some(device) => {
                    overrides.insert(device.clone(), item.effect);
                }
                None => effect = item.effect,
            }
        }
    }
    let _ = tx.send_replace(DesiredState {
        effect,
        overrides,
        brightness: inner.config.mlight.brightness,
        buzzer_on_alert: inner.config.yh50.buzzer_on_alert,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_beacon_core::{AgentState, Config};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn state() -> Arc<BeaconState> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            + u128::from(COUNTER.fetch_add(1, Ordering::Relaxed));
        let dir = std::env::temp_dir().join(format!("beacon-state-{unique}"));
        let store = ConfigStore::new(dir.join("config.json"));
        let config = Config::default();
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
        state.set_override(Effect::Alert, 30, Some("serial:abc".into()));
        let desired = state.subscribe_desired().borrow().clone();
        assert_eq!(desired.for_device("serial:abc"), Effect::Alert);
        assert_eq!(desired.for_device("serial:other"), Effect::Busy);
        assert_eq!(state.snapshot().effect, Effect::Busy);
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
    }
}
