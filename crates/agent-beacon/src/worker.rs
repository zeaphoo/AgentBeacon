//! 设备发现循环：发现新设备、按配置启动 actor，并处理 mock 设备开关。
//!
//! 两条发现通道并行：
//!
//! * 串口（USB CDC / 蓝牙 SPP）——每 2 s 枚举一次；
//! * BLE（NUS）——每 15 s 扫一次广播（扫描比较贵，且设备插着 USB 时不广播）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_beacon_core::DeviceProfile;
use agent_beacon_lights::actor::{spawn, ActorConfig, TransportFactory};
use agent_beacon_lights::ble::{known_links, BleTransport, KnownLink};
use agent_beacon_lights::device::{remove, upsert};
use agent_beacon_lights::discovery::{detect, PortInfo, PortKind};
use agent_beacon_lights::mock::{MockLog, MockTransport};
use agent_beacon_lights::transport::{
    probe_agentcore, AgentCoreTransport, SerialTransport, Transport, Yh50Transport,
};
use agent_beacon_lights::DeviceInfo;
use serialport::SerialPortType;
use tokio::task::JoinHandle;

use crate::state::BeaconState;

struct ActorEntry {
    profile: DeviceProfile,
    port: String,
    handle: JoinHandle<()>,
}

/// BLE 扫描节奏：每次扫多久、隔多久扫一次。
///
/// 只在「还没有 BLE 设备」时扫：BlueZ 发现过程中会打断已有的 LE 连接。
const BLE_SCAN_WINDOW: Duration = Duration::from_secs(3);
const BLE_SCAN_INTERVAL: Duration = Duration::from_secs(15);

/// 已知链路（BlueZ 已认识的设备）刷新节奏。
///
/// 这一步**不触发 BLE 发现**，只是读 BlueZ 的设备对象，所以不存在「扫描打断 LE 连接」
/// 的问题，可以比广播扫描勤快得多：设备连上之后就不再广播，靠它才能发现。
const KNOWN_LINK_INTERVAL: Duration = Duration::from_secs(10);

pub fn spawn_worker(state: Arc<BeaconState>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut actors: HashMap<String, ActorEntry> = HashMap::new();
        let mut mock_actor: Option<ActorEntry> = None;
        // 蓝牙端口的握手探测只做一次：`HELLO` 要等 1.5 s，不能每两秒重来。
        let mut probes: HashMap<String, bool> = HashMap::new();
        // BLE：扫描是阻塞动作（一次要扫几秒），交给后台线程跑，结果从这个 channel 回来，
        // 保证串口枚举和 mock 设备不被拖慢。
        let (ble_tx, mut ble_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ble_scanning = false;
        let mut last_ble_scan: Option<Instant> = None;
        let mut ble_unavailable = false;
        // 已认识的链路（不扫描）：与广播扫描分开跑，互不阻塞。
        let (known_tx, mut known_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut known_running = false;
        let mut last_known: Option<Instant> = None;

        loop {
            let config = state.config();
            let ports = match serialport::available_ports() {
                Ok(ports) => ports,
                Err(error) => {
                    tracing::warn!(error = %error, "failed to enumerate serial ports");
                    Vec::new()
                }
            };

            let mut current_ports: Vec<String> = Vec::new();

            for port in ports {
                let info = to_port_info(port);
                let Some(device) = detect(&info, &config.devices) else {
                    continue;
                };
                current_ports.push(info.port_name.clone());

                if device.probe_required && !probe_allows(&mut probes, &device.port).await {
                    continue;
                }
                let reuse = actors.get(&device.id).is_some_and(|entry| {
                    entry.profile == device.profile && entry.port == device.port
                });
                if reuse {
                    continue;
                }
                if let Some(entry) = actors.remove(&device.id) {
                    entry.handle.abort();
                }
                upsert(
                    state.devices(),
                    DeviceInfo::new(&device.id, &device.port, device.profile),
                );
                let handle = spawn_actor(&state, &device.id, &device.port, device.profile);
                actors.insert(
                    device.id,
                    ActorEntry {
                        profile: device.profile,
                        port: device.port,
                        handle,
                    },
                );
            }

            // 端口拔掉后清掉探测结果，重新插上时会再握手一次。
            probes.retain(|port, _| current_ports.contains(port));

            // 配置里显式绑定的 BLE / SPP 链路：这两类端口系统不会枚举出来，
            // 只能按 `devices[{ port: "ble:…" / "spp:…" }]` 直接起 actor。
            for binding in config.devices.iter() {
                if !binding.profile.is_agentcore() {
                    continue;
                }
                let Some(port) = binding.port.as_deref() else {
                    continue;
                };
                if !(port.starts_with("ble:") || port.starts_with("spp:")) {
                    continue;
                }
                let id = port.to_lowercase();
                if actors.contains_key(&id) {
                    continue;
                }
                tracing::info!(port = %id, "binding AgentCore-Light link from config");
                upsert(
                    state.devices(),
                    DeviceInfo::new(&id, &id, DeviceProfile::AgentCore),
                );
                let handle = spawn_actor(&state, &id, &id, DeviceProfile::AgentCore);
                actors.insert(
                    id.clone(),
                    ActorEntry {
                        profile: DeviceProfile::AgentCore,
                        port: id,
                        handle,
                    },
                );
            }

            // 先看 BlueZ 已经认识的设备：**不扫描**，所以既不会打断已有 LE 连接，
            // 也能看到「已经连着、早就不广播」的设备（这正是只靠扫描发现不了的那类）。
            if !known_running && last_known.is_none_or(|at| at.elapsed() >= KNOWN_LINK_INTERVAL) {
                last_known = Some(Instant::now());
                known_running = true;
                let sender = known_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = sender.send(known_links());
                });
            }
            let mut known_ble = false;
            while let Ok(result) = known_rx.try_recv() {
                known_running = false;
                match result {
                    Ok(links) => {
                        let ble_available =
                            links.iter().any(|link| matches!(link, KnownLink::Ble(_)));
                        known_ble |= ble_available;
                        for link in links {
                            let id = link.device_id();
                            // BLE 优先：能走 NUS 就不走 SPP（SPP 会占用设备的经典蓝牙链路）。
                            if matches!(link, KnownLink::Spp { .. }) && ble_available {
                                continue;
                            }
                            if actors.contains_key(&id) {
                                continue;
                            }
                            tracing::info!(
                                address = link.address(),
                                name = link.name(),
                                link = if matches!(link, KnownLink::Ble(_)) {
                                    "ble"
                                } else {
                                    "spp"
                                },
                                "detected AgentCore-Light from known devices (no scan)"
                            );
                            upsert(
                                state.devices(),
                                DeviceInfo::new(&id, &id, DeviceProfile::AgentCore),
                            );
                            let handle = spawn_actor(&state, &id, &id, DeviceProfile::AgentCore);
                            actors.insert(
                                id.clone(),
                                ActorEntry {
                                    profile: DeviceProfile::AgentCore,
                                    port: id,
                                    handle,
                                },
                            );
                        }
                    }
                    Err(error) => {
                        tracing::debug!(error = %error, "cannot list known Bluetooth devices");
                    }
                }
            }

            // 广播扫描只作为最后兜底：设备已知（配对过/连着）时它根本不广播，扫也没用，
            // 而 BlueZ 的发现过程还会打断已有的 LE 连接（实测连接被 `Not connected` 掉线）。
            let has_ble_actor = known_ble || actors.keys().any(|id| id.starts_with("ble:"));
            if !has_ble_actor
                && !ble_scanning
                && last_ble_scan.is_none_or(|at| at.elapsed() >= BLE_SCAN_INTERVAL)
            {
                last_ble_scan = Some(Instant::now());
                ble_scanning = true;
                let sender = ble_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = sender.send(agent_beacon_lights::ble::scan(BLE_SCAN_WINDOW));
                });
            }

            while let Ok(result) = ble_rx.try_recv() {
                ble_scanning = false;
                match result {
                    Ok(discoveries) => {
                        if ble_unavailable {
                            tracing::info!("BLE scanning is available again");
                            ble_unavailable = false;
                        }
                        for discovery in discoveries {
                            let id = discovery.device_id();
                            if actors.contains_key(&id) {
                                continue;
                            }
                            tracing::info!(
                                address = discovery.address,
                                name = discovery.name.as_deref(),
                                "detected AgentCore-Light over BLE"
                            );
                            let port = format!("ble:{}", discovery.address.to_lowercase());
                            upsert(
                                state.devices(),
                                DeviceInfo::new(&id, &port, DeviceProfile::AgentCore),
                            );
                            let handle = spawn_actor(&state, &id, &port, DeviceProfile::AgentCore);
                            actors.insert(
                                id.clone(),
                                ActorEntry {
                                    profile: DeviceProfile::AgentCore,
                                    port,
                                    handle,
                                },
                            );
                        }
                    }
                    Err(error) => {
                        if !ble_unavailable {
                            tracing::info!(error = %error, "BLE scanning unavailable (serial links still work)");
                            ble_unavailable = true;
                        }
                    }
                }
            }

            if config.mock.enabled {
                if mock_actor.is_none() {
                    upsert(
                        state.devices(),
                        DeviceInfo::new("mock:0", "mock", DeviceProfile::Mock),
                    );
                    let handle = spawn_actor(&state, "mock:0", "mock", DeviceProfile::Mock);
                    mock_actor = Some(ActorEntry {
                        profile: DeviceProfile::Mock,
                        port: "mock".to_string(),
                        handle,
                    });
                }
            } else if let Some(entry) = mock_actor.take() {
                entry.handle.abort();
                if let Ok(mut devices) = state.devices().lock() {
                    devices.remove("mock:0");
                }
            }

            // 让被 abort 的 actor 有机会结束，避免句柄堆积。
            actors.retain(|_, entry| !entry.handle.is_finished());
            if mock_actor
                .as_ref()
                .is_some_and(|entry| entry.handle.is_finished())
            {
                mock_actor = None;
            }

            let manual = tokio::select! {
                _ = state.scan_notify().notified() => true,
                _ = tokio::time::sleep(Duration::from_secs(2)) => false,
            };
            if manual {
                // 手动“重新扫描”时丢掉握手结果：刚配好对的蓝牙设备可以立刻重试。
                probes.clear();
                // BLE 设备在用时不会后台扫描（会打断连接），所以这里让它们重新发现一次。
                let ble_devices: Vec<String> = actors
                    .keys()
                    .filter(|id| id.starts_with("ble:"))
                    .cloned()
                    .collect();
                for id in ble_devices {
                    if let Some(entry) = actors.remove(&id) {
                        entry.handle.abort();
                    }
                    remove(state.devices(), &id);
                }
                last_ble_scan = None;
                last_known = None;
            }
        }
    })
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
        DeviceProfile::AgentCore if port.starts_with("ble:") => {
            // BLE 链路：连接/扫描都在它自己的后台线程里跑，这里只负责建连。
            let selector = port.trim_start_matches("ble:").to_string();
            Arc::new(move || {
                let target = selector.clone();
                in_blocking(move || BleTransport::open(&target))
                    .map(|transport| Box::new(transport) as Box<dyn Transport>)
            })
        }
        DeviceProfile::AgentCore if port.starts_with("spp:") => {
            // 免 root 的 SPP 直连：自己开 RFCOMM socket，不依赖 /dev/rfcomm* 节点。
            let target = port.trim_start_matches("spp:").to_string();
            Arc::new(move || {
                let target = target.clone();
                in_blocking(move || AgentCoreTransport::open_spp(&target))
                    .map(|transport| Box::new(transport) as Box<dyn Transport>)
            })
        }
        DeviceProfile::AgentCore => {
            let port = port.to_string();
            Arc::new(move || {
                // 握手要等 1.5 s 的 CDC 就绪时间，放到阻塞线程池里等，
                // 别把 API/SSE 所在的 tokio worker 卡住。
                let target = port.clone();
                in_blocking(move || AgentCoreTransport::open(&target))
                    .map(|transport| Box::new(transport) as Box<dyn Transport>)
            })
        }
        DeviceProfile::Mock => {
            Arc::new(|| Ok(Box::new(MockTransport::new(MockLog::default())) as Box<dyn Transport>))
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
