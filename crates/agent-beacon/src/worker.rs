//! 设备发现与生命周期管理。
//!
//! 分成两个独立任务：
//!
//! * **扫描 worker**（`spawn_scanner`）：不停枚举所有支持的链路——USB/串口、BlueZ
//!   已知的蓝牙链路、BLE(NUS) 广播——把候选设备推进 channel。扫描与“每台设备的
//!   actor”解耦，新设备一被发现就能登记并显示，不必等某个 actor 结束。
//! * **管理循环**（`spawn_worker`）：消费扫描结果，登记设备、启动 actor，并在设备
//!   离线过久时把它从视图里摘掉。
//!
//! 保留策略：
//!
//! * 运行时设备（`/api/v1/state` 的 `devices`）离线超过 [`OFFLINE_RETENTION_MS`]
//!   （3 分钟）就注销，页面不再显示；
//! * **配置**（哪台灯显示哪些 Agent）独立存在：配置过的设备即使不在场也仍然出现在
//!   设置页，直到用户把它的 Agent 全部删掉且设备离线，才自动清理该配置；
//! * 蓝耳链路（`ble:` / `spp:`）连接失败后进入冷却期，避免陈旧的 BlueZ 配对记录
//!   每 3 分钟把设备“复活”一次；手动「重新扫描」会清掉冷却立刻重试。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_beacon_core::DeviceProfile;
use agent_beacon_lights::actor::{spawn, ActorConfig, TransportFactory};
use agent_beacon_lights::ble::{known_links, BleTransport, KnownLink};
use agent_beacon_lights::device::{mark_offline, offline_for_ms, snapshot as device_snapshot};
use agent_beacon_lights::discovery::{detect, PortInfo, PortKind};
use agent_beacon_lights::transport::{
    probe_agentcore, AgentCoreTransport, SerialTransport, Transport, Yh50Transport,
};
use agent_beacon_lights::DeviceInfo;
use serialport::SerialPortType;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::state::BeaconState;

/// 运行时设备离线多久后不再显示（配置里的绑定不受影响）。
pub const OFFLINE_RETENTION_MS: u64 = 180_000;
/// 蓝牙链路进入冷却后再自动重连的间隔。
const LINK_COOLDOWN: Duration = Duration::from_secs(600);
const SERIAL_INTERVAL: Duration = Duration::from_secs(2);
/// 已知链路（BlueZ 记录）刷新节奏：只读设备对象，不触发发现，可以比广播扫描勤快。
const KNOWN_LINK_INTERVAL: Duration = Duration::from_secs(5);
const BLE_SCAN_WINDOW: Duration = Duration::from_secs(3);
const BLE_SCAN_INTERVAL: Duration = Duration::from_secs(15);
/// 配置清理（删除“没有 Agent 且离线”的蓝牙配置）的节奏。
const CONFIG_PRUNE_INTERVAL: Duration = Duration::from_secs(30);
const MANAGER_TICK: Duration = Duration::from_secs(1);
/// 扫描 worker 的主循环节奏：只决定检查各通道计时器的频率。
const SCANNER_TICK: Duration = Duration::from_millis(200);

/// 设备是通过哪条链路发现的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// 系统串口（USB CDC 或蓝牙 SPP 端口）。
    Serial,
    /// BLE(NUS)。
    Ble,
    /// 免 root 的 SPP(RFCOMM) 直连。
    Spp,
}

impl LinkKind {
    fn is_bluetooth(self) -> bool {
        matches!(self, LinkKind::Ble | LinkKind::Spp)
    }

    fn scheme(self) -> &'static str {
        match self {
            LinkKind::Serial => "serial",
            LinkKind::Ble => "ble",
            LinkKind::Spp => "spp",
        }
    }
}

/// 扫描 worker 发现的一台候选设备。
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub port: String,
    pub profile: DeviceProfile,
    pub name: Option<String>,
    pub kind: LinkKind,
    /// 来自广播扫描（能证明设备此刻真的在场，可以解除冷却）。
    pub from_advertisement: bool,
}

/// 扫描 worker 推给管理循环的消息。
enum Discovery {
    /// 本轮串口枚举结果；有线设备不在其中就视为拔掉。
    SerialPorts(Vec<String>),
    Found(Candidate),
}

struct ActorEntry {
    port: String,
    kind: LinkKind,
    handle: JoinHandle<()>,
}

/// 启动后先给设备一点时间连上，再开始清理配置，避免刚开机就把还没连上的绑定删掉。
const CONFIG_PRUNE_WARMUP: Duration = Duration::from_secs(90);

/// 管理循环的状态。
struct Manager {
    actors: HashMap<String, ActorEntry>,
    /// 手动「重新扫描」时递增，扫描 worker 据此丢掉握手缓存立刻重试。
    probe_epoch: Arc<AtomicU64>,
    /// 连接失败后的冷却截止时间。
    quarantine: HashMap<String, Instant>,
    serial_ports: Vec<String>,
    last_prune: Option<Instant>,
    started: Instant,
}

impl Default for Manager {
    fn default() -> Self {
        Self {
            actors: HashMap::new(),
            probe_epoch: Arc::new(AtomicU64::new(0)),
            quarantine: HashMap::new(),
            serial_ports: Vec::new(),
            last_prune: None,
            started: Instant::now(),
        }
    }
}

/// 持有扫描任务：管理循环被中止时一起停掉，避免留下一个没人消费的扫描线程。
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn spawn_worker(state: Arc<BeaconState>) -> JoinHandle<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Discovery>();

    let manager_epoch = Arc::new(AtomicU64::new(0));
    tokio::spawn(async move {
        // 扫描 worker 独立运行：即使管理循环在忙，扫描节奏也不受影响。
        let _scanner = AbortOnDrop(spawn_scanner(state.clone(), tx, manager_epoch.clone()));
        let mut manager = Manager {
            probe_epoch: manager_epoch,
            ..Manager::default()
        };
        let mut ticker = tokio::time::interval(MANAGER_TICK);
        loop {
            let manual = tokio::select! {
                _ = ticker.tick() => false,
                _ = state.scan_notify().notified() => true,
            };
            if manual {
                manager.reset(&state);
            }
            while let Ok(item) = rx.try_recv() {
                manager.on_discovery(&state, item);
            }
            // 配置里显式绑定的 BLE / SPP 链路：这两类端口系统不会枚举出来，
            // 在 macOS / Windows 上也读不到 BlueZ 记录，只能按配置直接连。
            for candidate in candidates_from_config(&state.config()) {
                manager.on_candidate(&state, candidate);
            }
            manager.reap(&state);
            manager.prune_config(&state);
        }
    })
}

/// 扫描循环：串口 / 已知蓝牙链路 / BLE 广播三条通道各自按节奏跑，互不阻塞。
fn spawn_scanner(
    state: Arc<BeaconState>,
    tx: mpsc::UnboundedSender<Discovery>,
    probe_epoch: Arc<AtomicU64>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut probes: HashMap<String, bool> = HashMap::new();
        let mut seen_epoch = probe_epoch.load(Ordering::Relaxed);
        let mut last_serial: Option<Instant> = None;
        let mut last_known: Option<Instant> = None;
        let mut last_ble: Option<Instant> = None;
        let mut known_running = false;
        let mut ble_running = false;
        let mut serial_running = false;
        let (serial_tx, mut serial_rx) = mpsc::unbounded_channel();
        let (known_tx, mut known_rx) = mpsc::unbounded_channel();
        let (ble_tx, mut ble_rx) = mpsc::unbounded_channel();

        loop {
            // 手动「重新扫描」：丢掉握手缓存，刚配好对的蓝牙设备可以立刻重试。
            let epoch = probe_epoch.load(Ordering::Relaxed);
            if epoch != seen_epoch {
                seen_epoch = epoch;
                probes.clear();
            }

            // ── 串口 ────────────────────────────────────────────────────────
            if !serial_running && last_serial.is_none_or(|at| at.elapsed() >= SERIAL_INTERVAL) {
                last_serial = Some(Instant::now());
                serial_running = true;
                let sender = serial_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = sender.send(serialport::available_ports());
                });
            }
            while let Ok(result) = serial_rx.try_recv() {
                serial_running = false;
                match result {
                    Ok(ports) => {
                        let mut present = Vec::with_capacity(ports.len());
                        for port in ports {
                            let info = to_port_info(port);
                            present.push(info.port_name.clone());
                            let config = state.config();
                            let Some(device) = detect(&info, &config.devices) else {
                                continue;
                            };
                            if device.probe_required
                                && !probe_allows(&mut probes, &device.port).await
                            {
                                continue;
                            }
                            let _ = tx.send(Discovery::Found(Candidate {
                                id: device.id,
                                port: device.port,
                                profile: device.profile,
                                name: device.name,
                                kind: LinkKind::Serial,
                                from_advertisement: false,
                            }));
                        }
                        probes.retain(|port, _| present.contains(port));
                        let _ = tx.send(Discovery::SerialPorts(present));
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "failed to enumerate serial ports");
                    }
                }
            }

            // ── BlueZ 已知链路（不扫描）────────────────────────────────────
            if !known_running && last_known.is_none_or(|at| at.elapsed() >= KNOWN_LINK_INTERVAL) {
                last_known = Some(Instant::now());
                known_running = true;
                let sender = known_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = sender.send(known_links());
                });
            }
            while let Ok(result) = known_rx.try_recv() {
                known_running = false;
                match result {
                    Ok(links) => {
                        let ble_available =
                            links.iter().any(|link| matches!(link, KnownLink::Ble(_)));
                        for link in links {
                            // BLE 优先：能走 NUS 就不走 SPP（SPP 会占用设备的经典蓝牙链路）。
                            if matches!(link, KnownLink::Spp { .. }) && ble_available {
                                continue;
                            }
                            let kind = match link {
                                KnownLink::Ble(_) => LinkKind::Ble,
                                KnownLink::Spp { .. } => LinkKind::Spp,
                            };
                            let profile = link
                                .name()
                                .and_then(agent_beacon_lights::ble::profile_for_name)
                                .unwrap_or(DeviceProfile::AgentCore);
                            let _ = tx.send(Discovery::Found(Candidate {
                                id: link.device_id(),
                                port: link.port(),
                                profile,
                                name: link.name().map(str::to_string),
                                kind,
                                from_advertisement: false,
                            }));
                        }
                    }
                    Err(error) => {
                        tracing::debug!(error = %error, "cannot list known Bluetooth devices");
                    }
                }
            }

            // ── BLE 广播：只在没有已连接的 BLE 设备时扫 ────────────────────
            // BlueZ 的发现过程会打断已有 LE 连接，所以有连接时不扫；判断依据是
            // “有没有连着的 BLE 设备”，而不是“有没有 BLE 记录”——后者会让一条陈旧
            // 配对记录把扫描永久关掉。
            let ble_connected = device_snapshot(state.devices())
                .iter()
                .any(|info| info.connected && info.id.starts_with("ble:"));
            if !ble_connected
                && !ble_running
                && last_ble.is_none_or(|at| at.elapsed() >= BLE_SCAN_INTERVAL)
            {
                last_ble = Some(Instant::now());
                ble_running = true;
                let sender = ble_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = sender.send(agent_beacon_lights::ble::scan(BLE_SCAN_WINDOW));
                });
            }
            while let Ok(result) = ble_rx.try_recv() {
                ble_running = false;
                match result {
                    Ok(discoveries) => {
                        for discovery in discoveries {
                            let profile = discovery
                                .name
                                .as_deref()
                                .and_then(agent_beacon_lights::ble::profile_for_name)
                                .unwrap_or(DeviceProfile::AgentCore);
                            let port = format!("ble:{}", discovery.address.to_lowercase());
                            let _ = tx.send(Discovery::Found(Candidate {
                                id: discovery.device_id(),
                                port,
                                profile,
                                name: discovery.name.clone(),
                                kind: LinkKind::Ble,
                                from_advertisement: true,
                            }));
                        }
                    }
                    Err(error) => {
                        tracing::debug!(error = %error, "BLE scanning unavailable");
                    }
                }
            }

            tokio::time::sleep(SCANNER_TICK).await;
        }
    })
}

impl Manager {
    fn on_discovery(&mut self, state: &Arc<BeaconState>, item: Discovery) {
        match item {
            Discovery::SerialPorts(ports) => {
                self.serial_ports = ports;
                // 有线设备拔掉立刻显示“未连接”，保留期过了再移除。
                let missing: Vec<String> = self
                    .actors
                    .iter()
                    .filter(|(_, entry)| {
                        entry.kind == LinkKind::Serial && !self.serial_ports.contains(&entry.port)
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                let now = agent_beacon_core::now_ms();
                for id in missing {
                    mark_offline(state.devices(), &id, now, None);
                }
            }
            Discovery::Found(candidate) => self.on_candidate(state, candidate),
        }
    }

    fn on_candidate(&mut self, state: &Arc<BeaconState>, candidate: Candidate) {
        if self.actors.contains_key(&candidate.id) {
            // 已经连着的链路：广播名 / BlueZ 记录晚到，补上展示用的设备名。
            if let Some(name) = candidate.name.as_deref() {
                agent_beacon_lights::device::update(state.devices(), &candidate.id, |info| {
                    if info.name.is_none() {
                        info.name = Some(name.to_string());
                    }
                });
            }
            return;
        }
        // 广播扫到就说明设备确实在场，解除冷却。
        if candidate.from_advertisement {
            self.quarantine.remove(&candidate.id);
        }
        if let Some(until) = self.quarantine.get(&candidate.id) {
            if *until > Instant::now() {
                return;
            }
            self.quarantine.remove(&candidate.id);
        }
        tracing::info!(
            device = %candidate.id,
            port = %candidate.port,
            profile = candidate.profile.as_str(),
            name = candidate.name.as_deref().unwrap_or(""),
            link = candidate.kind.scheme(),
            "device discovered"
        );
        register_device(
            state,
            &candidate.id,
            &candidate.port,
            candidate.profile,
            candidate.name.as_deref(),
        );
        let handle = spawn_actor(state, &candidate.id, &candidate.port, candidate.profile);
        self.actors.insert(
            candidate.id,
            ActorEntry {
                port: candidate.port,
                kind: candidate.kind,
                handle,
            },
        );
    }

    /// 移除离线过久的运行时设备；蓝耳链路顺带进入冷却，避免陈旧配对记录反复“复活”。
    fn reap(&mut self, state: &Arc<BeaconState>) {
        let now = agent_beacon_core::now_ms();
        let devices = device_snapshot(state.devices());
        let mut expired: Vec<(String, LinkKind)> = Vec::new();
        let mut finished: Vec<String> = Vec::new();

        for (id, entry) in self.actors.iter() {
            if entry.handle.is_finished() {
                finished.push(id.clone());
                continue;
            }
            let info = devices.iter().find(|info| &info.id == id);
            // 掉线（含“拔掉后一直没连上”）超过保留期的设备不再显示。
            if info.is_some_and(|info| retention_expired(info, now)) {
                expired.push((id.clone(), entry.kind));
            }
        }

        for id in finished {
            if let Some(entry) = self.actors.remove(&id) {
                entry.handle.abort();
            }
            state.unregister_device(&id);
        }

        for (id, kind) in expired {
            if let Some(entry) = self.actors.remove(&id) {
                entry.handle.abort();
                tracing::info!(
                    device = %id,
                    port = %entry.port,
                    link = kind.scheme(),
                    "device offline too long; removing it from the device list"
                );
            }
            state.unregister_device(&id);
            if kind.is_bluetooth() {
                self.quarantine.insert(id, Instant::now() + LINK_COOLDOWN);
            }
        }
    }

    /// 手动「重新扫描」：清掉冷却与握手缓存，让蓝牙设备立刻重试。
    fn reset(&mut self, state: &Arc<BeaconState>) {
        self.probe_epoch.fetch_add(1, Ordering::Relaxed);
        self.quarantine.clear();
        let bluetooth: Vec<String> = self
            .actors
            .iter()
            .filter(|(_, entry)| entry.kind.is_bluetooth())
            .map(|(id, _)| id.clone())
            .collect();
        for id in bluetooth {
            if let Some(entry) = self.actors.remove(&id) {
                entry.handle.abort();
            }
            state.unregister_device(&id);
        }
    }

    /// 清理“没有 Agent 且不在线”的蓝牙配置。
    ///
    /// 配置过的设备即使不在场也保留（设置页仍要显示）；只有用户把它的 Agent 全删了、
    /// 设备又确实不在线时，才把这个陈旧配对记录对应的配置删掉。串口/端口绑定用来
    /// 识别设备（Windows 的 COM 口没有 USB 元数据），不自动清理。
    fn prune_config(&mut self, state: &Arc<BeaconState>) {
        if self.started.elapsed() < CONFIG_PRUNE_WARMUP {
            return;
        }
        if self
            .last_prune
            .is_some_and(|at| at.elapsed() < CONFIG_PRUNE_INTERVAL)
        {
            return;
        }
        self.last_prune = Some(Instant::now());

        let mut next = state.config();
        let connected: HashSet<String> = device_snapshot(state.devices())
            .into_iter()
            .filter(|info| info.connected)
            .map(|info| info.id)
            .collect();
        let changed = prune_config_value(&mut next, &connected);
        if changed {
            match state.set_config(next.clone()) {
                Ok(()) => tracing::info!(
                    devices = next.devices.len(),
                    manual = next.routing.manual.len(),
                    "pruned stale device configuration"
                ),
                Err(error) => tracing::warn!(error = %error, "failed to save pruned config"),
            }
        }
    }
}

/// 配置里显式绑定的蓝牙链路（`ble:` / `spp:`）。
///
/// 这两类端口不会被系统枚举出来；macOS / Windows 也没有 BlueZ 记录可读，
/// 所以配置是它们唯一的发现来源。
fn candidates_from_config(config: &agent_beacon_core::Config) -> Vec<Candidate> {
    config
        .devices
        .iter()
        .filter_map(|binding| {
            if !binding.profile.is_agentcore() {
                return None;
            }
            let link = binding.port.as_deref().or(binding.id.as_deref())?;
            let link = link.trim().to_lowercase();
            let kind = if link.starts_with("ble:") {
                LinkKind::Ble
            } else if link.starts_with("spp:") {
                LinkKind::Spp
            } else {
                return None;
            };
            Some(Candidate {
                id: link.clone(),
                port: link,
                profile: binding.profile,
                name: None,
                kind,
                from_advertisement: false,
            })
        })
        .collect()
}

/// 清理配置；返回是否有改动。抽出来便于单测，不需要真实设备。
fn prune_config_value(config: &mut agent_beacon_core::Config, connected: &HashSet<String>) -> bool {
    let mut changed = false;

    // 空 Agent 列表没有任何意义，直接清掉。
    let empty: Vec<String> = config
        .routing
        .manual
        .iter()
        .filter(|(_, agents)| agents.is_empty())
        .map(|(id, _)| id.clone())
        .collect();
    for id in empty {
        config.routing.manual.remove(&id);
        changed = true;
    }

    let manual = config.routing.manual.clone();
    let before = config.devices.len();
    config
        .devices
        .retain(|binding| !prunable_bluetooth_binding(binding, &manual, connected));
    changed || config.devices.len() != before
}

/// 设备是否已经离线超过保留期。
fn retention_expired(info: &DeviceInfo, now_ms: u64) -> bool {
    offline_for_ms(info, now_ms).is_some_and(|ms| ms >= OFFLINE_RETENTION_MS)
}

/// 蓝牙绑定（`ble:` / `spp:`）在没有 Agent 且设备不在线时可以删除。
fn prunable_bluetooth_binding(
    binding: &agent_beacon_core::DeviceBinding,
    manual: &HashMap<String, Vec<String>>,
    connected: &HashSet<String>,
) -> bool {
    let ids = bluetooth_ids(binding);
    if ids.is_empty() {
        return false;
    }
    // 带了展示设置的绑定是用户的显式配置，不自动删。
    if binding.label.is_some() || binding.brightness.is_some() || binding.buzzer_on_alert.is_some()
    {
        return false;
    }
    if ids.iter().any(|id| connected.contains(id)) {
        return false;
    }
    // 还有 Agent 分配就保留：设备不在场，配置也要能在设置页里看到。
    !ids.iter()
        .any(|id| manual.get(id).is_some_and(|agents| !agents.is_empty()))
}

/// 绑定里指向蓝牙链路的设备 id（`ble:…` / `spp:…`）。
fn bluetooth_ids(binding: &agent_beacon_core::DeviceBinding) -> Vec<String> {
    let mut ids = Vec::new();
    for value in [binding.id.as_deref(), binding.port.as_deref()] {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        let value = value.to_lowercase();
        if value.starts_with("ble:") || value.starts_with("spp:") {
            ids.push(value);
        }
    }
    ids
}

/// 注册设备并把设备自报的名字（BLE 广播名 / USB 产品名）一起存下来。
fn register_device(
    state: &Arc<BeaconState>,
    id: &str,
    port: &str,
    profile: DeviceProfile,
    name: Option<&str>,
) {
    let mut info = DeviceInfo::new(id, port, profile);
    info.name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    state.register_device(info);
}

fn spawn_actor(
    state: &Arc<BeaconState>,
    id: &str,
    port: &str,
    profile: DeviceProfile,
) -> JoinHandle<()> {
    let factory: TransportFactory = match profile {
        DeviceProfile::MlightSingle | DeviceProfile::Mlight3Led => {
            let port = port.to_string();
            Arc::new(move || {
                SerialTransport::open(&port, 115_200)
                    .map(|transport| Box::new(transport) as Box<dyn Transport>)
            })
        }
        DeviceProfile::Yh50 => {
            let port = port.to_string();
            Arc::new(move || {
                Yh50Transport::open(&port)
                    .map(|transport| Box::new(transport) as Box<dyn Transport>)
            })
        }
        profile if profile.is_agentcore() && port.starts_with("ble:") => {
            // BLE 链路：连接/扫描都在它自己的后台线程里跑，这里只负责建连。
            let selector = port.trim_start_matches("ble:").to_string();
            Arc::new(move || {
                let target = selector.clone();
                in_blocking(move || BleTransport::open(&target))
                    .map(|transport| Box::new(transport) as Box<dyn Transport>)
            })
        }
        profile if profile.is_agentcore() && port.starts_with("spp:") => {
            // 免 root 的 SPP 直连：自己开 RFCOMM socket，不依赖 /dev/rfcomm* 节点。
            let target = port.trim_start_matches("spp:").to_string();
            Arc::new(move || {
                let target = target.clone();
                in_blocking(move || AgentCoreTransport::open_spp(&target))
                    .map(|transport| Box::new(transport) as Box<dyn Transport>)
            })
        }
        // AgentCore-Light / Halo 的 USB CDC 链路（也是其余型号的兜底分支）。
        _ => {
            let port = port.to_string();
            Arc::new(move || {
                // 握手要等 1.5 s 的 CDC 就绪时间，放到阻塞线程池里等，
                // 别把 API/SSE 所在的 tokio worker 卡住。
                let target = port.clone();
                in_blocking(move || AgentCoreTransport::open(&target))
                    .map(|transport| Box::new(transport) as Box<dyn Transport>)
            })
        }
    };

    spawn(ActorConfig {
        id: id.to_string(),
        port: port.to_string(),
        profile,
        factory,
        desired: state.subscribe_desired(),
        registry: state.devices().clone(),
    })
}

/// 在阻塞线程池里跑一段会 sleep 的打开流程；当前线程运行时则直接执行。
fn in_blocking<T, F>(work: F) -> T
where
    F: FnOnce() -> T,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(work)
        }
        _ => work(),
    }
}

/// 蓝牙等“端口名不可靠”的候选设备：只在第一次遇到该端口时握手确认。
async fn probe_allows(probes: &mut HashMap<String, bool>, port: &str) -> bool {
    if let Some(allowed) = probes.get(port) {
        return *allowed;
    }
    let path = port.to_string();
    let result = tokio::task::spawn_blocking(move || probe_agentcore(&path))
        .await
        .unwrap_or_else(|error| Err(format!("probe task failed: {error}")));
    let allowed = match result {
        Ok(identity) => {
            tracing::info!(
                port = port,
                firmware = identity.firmware.as_deref(),
                state = identity.state.as_deref(),
                "detected AgentCore-Light over bluetooth"
            );
            true
        }
        Err(error) => {
            tracing::debug!(port = port, error = %error, "bluetooth port is not an AgentCore-Light");
            false
        }
    };
    probes.insert(port.to_string(), allowed);
    allowed
}

fn to_port_info(port: serialport::SerialPortInfo) -> PortInfo {
    let port_name = port.port_name;
    match port.port_type {
        SerialPortType::UsbPort(usb) => PortInfo {
            port_name,
            vid: Some(usb.vid),
            pid: Some(usb.pid),
            serial: usb.serial_number,
            product: usb.product,
            kind: PortKind::Usb,
        },
        SerialPortType::BluetoothPort => PortInfo {
            port_name,
            vid: None,
            pid: None,
            serial: None,
            product: None,
            kind: PortKind::Bluetooth,
        },
        SerialPortType::PciPort | SerialPortType::Unknown => PortInfo {
            port_name,
            vid: None,
            pid: None,
            serial: None,
            product: None,
            kind: PortKind::Other,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_beacon_core::DeviceBinding;

    fn binding(id: &str, profile: DeviceProfile) -> DeviceBinding {
        DeviceBinding {
            serial: None,
            port: None,
            id: Some(id.to_string()),
            profile,
            label: None,
            brightness: None,
            buzzer_on_alert: None,
        }
    }

    fn device(offline_since_ms: Option<u64>, connected: bool) -> DeviceInfo {
        let mut info = DeviceInfo::new("ble:aa", "ble:aa", DeviceProfile::AgentCore);
        info.connected = connected;
        info.offline_since_ms = offline_since_ms;
        info
    }

    #[test]
    fn devices_are_reaped_after_the_retention_window() {
        let never_connected = device(Some(1_000), false);
        assert!(!retention_expired(&never_connected, 1_000 + 179_000));
        assert!(retention_expired(&never_connected, 1_000 + 180_000));

        let connected = device(None, true);
        assert!(!retention_expired(&connected, u64::MAX));
    }

    #[test]
    fn pruning_keeps_assigned_and_configured_bindings() {
        use agent_beacon_core::{Config, RoutingStrategy};

        let mut config = Config::default();
        config.routing.strategy = RoutingStrategy::ManualAgent;
        // 空 Agent：没意义，删掉。
        config
            .routing
            .manual
            .insert("ble:de:ad:be:ef:00:00".into(), Vec::new());
        // 有 Agent 的蓝牙配置：设备不在线也要保留（设置页要能显示）。
        config
            .routing
            .manual
            .insert("ble:de:ad:be:ef:00:01".into(), vec!["pi".into()]);
        config
            .devices
            .push(binding("ble:de:ad:be:ef:00:01", DeviceProfile::AgentCore));
        // 没有 Agent、没有展示设置的陈旧配对记录：清理。
        config
            .devices
            .push(binding("ble:de:ad:be:ef:00:02", DeviceProfile::AgentCore));
        // 有线绑定用来识别设备（Windows 的 COM 口），不自动清理。
        let mut wired = binding("serial:ABC", DeviceProfile::Mlight3Led);
        wired.id = None;
        wired.serial = Some("ABC".into());
        config.devices.push(wired);

        let changed = prune_config_value(&mut config, &HashSet::new());
        assert!(changed);
        assert!(!config.routing.manual.contains_key("ble:de:ad:be:ef:00:00"));
        assert!(config.routing.manual.contains_key("ble:de:ad:be:ef:00:01"));
        let ids: Vec<_> = config
            .devices
            .iter()
            .filter_map(|binding| binding.id.clone().or_else(|| binding.serial.clone()))
            .collect();
        assert_eq!(
            ids,
            vec!["ble:de:ad:be:ef:00:01".to_string(), "ABC".to_string()]
        );
    }

    #[test]
    fn configured_bluetooth_links_become_candidates() {
        use agent_beacon_core::{Config, DeviceBinding};

        let mut config = Config::default();
        config.devices.push(DeviceBinding {
            serial: None,
            port: Some("ble:af:c3:a4:49:2c:6a".into()),
            id: None,
            profile: DeviceProfile::AgentCore,
            label: None,
            brightness: None,
            buzzer_on_alert: None,
        });
        config.devices.push(DeviceBinding {
            serial: Some("ABC".into()),
            port: None,
            id: None,
            profile: DeviceProfile::Mlight3Led,
            label: None,
            brightness: None,
            buzzer_on_alert: None,
        });

        let candidates = candidates_from_config(&config);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "ble:af:c3:a4:49:2c:6a");
        assert_eq!(candidates[0].kind, LinkKind::Ble);
    }

    #[test]
    fn only_bluetooth_ids_are_collected() {
        assert_eq!(
            bluetooth_ids(&binding(
                "ble:24:26:4c:c3:fd:10",
                DeviceProfile::AgentCoreHalo
            )),
            vec!["ble:24:26:4c:c3:fd:10".to_string()]
        );
        assert!(bluetooth_ids(&binding("serial:ABC", DeviceProfile::AgentCore)).is_empty());
    }

    #[test]
    fn bluetooth_binding_without_agents_is_pruned_when_offline() {
        let mut manual = HashMap::new();
        let connected = HashSet::new();
        let device = binding("ble:af:c3:a4:49:2c:6a", DeviceProfile::AgentCore);
        assert!(prunable_bluetooth_binding(&device, &manual, &connected));

        manual.insert("ble:af:c3:a4:49:2c:6a".to_string(), vec!["pi".to_string()]);
        assert!(
            !prunable_bluetooth_binding(&device, &manual, &connected),
            "还有 Agent 的配置要保留，设备不在线也要能显示"
        );
    }

    #[test]
    fn connected_device_or_user_settings_are_never_pruned() {
        let manual = HashMap::new();
        let device = binding("ble:af:c3:a4:49:2c:6a", DeviceProfile::AgentCore);
        let mut connected = HashSet::new();
        connected.insert("ble:af:c3:a4:49:2c:6a".to_string());
        assert!(!prunable_bluetooth_binding(&device, &manual, &connected));

        let mut labelled = device.clone();
        labelled.label = Some("书桌灯".to_string());
        assert!(!prunable_bluetooth_binding(
            &labelled,
            &manual,
            &HashSet::new()
        ));
    }
}
