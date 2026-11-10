//! AgentCore-Light 的 BLE（Nordic UART Service）链路。
//!
//! 设备用 NUS 暴露同一套文本行协议：
//!
//! | 用途 | UUID |
//! | --- | --- |
//! | Service | `6e400001-b5a3-f393-e0a9-e50e24dcca9e` |
//! | RX（主机 → 设备，Write） | `6e400002-b5a3-f393-e0a9-e50e24dcca9e` |
//! | TX（设备 → 主机，Notify） | `6e400003-b5a3-f393-e0a9-e50e24dcca9e` |
//!
//! 与串口的两点差异：
//!
//! * BLE 不做 `HELLO` 握手，连上直接发命令（设备只会回 `STATUS:` / `CHG:` 等上报）；
//! * 写入要按 MTU 分片（默认 20 字节），不支持无响应写时降级为带响应写。
//!
//! `Transport` 是同步 trait，而 btleplug 是 async，因此这里把连接与读写放进一个
//! 专用后台线程里跑（自带 current-thread runtime），调用方通过 channel 交互：
//! 这样既不用在 actor 的 async 任务里 `block_on`（会 panic），也不会把 BLE 的
//! 通知流拖进主 runtime。
//!
//! 注意：设备插着 USB 时**不广播**，所以用 BLE 必须先拔掉 USB（协议文档 §2.3）。

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use futures_util::StreamExt as _;
use btleplug::platform::{Adapter, Manager, Peripheral};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use uuid::Uuid;

use crate::transport::{split_lines, Transport};

pub const NUS_SERVICE: Uuid = Uuid::from_u128(0x6e400001_b5a3_f393_e0a9_e50e24dcca9e);
pub const NUS_RX: Uuid = Uuid::from_u128(0x6e400002_b5a3_f393_e0a9_e50e24dcca9e);
pub const NUS_TX: Uuid = Uuid::from_u128(0x6e400003_b5a3_f393_e0a9_e50e24dcca9e);

/// 广播名 / 本地名前缀；改名后仍是这个前缀（`SET_BT_NAME` 限制）。
pub const NAME_PREFIX: &str = "agentcore-light";
/// 默认 MTU 分片大小（协议文档 §2.3）。
const DEFAULT_CHUNK: usize = 20;
const SCAN_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// 同一条通知在 BlueZ 上会来两次，这个窗口内的重复行丢掉。
const DUPLICATE_WINDOW: Duration = Duration::from_millis(300);

/// 扫描到的一台 AgentCore-Light。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BleDiscovery {
    /// 形如 `aa:bb:cc:dd:ee:ff`；BLE 地址与经典蓝牙 SPP 地址不同（协议文档 §2.3）。
    pub address: String,
    pub name: Option<String>,
}

impl BleDiscovery {
    /// 设备 id：`ble:<address>`，与串口的 `serial:`/`port:` 命名区分开。
    pub fn device_id(&self) -> String {
        format!("ble:{}", self.address.to_lowercase())
    }
}

pub fn name_matches(name: &str) -> bool {
    name.to_lowercase().starts_with(NAME_PREFIX)
}

/// 是不是一条可用的 BLE（NUS）链路。
///
/// 实测同一台设备会广播两条记录：经典蓝牙那条名字正好是 `AgentCore-Light`（无后缀），
/// BLE(NUS) 那条是 `AgentCore-Light-<suffix>`。经典记录连上也拿不到 NUS 特征，
/// 所以「名字完全等于 AgentCore-Light 且没广播 NUS service」的要跳过。
pub fn looks_like_ble(name: Option<&str>, advertises_nus: bool) -> bool {
    if advertises_nus {
        return true;
    }
    let Some(name) = name else {
        return false;
    };
    if name.trim().eq_ignore_ascii_case("AgentCore-Light") {
        return false;
    }
    name_matches(name)
}

/// 扫描一次广播，返回所有像 AgentCore-Light 的设备（按名字前缀或 NUS service 判断）。
///
/// 需要拔掉 USB：插着线时设备不广播，扫不到是正常的。
pub fn scan(timeout: Duration) -> Result<Vec<BleDiscovery>, String> {
    let runtime = runtime()?;
    runtime.block_on(async move {
        let adapter = first_adapter().await?;
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|error| format!("failed to start BLE scan: {error}"))?;
        tokio::time::sleep(timeout).await;
        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|error| format!("failed to list BLE peripherals: {error}"))?;
        let _ = adapter.stop_scan().await;

        let mut found = Vec::new();
        for peripheral in peripherals {
            let properties = peripheral.properties().await.ok().flatten();
            let name = properties
                .as_ref()
                .and_then(|properties| {
                    properties
                        .local_name
                        .clone()
                        .or_else(|| properties.advertisement_name.clone())
                })
                .filter(|name| !name.trim().is_empty());
            let advertises_nus = properties
                .as_ref()
                .is_some_and(|properties| properties.services.contains(&NUS_SERVICE));
            if !looks_like_ble(name.as_deref(), advertises_nus) {
                continue;
            }
            found.push(BleDiscovery {
                address: peripheral.address().to_string(),
                name,
            });
        }
        Ok(found)
    })
}

/// 一次 BLE 连接：把 `Transport` 的同步接口桥接到后台线程里的 btleplug async 调用。
pub struct BleTransport {
    commands: UnboundedSender<Command>,
    lines: Arc<Mutex<Vec<String>>>,
    connected: Arc<AtomicBool>,
    address: String,
    worker: Option<std::thread::JoinHandle<()>>,
}

enum Command {
    /// 写入一整行（调用方已在末尾带 `\n`），结果通过 channel 回传。
    Write(Vec<u8>, SyncSender<Result<(), String>>),
    Shutdown,
}

impl BleTransport {
    /// 连接指定地址（或广播名前缀匹配的第一台）。
    ///
    /// `selector` 可以是 `aa:bb:cc:dd:ee:ff`、`ble:aa:bb:...`，或 `name:AgentCore-Light-BLE`。
    pub fn open(selector: &str) -> Result<Self, String> {
        let selector = selector.trim().to_string();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let connected = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = sync_channel::<Result<String, String>>(1);
        let (commands, command_rx) = unbounded_channel();

        let worker_lines = lines.clone();
        let worker_connected = connected.clone();
        let worker = std::thread::Builder::new()
            .name("agentcore-ble".to_string())
            .spawn(move || {
                run_worker(
                    selector,
                    command_rx,
                    worker_lines,
                    worker_connected,
                    ready_tx,
                )
            })
            .map_err(|error| format!("failed to start BLE worker: {error}"))?;

        match ready_rx.recv_timeout(CONNECT_TIMEOUT + SCAN_TIMEOUT) {
            Ok(Ok(address)) => Ok(Self {
                commands,
                lines,
                connected,
                address,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(RecvTimeoutError::Timeout) => Err(format!(
                "BLE connection timed out after {} s; the device only advertises while unplugged from USB",
                (CONNECT_TIMEOUT + SCAN_TIMEOUT).as_secs()
            )),
            Err(RecvTimeoutError::Disconnected) => Err("BLE worker stopped unexpectedly".to_string()),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

impl Transport for BleTransport {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        if !self.is_connected() {
            return Err(format!("BLE link to {} is down", self.address));
        }
        let (reply_tx, reply_rx) = sync_channel(1);
        self.commands
            .send(Command::Write(bytes.to_vec(), reply_tx))
            .map_err(|_| "BLE worker is gone".to_string())?;
        // 等一次 BLE 写入的确认；`write()` 是同步接口，这里短暂阻塞调用方。
        match reply_rx.recv_timeout(WRITE_TIMEOUT) {
            Ok(result) => result,
            Err(_) => Err("BLE write timed out".to_string()),
        }
    }

    fn describe(&self) -> String {
        format!("ble:{}", self.address)
    }

    fn drain_lines(&mut self) -> Vec<String> {
        self.lines
            .lock()
            .map(|mut lines| std::mem::take(&mut *lines))
            .unwrap_or_default()
    }
}

impl Drop for BleTransport {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            // 后台线程会在退出前 `disconnect()`；这里等它收尾，避免连接泄漏。
            let _ = worker.join();
        }
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to create BLE runtime: {error}"))
}

async fn first_adapter() -> Result<Adapter, String> {
    let manager = Manager::new()
        .await
        .map_err(|error| format!("Bluetooth is unavailable: {error}"))?;
    let mut adapters = manager
        .adapters()
        .await
        .map_err(|error| format!("failed to list Bluetooth adapters: {error}"))?;
    if adapters.is_empty() {
        return Err("no Bluetooth adapter found".to_string());
    }
    Ok(adapters.remove(0))
}

/// 扫描并连接目标设备，返回 (peripheral, RX 特征, TX 特征, 实际地址)。
async fn connect(
    selector: &str,
) -> Result<(Peripheral, Characteristic, Characteristic, String), String> {
    let adapter = first_adapter().await?;
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|error| format!("failed to start BLE scan: {error}"))?;

    let deadline = Instant::now() + SCAN_TIMEOUT;
    let mut target = None;
    while Instant::now() < deadline {
        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|error| format!("failed to list BLE peripherals: {error}"))?;
        // 名字匹配可能命中同一台设备的经典广播；优先挑广播了 NUS service 的那条。
        let mut fallback = None;
        for peripheral in peripherals {
            if !matches_selector(&peripheral, selector).await {
                continue;
            }
            let advertises_nus = peripheral
                .properties()
                .await
                .ok()
                .flatten()
                .is_some_and(|properties| properties.services.contains(&NUS_SERVICE));
            if advertises_nus {
                target = Some(peripheral);
                break;
            }
            fallback.get_or_insert(peripheral);
        }
        if target.is_none() {
            target = fallback;
        }
        if target.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let _ = adapter.stop_scan().await;

    let peripheral = target.ok_or_else(|| {
        format!(
            "{selector} not found in BLE scan; the device stops advertising while USB is plugged in"
        )
    })?;
    peripheral
        .connect_with_timeout(CONNECT_TIMEOUT)
        .await
        .map_err(|error| format!("failed to connect to {}: {error}", selector))?;
    peripheral
        .discover_services_with_timeout(CONNECT_TIMEOUT)
        .await
        .map_err(|error| format!("failed to discover BLE services: {error}"))?;

    let characteristics: BTreeSet<Characteristic> = peripheral.characteristics();
    let rx = characteristics
        .iter()
        .find(|characteristic| characteristic.uuid == NUS_RX)
        .cloned()
        .ok_or_else(|| "device has no NUS RX characteristic".to_string())?;
    let tx = characteristics
        .iter()
        .find(|characteristic| characteristic.uuid == NUS_TX)
        .cloned()
        .ok_or_else(|| "device has no NUS TX characteristic".to_string())?;
    peripheral
        .subscribe(&tx)
        .await
        .map_err(|error| format!("failed to subscribe to NUS TX: {error}"))?;
    let address = peripheral.address().to_string().to_lowercase();
    Ok((peripheral, rx, tx, address))
}

async fn matches_selector(peripheral: &Peripheral, selector: &str) -> bool {
    let selector = selector.trim();
    if let Some(name) = selector.strip_prefix("name:") {
        let name = name.to_lowercase();
        let local = peripheral
            .properties()
            .await
            .ok()
            .flatten()
            .and_then(|properties| properties.local_name.or(properties.advertisement_name))
            .unwrap_or_default()
            .to_lowercase();
        return local.starts_with(&name) || name_matches(&local);
    }
    let address = selector
        .strip_prefix("ble:")
        .unwrap_or(selector)
        .to_lowercase();
    peripheral.address().to_string().to_lowercase() == address
}

/// 后台线程：连接、订阅通知、按命令写入。
fn run_worker(
    selector: String,
    mut commands: UnboundedReceiver<Command>,
    lines: Arc<Mutex<Vec<String>>>,
    connected: Arc<AtomicBool>,
    ready: SyncSender<Result<String, String>>,
) {
    let Ok(runtime) = runtime() else {
        let _ = ready.send(Err("failed to create BLE runtime".to_string()));
        return;
    };
    runtime.block_on(async move {
        let (peripheral, rx, tx, address) = match connect(&selector).await {
            Ok(parts) => parts,
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };
        let mut notifications = match peripheral.notifications().await {
            Ok(stream) => stream,
            Err(error) => {
                let _ = ready.send(Err(format!("failed to read NUS notifications: {error}")));
                let _ = peripheral.disconnect().await;
                return;
            }
        };
        connected.store(true, Ordering::Relaxed);
        let _ = ready.send(Ok(address));

        let chunk_size = write_chunk_size(&peripheral);
        let write_type = write_type(&rx);
        let mut buffer = Vec::new();
        // BlueZ 会把同一条通知投递两次；短窗口内的重复行丢掉。
        let mut last_line: Option<(String, Instant)> = None;

        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    None | Some(Command::Shutdown) => break,
                    Some(Command::Write(bytes, reply)) => {
                        let result = write_all(&peripheral, &rx, &bytes, chunk_size, write_type).await;
                        if result.is_err() {
                            connected.store(false, Ordering::Relaxed);
                        }
                        let _ = reply.send(result);
                    }
                },
                notification = notifications.next() => {
                    let Some(notification) = notification else {
                        // 设备断链时通知流会结束。
                        connected.store(false, Ordering::Relaxed);
                        break;
                    };
                    let new_lines = split_lines(&mut buffer, &notification.value);
                    if !new_lines.is_empty() {
                        if let Ok(mut shared) = lines.lock() {
                            shared.extend(new_lines.into_iter().filter(|line| {
                                let duplicate = last_line.as_ref().is_some_and(|(previous, at)| {
                                    previous == line && at.elapsed() < DUPLICATE_WINDOW
                                });
                                if !duplicate {
                                    last_line = Some((line.clone(), Instant::now()));
                                }
                                !duplicate
                            }));
                            if shared.len() > 200 {
                                let excess = shared.len() - 200;
                                shared.drain(..excess);
                            }
                        }
                    }
                }
            }
        }

        connected.store(false, Ordering::Relaxed);
        let _ = peripheral.unsubscribe(&tx).await;
        let _ = peripheral.disconnect().await;
    });
}

fn write_chunk_size(peripheral: &Peripheral) -> usize {
    // `mtu()` 在部分平台连上之前是 0；默认按 20 字节分片。
    let mtu = usize::from(peripheral.mtu());
    if mtu > DEFAULT_CHUNK + 3 {
        mtu - 3
    } else {
        DEFAULT_CHUNK
    }
}

fn write_type(characteristic: &Characteristic) -> WriteType {
    if characteristic
        .properties
        .contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
    {
        WriteType::WithoutResponse
    } else {
        // 链路不支持无响应写时降级为带响应写（协议文档 §2.3）。
        WriteType::WithResponse
    }
}

async fn write_all(
    peripheral: &Peripheral,
    characteristic: &Characteristic,
    bytes: &[u8],
    chunk_size: usize,
    write_type: WriteType,
) -> Result<(), String> {
    for chunk in bytes.chunks(chunk_size.max(1)) {
        peripheral
            .write(characteristic, chunk, write_type)
            .await
            .map_err(|error| format!("BLE write failed: {error}"))?;
        if write_type == WriteType::WithResponse {
            // 带响应写需要给设备留一点处理时间。
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nus_uuids_match_the_protocol_document() {
        assert_eq!(
            NUS_SERVICE.to_string(),
            "6e400001-b5a3-f393-e0a9-e50e24dcca9e"
        );
        assert_eq!(NUS_RX.to_string(), "6e400002-b5a3-f393-e0a9-e50e24dcca9e");
        assert_eq!(NUS_TX.to_string(), "6e400003-b5a3-f393-e0a9-e50e24dcca9e");
    }

    #[test]
    fn recognises_device_names() {
        assert!(name_matches("AgentCore-Light-BLE"));
        assert!(name_matches("agentcore-light-desk"));
        assert!(!name_matches("Mi Band"));
        assert!(!name_matches(""));
    }

    #[test]
    fn skips_the_classic_advertisement_of_the_same_device() {
        // 实测：经典蓝牙那条正好叫 AgentCore-Light，连上也没有 NUS 特征。
        assert!(!looks_like_ble(Some("AgentCore-Light"), false));
        assert!(looks_like_ble(Some("AgentCore-Light-BLE"), false));
        assert!(looks_like_ble(Some("AgentCore-Light-Desk"), false));
        assert!(looks_like_ble(Some("AgentCore-Light"), true));
        assert!(!looks_like_ble(Some("Mi Band"), false));
        assert!(!looks_like_ble(None, false));
    }

    #[test]
    fn device_id_is_address_scoped() {
        let discovery = BleDiscovery {
            address: "AA:BB:CC:DD:EE:FF".to_string(),
            name: Some("AgentCore-Light-BLE".to_string()),
        };
        assert_eq!(discovery.device_id(), "ble:aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn scan_without_bluetooth_reports_an_error_instead_of_panicking() {
        // 无蓝牙适配器的环境（CI）应当返回 Err。
        match scan(Duration::from_millis(100)) {
            Ok(devices) => {
                for device in devices {
                    assert!(
                        looks_like_ble(device.name.as_deref(), false),
                        "只有 BLE 链路才算 AgentCore-Light：{device:?}"
                    );
                }
            }
            Err(error) => assert!(!error.is_empty()),
        }
    }
}