//! 设备发现循环：发现新设备、按配置启动 actor，并处理 mock 设备开关。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use agent_beacon_core::DeviceProfile;
use agent_beacon_lights::actor::{spawn, ActorConfig, TransportFactory};
use agent_beacon_lights::device::upsert;
use agent_beacon_lights::discovery::{detect, PortInfo};
use agent_beacon_lights::mock::{MockLog, MockTransport};
use agent_beacon_lights::transport::{SerialTransport, Transport, Yh50Transport};
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

        loop {
            let config = state.config();
            let ports = match serialport::available_ports() {
                Ok(ports) => ports,
                Err(error) => {
                    tracing::warn!(error = %error, "枚举串口失败");
                    Vec::new()
                }
            };

            for port in ports {
                let Some(info) = to_port_info(port) else {
                    continue;
                };
                let Some(device) = detect(&info, &config.devices) else {
                    continue;
                };
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

            tokio::select! {
                _ = state.scan_notify().notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
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

fn to_port_info(port: serialport::SerialPortInfo) -> Option<PortInfo> {
    match port.port_type {
        SerialPortType::UsbPort(usb) => Some(PortInfo {
            port_name: port.port_name,
            vid: Some(usb.vid),
            pid: Some(usb.pid),
            serial: usb.serial_number,
        }),
        _ => None,
    }
}
