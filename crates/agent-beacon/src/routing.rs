//! 多设备分配：统一显示、按 Agent 自动/手动分配、按 Session 自动分配。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use agent_beacon_core::{
    AgentState, Aggregate, Config, DeviceAssignment, DeviceBinding, DeviceProfile, Effect,
    RoutingStrategy, SessionView,
};
use agent_beacon_lights::device::{snapshot as device_snapshot, DeviceRegistry};
use agent_beacon_lights::DeviceInfo;
use serde::Serialize;

/// 短暂断线保留归属的时间；超过后自动设备上的 session 才会迁移。
///
/// 只用于**保留**已有归属：设备心跳是 5 秒一次、重连退避 1–8 秒，15 秒足够覆盖一次
/// 瞬时掉线；更长的窗口会让“灯不亮却还占着会话”拖很久。**新的分配不会给未连接的设备**
/// （见 [`connected_auto_ids`]）。
pub const MISSING_GRACE_MS: u64 = 15_000;

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

        let targets = connected_auto_ids(devices);
        let views: HashMap<(&str, &str), &SessionView> = sessions
            .iter()
            .map(|view| ((view.agent.as_str(), view.session.as_str()), view))
            .collect();
        let mut loads = empty_loads(&auto_ids);
        for view in sessions {
            let key = (view.agent.clone(), view.session.clone());
            if let Some(device_id) = self.sessions.get(&key) {
                if auto_ids.contains(device_id) {
                    loads.entry(device_id.clone()).or_default().add(view.state);
                }
            }
        }
        for view in pending {
            let key = (view.agent.clone(), view.session.clone());
            match least_loaded(&targets, &loads) {
                Some(device_id) => {
                    loads
                        .entry(device_id.clone())
                        .or_default()
                        .add(view.state);
                    self.sessions.insert(key, device_id);
                }
                None => {
                    self.sessions.remove(&key);
                }
            }
        }
        // 灯后来上线时把堆积的会话摊开（否则会一直“两条会话挤一台灯、其它两台空着”）。
        //
        // 顺序很重要：先按重要性等级摊（`error` → `waiting` → `working`），再按 session
        // 数摊平。反过来的话摊平会优先搬 `idle` 的会话，结果就是“两条 working 挤在同一
        // 盏灯、另外两盏灯拿着 idle 显示空闲”，看上去就像有灯没被用上。
        spread_by_priority(
            &mut self.sessions,
            sessions.len(),
            &targets,
            &mut loads,
            |key| session_priority(&views, key),
        );
        rebalance_sessions(&mut self.sessions, &views, &targets, &mut loads);

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

        let targets = connected_auto_ids(devices);
        let states = agent_states(sessions);
        let mut loads = empty_loads(&auto_ids);
        for (agent, device_id) in &self.agents {
            if auto_ids.contains(device_id) {
                if let Some(priority) = states.get(agent.as_str()) {
                    loads.entry(device_id.clone()).or_default().add(priority.0);
                }
            }
        }
        for agent in pending {
            if let Some(device_id) = least_loaded(&targets, &loads) {
                let state = states
                    .get(agent.as_str())
                    .map(|priority| priority.0)
                    .unwrap_or(AgentState::Idle);
                loads.entry(device_id.clone()).or_default().add(state);
                self.agents.insert(agent, device_id);
            }
        }
        // 灯后来上线时，先把重要等级高的 agent（`error` → `waiting` → `working`）摊开，
        // 再把 agent 数摊平。
        spread_by_priority(
            &mut self.agents,
            states.len(),
            &targets,
            &mut loads,
            |agent| {
                states
                    .get(agent.as_str())
                    .copied()
                    .unwrap_or((AgentState::Idle, 0))
            },
        );
        rebalance_agents(&mut self.agents, &states, &targets, &mut loads);
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

/// 能接新分配的自动设备：只有**当前已连接**的才算。
///
/// 掉线宽限期（[`MISSING_GRACE_MS`]）只用来保留已有归属，避免瞬时掉线就把会话搬走；
/// 但把新会话分给一台连不上的灯，只会让灯不亮、用户以为分配没生效。
fn connected_auto_ids(devices: &[ResolvedDevice]) -> BTreeSet<String> {
    devices
        .iter()
        .filter(|device| device.assignment == DeviceAssignment::Auto && device.connected)
        .map(|device| device.id.clone())
        .collect()
}

/// 一台灯的“负载”：按重要性分级的计数，按字段顺序做字典序比较。
///
/// 字段顺序就是优先级顺序：`error` 最该分散，其次 `waiting`，然后才是活动会话数与
/// 会话总数。前面的字段决定两台灯谁更“空”，后面的只在前面打平时才起作用。
///
/// 注意：派生出来的 `Ord` 是**先比靠前的字段**，所以 `(0 error, 6 total)` 比
/// `(1 error, 1 total)` 更“空”——这正是想要的效果：已经报了错的灯不要再接新会话。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Load {
    error: usize,
    waiting: usize,
    active: usize,
    total: usize,
}

impl Load {
    fn add(&mut self, state: AgentState) {
        self.total += 1;
        if state.rank() > 0 {
            self.active += 1;
        }
        match state {
            AgentState::Error => self.error += 1,
            AgentState::Waiting => self.waiting += 1,
            AgentState::Idle | AgentState::Working => {}
        }
    }

    fn remove(&mut self, state: AgentState) {
        self.total = self.total.saturating_sub(1);
        if state.rank() > 0 {
            self.active = self.active.saturating_sub(1);
        }
        match state {
            AgentState::Error => self.error = self.error.saturating_sub(1),
            AgentState::Waiting => self.waiting = self.waiting.saturating_sub(1),
            AgentState::Idle | AgentState::Working => {}
        }
    }

    /// 该等级的单元数，用来判断“同一等级是否挤在一起”。
    fn count(&self, state: AgentState) -> usize {
        match state {
            AgentState::Error => self.error,
            AgentState::Waiting => self.waiting,
            AgentState::Working => self.active.saturating_sub(self.error + self.waiting),
            AgentState::Idle => self
                .total
                .saturating_sub(self.active),
        }
    }
}

/// 把负载最高的在线设备上的一条“单元”挪给最闲的在线设备，直到最大/最小会话数差 < 2。
///
/// 归属本身是粘性的（不轻易搬家，避免灯闪），但只有一台灯在线时所有会话都会堆到它
/// 身上；灯陆续上线后如果不重新平衡，就会一直是一台灯满、其它灯空着。
///
/// * 只在**当前已连接**的自动设备之间平衡；
/// * 只有 `max - min >= 2` 才动手：差 1 已经是“再搬就会来回跳”的状态；
/// * 每次搬 `priority` 最小（最不重要、最久没更新）的那个单元。
fn rebalance_device_load<K>(
    map: &mut HashMap<K, String>,
    targets: &BTreeSet<String>,
    loads: &mut HashMap<String, Load>,
    units: usize,
    priority: impl Fn(&K) -> (AgentState, u64),
) where
    K: std::hash::Hash + Eq + Clone,
{
    if targets.len() < 2 {
        return;
    }
    for _ in 0..units {
        let mut ranking: Vec<(&String, usize)> = targets
            .iter()
            .map(|id| (id, loads.get(id).map(|load| load.total).unwrap_or_default()))
            .collect();
        ranking.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));
        let (Some(&(least_ref, min)), Some(&(most_ref, max))) = (ranking.first(), ranking.last())
        else {
            return;
        };
        let (least, most) = (least_ref.clone(), most_ref.clone());
        if most == least || max.saturating_sub(min) < 2 {
            return;
        }
        let victim = map
            .iter()
            .filter(|(_, device)| **device == most)
            .map(|(key, _)| (key.clone(), priority(key)))
            .min_by_key(|(_, (state, updated_at))| (state.rank(), *updated_at))
            .map(|(key, _)| key);
        let Some(victim) = victim else {
            return;
        };
        let state = priority(&victim).0;
        map.insert(victim, least.clone());
        if let Some(load) = loads.get_mut(&most) {
            load.remove(state);
        }
        loads.entry(least).or_default().add(state);
    }
}

/// 按重要性等级把“同一等级挤在一起”的单元摊到不同灯上。
///
/// 按 `error` → `waiting` → `working` 的顺序处理：只要有两盏在线灯在同一等级上的单元数
/// 差 ≥ 2，就把多的一盏上该等级里最久没更新的一个搬到少的一盏；每次搬动都会把这个差
/// 缩小 2，所以搬完就停，不会在两盏灯之间来回跳。
///
/// `loads` 必须已经统计过 `map` 里所有属于 `targets` 的单元。
fn spread_by_priority<K>(
    map: &mut HashMap<K, String>,
    units: usize,
    targets: &BTreeSet<String>,
    loads: &mut HashMap<String, Load>,
    priority: impl Fn(&K) -> (AgentState, u64),
) where
    K: std::hash::Hash + Eq + Clone,
{
    if targets.len() < 2 {
        return;
    }
    // 每台在线灯上按等级分组的单元。
    let mut groups: HashMap<String, [Vec<K>; 4]> = targets
        .iter()
        .map(|id| (id.clone(), std::array::from_fn(|_| Vec::new())))
        .collect();
    for (key, device) in map.iter() {
        let Some(group) = groups.get_mut(device) else {
            continue;
        };
        group[priority(key).0.rank() as usize].push(key.clone());
    }

    let mut budget = units;
    for state in [AgentState::Error, AgentState::Waiting, AgentState::Working] {
        let rank = state.rank() as usize;
        while budget > 0 {
            let count_on = |id: &String| {
                loads
                    .get(id)
                    .map(|load| load.count(state))
                    .unwrap_or_default()
            };
            let most = targets
                .iter()
                .max_by(|a, b| {
                    count_on(a)
                        .cmp(&count_on(b))
                        .then_with(|| loads.get(*a).cmp(&loads.get(*b)))
                        .then_with(|| b.cmp(a))
                })
                .cloned();
            let least = targets
                .iter()
                .min_by(|a, b| {
                    count_on(a)
                        .cmp(&count_on(b))
                        .then_with(|| loads.get(*a).cmp(&loads.get(*b)))
                        .then_with(|| a.cmp(b))
                })
                .cloned();
            let (Some(most), Some(least)) = (most, least) else {
                return;
            };
            if most == least || count_on(&most).saturating_sub(count_on(&least)) < 2 {
                break;
            }
            let Some(group) = groups.get_mut(&most) else {
                break;
            };
            let Some(index) = group[rank]
                .iter()
                .enumerate()
                .min_by_key(|(_, key)| priority(key).1)
                .map(|(index, _)| index)
            else {
                break;
            };
            let victim = group[rank].swap_remove(index);
            map.insert(victim.clone(), least.clone());
            if let Some(load) = loads.get_mut(&most) {
                load.remove(state);
            }
            loads.entry(least.clone()).or_default().add(state);
            if let Some(group) = groups.get_mut(&least) {
                group[rank].push(victim);
            }
            budget -= 1;
        }
    }
}

/// 会话层面按数量重新平衡：优先搬 `idle` 的会话，其次搬最久没更新的。
fn rebalance_sessions(
    map: &mut HashMap<(String, String), String>,
    views: &HashMap<(&str, &str), &SessionView>,
    targets: &BTreeSet<String>,
    loads: &mut HashMap<String, Load>,
) {
    rebalance_device_load(map, targets, loads, views.len(), |key| {
        session_priority(views, key)
    });
}

/// 某条会话的排序键：等级越低、越久没更新，越先被搬走。
fn session_priority(
    views: &HashMap<(&str, &str), &SessionView>,
    key: &(String, String),
) -> (AgentState, u64) {
    views
        .get(&(key.0.as_str(), key.1.as_str()))
        .map(|view| (view.state, view.updated_at_ms))
        // 理论上不会命中（map 里的 key 都来自活动会话）。
        .unwrap_or((AgentState::Idle, 0))
}

/// 每个 agent 的整体状态（所有 session 里等级最高的）与最近更新时间。
fn agent_states(sessions: &[SessionView]) -> HashMap<String, (AgentState, u64)> {
    let mut states: HashMap<String, (AgentState, u64)> = HashMap::new();
    for view in sessions {
        let entry = states
            .entry(view.agent.clone())
            .or_insert((view.state, view.updated_at_ms));
        if view.state.rank() > entry.0.rank() {
            entry.0 = view.state;
        }
        entry.1 = entry.1.max(view.updated_at_ms);
    }
    states
}

/// Agent 层面按数量重新平衡：整体状态最不重要的先搬（全是 idle 的优先），
/// 同等重要时搬最久没有更新的。
fn rebalance_agents(
    map: &mut HashMap<String, String>,
    states: &HashMap<String, (AgentState, u64)>,
    targets: &BTreeSet<String>,
    loads: &mut HashMap<String, Load>,
) {
    rebalance_device_load(map, targets, loads, states.len(), |agent| {
        states
            .get(agent.as_str())
            .copied()
            .unwrap_or((AgentState::Idle, 0))
    });
}

fn empty_loads(auto_ids: &BTreeSet<String>) -> HashMap<String, Load> {
    auto_ids
        .iter()
        .map(|device_id| (device_id.clone(), Load::default()))
        .collect()
}

/// 选负载最小的灯：先比 `error`，再比 `waiting`、活动会话数、会话总数，最后按设备 id。
fn least_loaded(auto_ids: &BTreeSet<String>, loads: &HashMap<String, Load>) -> Option<String> {
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
    fn sessions_rebalance_when_more_lights_come_online() {
        // A 在线、B/C 还没连上：两条会话只能都落在 A。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("B", true)
            },
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("C", true)
            },
        ];
        let sessions = [
            session("pi", "s1", 1, AgentState::Working),
            session("pi", "s2", 2, AgentState::Idle),
        ];
        let result = engine.resolve(&sessions, &devices, &config, 10);
        assert_eq!(result.by_device.get("A").unwrap().len(), 2);

        // B/C 上线后要重新平衡：不应继续把两条会话都挤在 A 上。
        devices[1] = device("B", true);
        devices[2] = device("C", true);
        let result = engine.resolve(&sessions, &devices, &config, 20);
        assert_eq!(result.by_device.get("A").unwrap().len(), 1);
        assert_eq!(result.by_device.get("B").unwrap().len(), 1);
    }

    #[test]
    fn agents_rebalance_when_more_lights_come_online() {
        // A 在线、B/C 还没连上：两个 agent 都落到 A。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoAgent);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("B", true)
            },
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("C", true)
            },
        ];
        let sessions = [
            session("claude", "s1", 1, AgentState::Working),
            session("codex", "s2", 2, AgentState::Working),
        ];
        let result = engine.resolve(&sessions, &devices, &config, 10);
        assert_eq!(result.by_device.get("A").unwrap().len(), 2);

        // 灯上线后两个 agent 应分摊到两台灯上。
        devices[1] = device("B", true);
        devices[2] = device("C", true);
        let result = engine.resolve(&sessions, &devices, &config, 20);
        assert_eq!(result.by_device.get("A").unwrap().len(), 1);
        assert_eq!(result.by_device.get("B").unwrap().len(), 1);
    }

    #[test]
    fn agents_rebalance_moves_the_idle_agent_first() {
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoAgent);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("B", true)
            },
        ];
        let sessions = [
            session("busy", "s1", 1, AgentState::Working),
            session("idle", "s2", 2, AgentState::Idle),
        ];
        engine.resolve(&sessions, &devices, &config, 10);

        devices[1] = device("B", true);
        let result = engine.resolve(&sessions, &devices, &config, 20);
        let moved = &result.by_device["B"];
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].agent, "idle", "先搬 idle 的 agent");
        assert_eq!(&result.by_device["A"][0].agent, "busy");
    }

    #[test]
    fn rebalance_moves_the_idle_session_first() {
        // A 上同时有 working 和 idle 两条，B 空闲：搬走 idle 的那条更不打扰用户。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("B", true)
            },
        ];
        let sessions = [
            session("pi", "busy", 1, AgentState::Working),
            session("pi", "idle", 2, AgentState::Idle),
        ];
        engine.resolve(&sessions, &devices, &config, 10);

        devices[1] = device("B", true);
        let result = engine.resolve(&sessions, &devices, &config, 20);
        let moved = &result.by_device["B"];
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].session, "idle");
        assert_eq!(&result.by_device["A"][0].session, "busy");
    }

    #[test]
    fn running_sessions_get_a_lamp_each_instead_of_piling_on_one() {
        // 截图里的情况：5 条 session 全落在当时唯一在线的 A 上（2 条 working + 3 条 idle），
        // B/C 上线后如果只按数量摊平，会把 idle 先搬走，两条 working 仍然挤在 A 上，
        // B/C 只显示“空闲”——看上去就像两盏灯没被用上。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("B", true)
            },
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("C", true)
            },
        ];
        let sessions = [
            session("pi", "w1", 1, AgentState::Working),
            session("pi", "w2", 2, AgentState::Working),
            session("pi", "i1", 3, AgentState::Idle),
            session("pi", "i2", 4, AgentState::Idle),
            session("pi", "i3", 5, AgentState::Idle),
        ];
        let result = engine.resolve(&sessions, &devices, &config, 10);
        assert_eq!(result.by_device["A"].len(), 5);

        devices[1] = device("B", true);
        devices[2] = device("C", true);
        let result = engine.resolve(&sessions, &devices, &config, 20);

        let lamps_with_work: Vec<&String> = result
            .by_device
            .iter()
            .filter(|(_, views)| views.iter().any(|view| view.state != AgentState::Idle))
            .map(|(device, _)| device)
            .collect();
        assert_eq!(lamps_with_work.len(), 2, "两条 working 应各占一盏灯");
        assert_eq!(result.by_device.values().map(Vec::len).sum::<usize>(), 5);
        let loads: Vec<usize> = ["A", "B", "C"]
            .iter()
            .map(|device| result.by_device[*device].len())
            .collect();
        assert_eq!(loads, vec![2, 2, 1], "数量仍然摊平");
    }

    #[test]
    fn spreading_does_not_bounce_between_lamps() {
        // 摊平后两盏灯各一条 working、剩下一盏拿着 idle：再来几轮也不应搬动。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("B", true)
            },
        ];
        let sessions = [
            session("pi", "w1", 1, AgentState::Working),
            session("pi", "w2", 2, AgentState::Working),
            session("pi", "i1", 3, AgentState::Idle),
        ];
        engine.resolve(&sessions, &devices, &config, 10);
        devices[1] = device("B", true);
        let once = engine.resolve(&sessions, &devices, &config, 20);
        let twice = engine.resolve(&sessions, &devices, &config, 21);
        let thrice = engine.resolve(&sessions, &devices, &config, 22);
        assert_eq!(once.by_device, twice.by_device);
        assert_eq!(twice.by_device, thrice.by_device);
    }

    /// 某台灯上所有 session 里等级最高的那个（灯实际显示的状态）。
    fn lamp_state(result: &AssignmentResult, device: &str) -> Option<AgentState> {
        result
            .by_device
            .get(device)?
            .iter()
            .map(|view| view.state)
            .max_by_key(|state| state.rank())
    }

    #[test]
    fn errors_get_a_lamp_each_before_working_sessions_do() {
        // 只按数量摊平时，两条 error 会分在同一盏灯（总数 2/1/1 就“平衡”了）。
        // A 在线时三条 session 全落在 A，B/C 上线后：两条 error 应各占一盏灯。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("B", true)
            },
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("C", true)
            },
        ];
        let sessions = [
            session("pi", "e1", 1, AgentState::Error),
            session("pi", "e2", 2, AgentState::Error),
            session("pi", "w1", 3, AgentState::Working),
        ];
        engine.resolve(&sessions, &devices, &config, 10);

        devices[1] = device("B", true);
        devices[2] = device("C", true);
        let result = engine.resolve(&sessions, &devices, &config, 20);

        let error_lamps: Vec<&String> = result
            .by_device
            .iter()
            .filter(|(_, views)| views.iter().any(|view| view.state == AgentState::Error))
            .map(|(device, _)| device)
            .collect();
        assert_eq!(error_lamps.len(), 2, "两条 error 应各占一盏灯");
        assert!(result
            .by_device
            .values()
            .all(|views| views.len() == 1));
    }

    #[test]
    fn waiting_keeps_its_lamp_and_working_moves_instead() {
        // A 上有 waiting + working，B 上线：搬 working，waiting 留在已经点亮的灯上。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("B", true)
            },
        ];
        let sessions = [
            session("pi", "w1", 1, AgentState::Waiting),
            session("pi", "w2", 2, AgentState::Working),
        ];
        engine.resolve(&sessions, &devices, &config, 10);

        devices[1] = device("B", true);
        let result = engine.resolve(&sessions, &devices, &config, 20);
        assert_eq!(lamp_state(&result, "A"), Some(AgentState::Waiting));
        assert_eq!(lamp_state(&result, "B"), Some(AgentState::Working));
    }

    #[test]
    fn new_sessions_prefer_the_less_important_light() {
        // A 已经报了错，B 只是 idle：新的 waiting 会话应该去 B，否则会被 A 的 error 盖住。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let devices = vec![device("A", true), device("B", true)];
        let sessions = [
            session("pi", "e1", 1, AgentState::Error),
            session("pi", "i1", 2, AgentState::Idle),
            session("pi", "n1", 3, AgentState::Waiting),
        ];
        let result = engine.resolve(&sessions, &devices, &config, 10);
        assert_eq!(lamp_state(&result, "A"), Some(AgentState::Error));
        assert_eq!(lamp_state(&result, "B"), Some(AgentState::Waiting));
    }

    #[test]
    fn auto_agent_also_spreads_errors_to_different_lamps() {
        // 按 Agent 分配同样按等级优先摊：两个 error agent 各占一盏灯。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoAgent);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("B", true)
            },
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("C", true)
            },
        ];
        let sessions = [
            session("a1", "s1", 1, AgentState::Error),
            session("a2", "s2", 2, AgentState::Error),
            session("a3", "s3", 3, AgentState::Working),
        ];
        engine.resolve(&sessions, &devices, &config, 10);

        devices[1] = device("B", true);
        devices[2] = device("C", true);
        let result = engine.resolve(&sessions, &devices, &config, 20);

        let error_lamps: Vec<&String> = result
            .by_device
            .iter()
            .filter(|(_, views)| views.iter().any(|view| view.state == AgentState::Error))
            .map(|(device, _)| device)
            .collect();
        assert_eq!(error_lamps.len(), 2, "两个 error agent 应各占一盏灯");
    }

    #[test]
    fn priority_spreading_settles_after_one_round() {
        // 按等级摊开之后不应该在后续 tick 又搬回去。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let mut devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("B", true)
            },
            ResolvedDevice {
                connected: false,
                last_seen_ms: None,
                ..device("C", true)
            },
        ];
        let sessions = [
            session("pi", "e1", 1, AgentState::Error),
            session("pi", "e2", 2, AgentState::Error),
            session("pi", "w1", 3, AgentState::Working),
            session("pi", "i1", 4, AgentState::Idle),
        ];
        engine.resolve(&sessions, &devices, &config, 10);
        devices[1] = device("B", true);
        devices[2] = device("C", true);
        let once = engine.resolve(&sessions, &devices, &config, 20);
        let twice = engine.resolve(&sessions, &devices, &config, 21);
        assert_eq!(once.by_device, twice.by_device);
    }

    #[test]
    fn moving_sessions_never_changes_a_lamp_state_by_itself() {
        // 等级优先的副作用：新单元先去“没有 error”的灯，总数摊平再把旧的匀回来。
        // 这里守住不变量：这一来一回不会把任何一盏灯的聚合状态改掉（灯不会闪）。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let devices = vec![device("A", true), device("B", true)];
        let sessions = [
            session("pi", "e1", 1, AgentState::Error),
            session("pi", "i1", 2, AgentState::Idle),
            session("pi", "i2", 3, AgentState::Idle),
            session("pi", "i3", 4, AgentState::Idle),
            session("pi", "i4", 5, AgentState::Idle),
        ];
        let before = engine.resolve(&sessions, &devices, &config, 10);
        let before_states = (
            lamp_state(&before, "A"),
            lamp_state(&before, "B"),
        );
        assert_eq!(before_states, (Some(AgentState::Error), Some(AgentState::Idle)));

        // 新来一条 idle：先落到没有 error 的灯上，再由总数摊平匀回来。
        let mut sessions = sessions.to_vec();
        sessions.push(session("pi", "i5", 6, AgentState::Idle));
        let after = engine.resolve(&sessions, &devices, &config, 20);
        assert_eq!(after.by_device["A"].len(), 3, "总数仍然摊平");
        assert_eq!(after.by_device["B"].len(), 3, "总数仍然摊平");
        assert_eq!(lamp_state(&after, "A"), before_states.0, "A 的灯效不应变化");
        assert_eq!(lamp_state(&after, "B"), before_states.1, "B 的灯效不应变化");
    }

    #[test]
    fn new_sessions_skip_a_light_that_is_only_in_the_grace_window() {
        // B 刚掉线（还在 15 秒宽限内）：归属保留，但新会话只给真正连着的 A。
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let devices = vec![
            device("A", true),
            ResolvedDevice {
                connected: false,
                last_seen_ms: Some(10),
                ..device("B", true)
            },
        ];
        let sessions = [
            session("claude", "s1", 1, AgentState::Working),
            session("claude", "s2", 2, AgentState::Working),
        ];
        let result = engine.resolve(&sessions, &devices, &config, 10);
        assert_eq!(result.by_device.get("A").unwrap().len(), 2);
        assert!(!result.by_device.contains_key("B"));
    }

    #[test]
    fn sessions_are_retained_during_grace_then_move_off_a_dead_light() {
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoSession);
        let mut devices = vec![device("A", true), device("B", true)];
        let sessions = [
            session("claude", "s1", 1, AgentState::Working),
            session("claude", "s2", 2, AgentState::Working),
        ];
        // 两台都在线：各分一个。
        let result = engine.resolve(&sessions, &devices, &config, 10);
        assert_eq!(result.by_device.get("A").unwrap().len(), 1);
        assert_eq!(result.by_device.get("B").unwrap().len(), 1);

        // B 掉线但仍在宽限内：归属不搬（避免瞬时掉线就闪一下）。
        devices[1].connected = false;
        devices[1].last_seen_ms = Some(10);
        let result = engine.resolve(&sessions, &devices, &config, MISSING_GRACE_MS - 1);
        assert_eq!(result.by_device.get("B").unwrap().len(), 1);

        // 超过宽限：B 的会话迁到 A。
        let result = engine.resolve(&sessions, &devices, &config, MISSING_GRACE_MS + 11);
        assert_eq!(result.by_device.get("A").unwrap().len(), 2);
        assert!(!result.by_device.contains_key("B"));
    }

    #[test]
    fn auto_agent_keeps_agents_during_grace_then_moves_them() {
        let mut engine = AssignmentEngine::default();
        let config = config(RoutingStrategy::AutoAgent);
        let mut devices = vec![device("A", true), device("B", true)];
        let sessions = [
            session("claude", "s1", 1, AgentState::Working),
            session("codex", "s2", 2, AgentState::Working),
        ];
        let result = engine.resolve(&sessions, &devices, &config, 10);
        assert_eq!(result.by_device.get("A").unwrap().len(), 1);
        assert_eq!(result.by_device.get("B").unwrap().len(), 1);

        // 宽限期内保留归属，但新 agent 不会再分给掉线的 B。
        devices[1].connected = false;
        devices[1].last_seen_ms = Some(10);
        let mut more = sessions.to_vec();
        more.push(session("pi", "p3", 3, AgentState::Working));
        let result = engine.resolve(&more, &devices, &config, MISSING_GRACE_MS - 1);
        assert_eq!(result.by_device.get("B").unwrap().len(), 1);
        assert_eq!(result.by_device.get("A").unwrap().len(), 2);

        // 超过宽限：B 的 agent 迁到 A。
        let result = engine.resolve(&more, &devices, &config, MISSING_GRACE_MS + 11);
        assert_eq!(result.by_device.get("A").unwrap().len(), 3);
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
