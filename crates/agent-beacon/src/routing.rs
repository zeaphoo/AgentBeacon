//! 多设备分配：统一显示、按 Agent 自动/手动分配、按 Session 自动分配。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use agent_beacon_core::{
    Aggregate, Config, DeviceAssignment, DeviceBinding, DeviceProfile, Effect, RoutingStrategy,
    SessionView,
};
use agent_beacon_lights::device::{snapshot as device_snapshot, DeviceRegistry};
use agent_beacon_lights::DeviceInfo;
use serde::Serialize;

/// 短暂断线保留归属的时间；超过后自动设备上的 session 才会迁移。
pub const MISSING_GRACE_MS: u64 = 60_000;

/// 配置绑定 + 运行时设备状态的合并视图。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedDevice {
    pub id: String,
    pub port: String,
    pub profile: DeviceProfile,
    pub label: Option<String>,
    pub assignment: DeviceAssignment,
    pub connected: bool,
    pub last_seen_ms: Option<u64>,
    pub brightness: Option<u8>,
    pub buzzer_on_alert: Option<bool>,
    /// 是否有正在运行的设备 actor。
    pub registered: bool,
}

impl ResolvedDevice {
    /// 当前可用：已连接，或仍在断开宽限期内。
    pub fn available(&self, now_ms: u64) -> bool {
        self.connected
            || self
                .last_seen_ms
                .is_some_and(|last| now_ms.saturating_sub(last) <= MISSING_GRACE_MS)
    }
}

/// 一次分配的结果。
#[derive(Debug, Clone, Default)]
pub struct AssignmentResult {
    /// device id -> 分配给它的活动 session。
    pub by_device: HashMap<String, Vec<SessionView>>,
    /// 没有出现在任何设备 session 集合中的 agent。
    pub unassigned_agents: Vec<String>,
    /// 统一显示模式下，所有设备都直接显示全部会话。
    pub mirror_device_ids: BTreeSet<String>,
}

/// 粘性分配表：AutoSession 记 session -> device，AutoAgent 记 agent -> device。
#[derive(Debug, Default)]
pub struct AssignmentEngine {
    sessions: HashMap<(String, String), String>,
    agents: HashMap<String, String>,
    last_strategy: Option<RoutingStrategy>,
}

impl AssignmentEngine {
    pub fn clear(&mut self) {
        self.sessions.clear();
        self.agents.clear();
        self.last_strategy = None;
    }

    pub fn resolve(
        &mut self,
        sessions: &[SessionView],
        devices: &[ResolvedDevice],
        config: &Config,
        now_ms: u64,
    ) -> AssignmentResult {
        let strategy = config.routing.strategy;
        if self.last_strategy != Some(strategy) {
            self.clear();
            self.last_strategy = Some(strategy);
        }
        match strategy {
            RoutingStrategy::Unified => self.resolve_unified(devices),
            RoutingStrategy::AutoAgent => self.resolve_auto_agent(sessions, devices, now_ms),
            RoutingStrategy::ManualAgent => self.resolve_manual(sessions, devices, config),
            RoutingStrategy::AutoSession => self.resolve_auto_session(sessions, devices, now_ms),
        }
    }

    fn resolve_unified(&mut self, devices: &[ResolvedDevice]) -> AssignmentResult {
        self.sessions.clear();
        self.agents.clear();
        AssignmentResult {
            by_device: HashMap::new(),
            unassigned_agents: Vec::new(),
            mirror_device_ids: devices.iter().map(|device| device.id.clone()).collect(),
        }
    }

    fn resolve_auto_session(
        &mut self,
        sessions: &[SessionView],
        devices: &[ResolvedDevice],
        now_ms: u64,
    ) -> AssignmentResult {
        let active: HashSet<(String, String)> = sessions
            .iter()
            .map(|view| (view.agent.clone(), view.session.clone()))
            .collect();
        self.sessions.retain(|key, _| active.contains(key));
        let auto_ids = auto_device_ids(devices, now_ms);

        let mut pending = Vec::new();
        for view in sessions {
            let key = (view.agent.clone(), view.session.clone());
            match self.sessions.get(&key) {
                Some(device_id) if auto_ids.contains(device_id) => {}
                _ => pending.push(view.clone()),
            }
        }
        pending.sort_by(|a, b| {
            a.created_at_ms
                .cmp(&b.created_at_ms)
                .then_with(|| a.agent.cmp(&b.agent))
                .then_with(|| a.session.cmp(&b.session))
        });

        let mut loads = empty_loads(&auto_ids);
        for ((agent, session), device_id) in &self.sessions {
            if auto_ids.contains(device_id) && active.contains(&(agent.clone(), session.clone())) {
                *loads.entry(device_id.clone()).or_default() += 1;
            }
        }
        for view in pending {
            let key = (view.agent.clone(), view.session.clone());
            match least_loaded(&auto_ids, &loads) {
                Some(device_id) => {
                    *loads.entry(device_id.clone()).or_default() += 1;
                    self.sessions.insert(key, device_id);
                }
                None => {
                    self.sessions.remove(&key);
                }
            }
        }

        let (by_device, assigned_agents) = group_sessions(sessions, |view| {
            self.sessions
                .get(&(view.agent.clone(), view.session.clone()))
                .cloned()
        });
        AssignmentResult {
            by_device,
            unassigned_agents: unassigned(sessions, &assigned_agents),
            mirror_device_ids: BTreeSet::new(),
        }
    }

    fn resolve_auto_agent(
        &mut self,
        sessions: &[SessionView],
        devices: &[ResolvedDevice],
        now_ms: u64,
    ) -> AssignmentResult {
        let active_agents = active_agents(sessions);
        let auto_ids = auto_device_ids(devices, now_ms);
        self.agents
            .retain(|agent, device| active_agents.contains(agent) && auto_ids.contains(device));

        let mut pending: Vec<String> = active_agents
            .iter()
            .filter(|agent| !self.agents.contains_key(*agent))
            .cloned()
            .collect();
        pending.sort();

        let mut loads = empty_loads(&auto_ids);
        for (agent, device_id) in &self.agents {
            if active_agents.contains(agent) && auto_ids.contains(device_id) {
                *loads.entry(device_id.clone()).or_default() += 1;
            }
        }
        for agent in pending {
            if let Some(device_id) = least_loaded(&auto_ids, &loads) {
                *loads.entry(device_id.clone()).or_default() += 1;
                self.agents.insert(agent, device_id);
            }
        }
        self.agents
            .retain(|agent, device| active_agents.contains(agent) && auto_ids.contains(device));

        let (by_device, assigned_agents) =
            group_sessions(sessions, |view| self.agents.get(&view.agent).cloned());
        AssignmentResult {
            by_device,
            unassigned_agents: unassigned(sessions, &assigned_agents),
            mirror_device_ids: BTreeSet::new(),
        }
    }

    fn resolve_manual(
        &mut self,
        sessions: &[SessionView],
        devices: &[ResolvedDevice],
        config: &Config,
    ) -> AssignmentResult {
        self.sessions.clear();
        self.agents.clear();

        let known: HashSet<&str> = devices.iter().map(|device| device.id.as_str()).collect();
        let mut agent_device: BTreeMap<String, String> = BTreeMap::new();
        let mut device_ids: Vec<&String> = config.routing.manual.keys().collect();
        device_ids.sort();
        for device_id in device_ids {
            if !known.contains(device_id.as_str()) {
                continue;
            }
            for agent in config.routing.manual.get(device_id).into_iter().flatten() {
                let agent = agent.trim();
                if !agent.is_empty() {
                    agent_device
                        .entry(agent.to_string())
                        .or_insert_with(|| device_id.clone());
                }
            }
        }

        let (by_device, assigned_agents) =
            group_sessions(sessions, |view| agent_device.get(&view.agent).cloned());
        AssignmentResult {
            by_device,
            unassigned_agents: unassigned(sessions, &assigned_agents),
            mirror_device_ids: BTreeSet::new(),
        }
    }
}

fn auto_device_ids(devices: &[ResolvedDevice], now_ms: u64) -> BTreeSet<String> {
    devices
        .iter()
        .filter(|device| device.assignment == DeviceAssignment::Auto && device.available(now_ms))
        .map(|device| device.id.clone())
        .collect()
}

fn empty_loads(auto_ids: &BTreeSet<String>) -> HashMap<String, usize> {
    auto_ids
        .iter()
        .map(|device_id| (device_id.clone(), 0))
        .collect()
}

fn least_loaded(auto_ids: &BTreeSet<String>, loads: &HashMap<String, usize>) -> Option<String> {
    auto_ids
        .iter()
        .min_by_key(|device_id| {
            (
                loads.get(*device_id).copied().unwrap_or_default(),
                device_id.as_str(),
            )
        })
        .cloned()
}

fn active_agents(sessions: &[SessionView]) -> BTreeSet<String> {
    sessions.iter().map(|view| view.agent.clone()).collect()
}

fn unassigned(sessions: &[SessionView], assigned_agents: &HashSet<String>) -> Vec<String> {
    let mut agents: Vec<String> = sessions
        .iter()
        .map(|view| view.agent.clone())
        .filter(|agent| !assigned_agents.contains(agent))
        .collect();
    agents.sort();
    agents.dedup();
    agents
}

fn group_sessions(
    sessions: &[SessionView],
    device_for: impl Fn(&SessionView) -> Option<String>,
) -> (HashMap<String, Vec<SessionView>>, HashSet<String>) {
    let mut by_device: HashMap<String, Vec<SessionView>> = HashMap::new();
    let mut assigned_agents = HashSet::new();
    for view in sessions {
        let Some(device_id) = device_for(view) else {
            continue;
        };
        by_device.entry(device_id).or_default().push(view.clone());
        assigned_agents.insert(view.agent.clone());
    }
    for views in by_device.values_mut() {
        views.sort_by_key(|view| std::cmp::Reverse(view.updated_at_ms));
    }
    (by_device, assigned_agents)
}

/// 把配置绑定与运行时设备合并；配置了但尚未发现的设备也会出现在结果里。
pub fn resolve_devices(config: &Config, registry: &DeviceRegistry) -> Vec<ResolvedDevice> {
    let infos = device_snapshot(registry);
    let assignment = role_for(config.routing.strategy);
    let mut resolved: BTreeMap<String, ResolvedDevice> = BTreeMap::new();
    let mut matched_bindings = vec![false; config.devices.len()];

    for info in &infos {
        let binding_index = config
            .devices
            .iter()
            .position(|binding| binding_matches(binding, info));
        if let Some(index) = binding_index {
            matched_bindings[index] = true;
        }
        let binding = binding_index.map(|index| &config.devices[index]);
        let device = resolve_one(info, binding, assignment);
        resolved.insert(device.id.clone(), device);
    }

    for (index, binding) in config.devices.iter().enumerate() {
        if matched_bindings[index] || infos.iter().any(|info| binding_matches(binding, info)) {
            continue;
        }
        let Some(id) = binding_device_id(binding) else {
            continue;
        };
        if resolved.contains_key(&id) {
            continue;
        }
        let port = binding
            .port
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| id.clone());
        resolved.insert(
            id.clone(),
            ResolvedDevice {
                id,
                port,
                profile: binding.profile,
                label: binding.label.clone(),
                assignment,
                connected: false,
                last_seen_ms: None,
                brightness: binding.brightness,
                buzzer_on_alert: binding.buzzer_on_alert,
                registered: false,
            },
        );
    }

    resolved.into_values().collect()
}

fn role_for(strategy: RoutingStrategy) -> DeviceAssignment {
    match strategy {
        RoutingStrategy::Unified => DeviceAssignment::Mirror,
        RoutingStrategy::AutoAgent | RoutingStrategy::AutoSession => DeviceAssignment::Auto,
        RoutingStrategy::ManualAgent => DeviceAssignment::Manual,
    }
}

fn resolve_one(
    info: &DeviceInfo,
    binding: Option<&DeviceBinding>,
    assignment: DeviceAssignment,
) -> ResolvedDevice {
    ResolvedDevice {
        id: info.id.clone(),
        port: info.port.clone(),
        profile: binding
            .map(|binding| binding.profile)
            .unwrap_or(info.profile),
        label: binding.and_then(|binding| binding.label.clone()),
        assignment,
        connected: info.connected,
        last_seen_ms: info.last_seen_ms,
        brightness: binding.and_then(|binding| binding.brightness),
        buzzer_on_alert: binding.and_then(|binding| binding.buzzer_on_alert),
        registered: true,
    }
}

/// 按设备 id/端口查找配置绑定，供 worker 设置展示标签。
pub fn binding_for_device<'a>(
    config: &'a Config,
    id: &str,
    port: &str,
) -> Option<&'a DeviceBinding> {
    config.devices.iter().find(|binding| {
        trimmed(&binding.id).is_some_and(|value| value == id)
            || trimmed(&binding.serial).is_some_and(|value| id == format!("serial:{value}"))
            || trimmed(&binding.port).is_some_and(|value| value == port)
    })
}

fn binding_matches(binding: &DeviceBinding, info: &DeviceInfo) -> bool {
    if let Some(id) = trimmed(&binding.id) {
        if id == info.id {
            return true;
        }
    }
    if let Some(serial) = trimmed(&binding.serial) {
        if info.id == format!("serial:{serial}") {
            return true;
        }
    }
    if let Some(port) = trimmed(&binding.port) {
        if port == info.port {
            return true;
        }
    }
    false
}

fn binding_device_id(binding: &DeviceBinding) -> Option<String> {
    if let Some(id) = trimmed(&binding.id) {
        return Some(id.to_string());
    }
    if let Some(serial) = trimmed(&binding.serial) {
        return Some(format!("serial:{serial}"));
    }
    if let Some(port) = trimmed(&binding.port) {
        return Some(format!("port:{port}"));
    }
    None
}

fn trimmed(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// 空设备使用的灯效：跟随 `mapping.none`（默认 off）。
pub fn empty_effect(config: &Config) -> Effect {
    config.mapping.effect_for(Aggregate::None)
}

/// 服务层对外暴露的路由视图。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceRouteView {
    pub device: String,
    pub label: Option<String>,
    pub assignment: DeviceAssignment,
    pub agents: Vec<String>,
    pub sessions: Vec<SessionView>,
    pub aggregate: Aggregate,
    pub auto_effect: Effect,
    pub effect: Effect,
    pub overridden: bool,
    pub connected: bool,
    pub last_seen_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_beacon_core::{AgentState, SessionView};
    use agent_beacon_lights::device::{new_registry, upsert};
    use agent_beacon_lights::DeviceInfo;

    fn session(agent: &str, session: &str, created: u64, state: AgentState) -> SessionView {
        SessionView {
            agent: agent.into(),
            session: session.into(),
            state,
            message: None,
            created_at_ms: created,
            updated_at_ms: created,
            expires_at_ms: u64::MAX,
        }
    }

    fn device(id: &str, connected: bool) -> ResolvedDevice {
        ResolvedDevice {
            id: id.into(),
            port: id.into(),
            profile: DeviceProfile::Mlight3Led,
            label: None,
            assignment: DeviceAssignment::Auto,
            connected,
            last_seen_ms: connected.then_some(0),
            brightness: None,
            buzzer_on_alert: None,
            registered: true,
        }
    }

    fn devices() -> Vec<ResolvedDevice> {
        vec![device("A", true), device("B", true), device("C", true)]
    }

    fn config(strategy: RoutingStrategy) -> Config {
        let mut config = Config::default();
        config.routing.strategy = strategy;
        config
    }

    #[test]
    fn unified_marks_every_device_as_mirror() {
        let mut engine = AssignmentEngine::default();
        let result = engine.resolve(
            &[session("claude", "s1", 1, AgentState::Working)],
            &devices(),
            &config(RoutingStrategy::Unified),
            10,
        );
        assert_eq!(result.mirror_device_ids.len(), 3);
        assert!(result.by_device.is_empty());
        assert!(result.unassigned_agents.is_empty());
    }

    #[test]
    fn auto_session_splits_sessions_evenly_and_sticks() {
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let sessions = [
            session("claude", "s1", 1, AgentState::Working),
            session("claude", "s2", 2, AgentState::Working),
            session("claude", "s3", 3, AgentState::Waiting),
        ];
        let result = engine.resolve(&sessions, &devices(), &config, 10);
        assert_eq!(result.by_device.get("A").unwrap().len(), 1);
        assert_eq!(result.by_device.get("B").unwrap().len(), 1);
        assert_eq!(result.by_device.get("C").unwrap().len(), 1);

        let mut sessions = sessions.to_vec();
        sessions.push(session("claude", "s4", 4, AgentState::Idle));
        let result = engine.resolve(&sessions, &devices(), &config, 11);
        assert_eq!(result.by_device.get("A").unwrap().len(), 2);
        assert_eq!(result.by_device.get("B").unwrap().len(), 1);
        assert_eq!(result.by_device.get("C").unwrap().len(), 1);
    }

    #[test]
    fn auto_agent_keeps_one_agent_on_one_light() {
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoAgent);
        let sessions = [
            session("claude", "s1", 1, AgentState::Working),
            session("claude", "s2", 2, AgentState::Working),
            session("codex", "c1", 3, AgentState::Error),
        ];
        let result = engine.resolve(&sessions, &devices(), &config, 10);
        let claude_device = result
            .by_device
            .iter()
            .find(|(_, views)| views.iter().any(|view| view.agent == "claude"))
            .map(|(device, _)| device.clone())
            .unwrap();
        let claude_views = result.by_device.get(&claude_device).unwrap();
        assert_eq!(
            claude_views
                .iter()
                .filter(|view| view.agent == "claude")
                .count(),
            2
        );
        let codex_device = result
            .by_device
            .iter()
            .find(|(_, views)| views.iter().any(|view| view.agent == "codex"))
            .map(|(device, _)| device.clone())
            .unwrap();
        assert_ne!(claude_device, codex_device);
    }

    #[test]
    fn manual_assigns_agents_to_lights_and_ignores_stale_devices() {
        let mut engine = AssignmentEngine::default();
        let mut config = config(RoutingStrategy::ManualAgent);
        config
            .routing
            .manual
            .insert("A".into(), vec!["claude".into(), "codex".into()]);
        config
            .routing
            .manual
            .insert("stale".into(), vec!["pi".into()]);
        let sessions = [
            session("claude", "s1", 1, AgentState::Working),
            session("codex", "c1", 2, AgentState::Error),
            session("pi", "p1", 3, AgentState::Waiting),
        ];
        let result = engine.resolve(&sessions, &devices(), &config, 10);
        assert_eq!(result.by_device.get("A").unwrap().len(), 2);
        assert_eq!(result.unassigned_agents, vec!["pi"]);
    }

    #[test]
    fn long_offline_device_is_excluded_and_sessions_move() {
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: Some(0),
                ..device("B", true)
            },
        ];
        let sessions = [
            session("claude", "s1", 1, AgentState::Working),
            session("claude", "s2", 2, AgentState::Working),
        ];
        let result = engine.resolve(&sessions, &devices, &config, 10);
        assert_eq!(result.by_device.get("A").unwrap().len(), 1);
        assert_eq!(result.by_device.get("B").unwrap().len(), 1);

        devices[1].last_seen_ms = Some(0);
        let result = engine.resolve(&sessions, &devices, &config, MISSING_GRACE_MS + 1);
        assert_eq!(result.by_device.get("A").unwrap().len(), 2);
        assert!(!result.by_device.contains_key("B"));
    }

    #[test]
    fn auto_agent_moves_agents_off_an_offline_light() {
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoAgent);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: Some(0),
                ..device("B", true)
            },
        ];
        let sessions = [
            session("claude", "s1", 1, AgentState::Working),
            session("codex", "s2", 2, AgentState::Working),
        ];
        let result = engine.resolve(&sessions, &devices, &config, 10);
        assert_eq!(result.by_device.get("A").unwrap().len(), 1);
        assert_eq!(result.by_device.get("B").unwrap().len(), 1);

        devices[1].last_seen_ms = Some(0);
        let result = engine.resolve(&sessions, &devices, &config, MISSING_GRACE_MS + 1);
        assert_eq!(result.by_device.get("A").unwrap().len(), 2);
        assert!(!result.by_device.contains_key("B"));
    }

    #[test]
    fn resolve_devices_applies_strategy_role() {
        let registry = new_registry();
        let mut info = DeviceInfo::new("serial:ABC", "/dev/ttyACM0", DeviceProfile::Mlight3Led);
        info.connected = true;
        info.last_seen_ms = Some(123);
        upsert(&registry, info);

        let mut config = Config::default();
        config.devices.push(DeviceBinding {
            serial: Some("ABC".into()),
            port: None,
            id: None,
            profile: DeviceProfile::Mlight3Led,
            label: Some("左灯".into()),
            brightness: Some(80),
            buzzer_on_alert: None,
        });

        config.routing.strategy = RoutingStrategy::Unified;
        assert_eq!(
            resolve_devices(&config, &registry)[0].assignment,
            DeviceAssignment::Mirror
        );
        config.routing.strategy = RoutingStrategy::AutoAgent;
        assert_eq!(
            resolve_devices(&config, &registry)[0].assignment,
            DeviceAssignment::Auto
        );
        config.routing.strategy = RoutingStrategy::ManualAgent;
        assert_eq!(
            resolve_devices(&config, &registry)[0].assignment,
            DeviceAssignment::Manual
        );
        assert_eq!(
            resolve_devices(&config, &registry)[0].label.as_deref(),
            Some("左灯")
        );
        assert_eq!(resolve_devices(&config, &registry)[0].brightness, Some(80));
    }
}
