//! 设备发现循环：发现新设备、按配置启动 actor，并处理 mock 设备开关。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use agent_beacon_core::DeviceProfile;
use agent_beacon_lights::actor::{spawn, ActorConfig, TransportFactory};
use agent_beacon_lights::device::upsert;
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

pub fn spawn_worker(state: Arc<BeaconState>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut actors: HashMap<String, ActorEntry> = HashMap::new();
        let mut mock_actor: Option<ActorEntry> = None;
        // 蓝牙端口的握手探测只做一次：`HELLO` 要等 1.5 s，不能每两秒重来。
        let mut probes: HashMap<String, bool> = HashMap::new();

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
