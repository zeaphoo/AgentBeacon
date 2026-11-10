//! 物理传输：串口、AgentCore-Light 行协议链路与 YH50 的 USB Bulk 直连。

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(not(windows))]
use rusb::UsbContext;

pub trait Transport: Send {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String>;
    fn describe(&self) -> String;

    /// 取走设备主动上报、还没被消费的行（只有文本行协议设备实现）。
    ///
    /// 默认返回空：二进制协议设备（MLight / YH50）没有上报。
    fn drain_lines(&mut self) -> Vec<String> {
        Vec::new()
    }
}

pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
    name: String,
}

impl SerialTransport {
    pub fn open(path: &str, baud: u32) -> Result<Self, String> {
        let port = serialport::new(path, baud)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(1000))
            .open()
            .map_err(|error| format!("failed to open serial port {path}: {error}"))?;
        Ok(Self {
            port,
            name: path.to_string(),
        })
    }
}

impl Transport for SerialTransport {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.port
            .write_all(bytes)
            .map_err(|error| format!("failed to write serial port {}: {error}", self.name))?;
        self.port
            .flush()
            .map_err(|error| format!("failed to flush serial port {}: {error}", self.name))
    }

    fn describe(&self) -> String {
        format!("serial:{}", self.name)
    }
}

/// 把字节流切成行：`\n` 结尾，读端兼容 `\r\n`。
pub(crate) fn split_lines(buffer: &mut Vec<u8>, chunk: &[u8]) -> Vec<String> {
    buffer.extend_from_slice(chunk);
    let mut lines = Vec::new();
    while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
        let line: Vec<u8> = buffer.drain(..=position).collect();
        let text = String::from_utf8_lossy(&line[..line.len() - 1])
            .trim_end_matches('\r')
            .to_string();
        if !text.is_empty() {
            lines.push(text);
        }
    }
    // 半行留着等下一次读取，避免把 `HELLO:OK` 或 `CHG:` 截成两半。
    if buffer.len() > 64 * 1024 {
        buffer.clear();
    }
    lines
}

/// 文本行链路底下的字节流：串口（USB CDC / 蓝牙串口）或 RFCOMM socket。
///
/// 抽出来是为了让 `AgentCoreTransport` 的握手、分行、上报读线程在三种链路上完全复用：
/// USB CDC 需要 DTR/RTS 与 1.5 s 就绪等待，RFCOMM socket 不需要（`prepare` 默认空实现）。
pub trait LineLink: Read + Write + Send + Sized + 'static {
    /// 复制一个句柄给后台读线程用（串口 `try_clone()` / socket `dup()`）。
    fn try_clone_link(&self) -> Result<Self, String>;

    /// 打开链路后、握手前的准备动作。
    fn prepare(&mut self) -> Result<(), String> {
        Ok(())
    }

    /// 展示用标签，例如 `serial:/dev/ttyACM0`、`spp:AF:C3:A4:E1:D8:95:2`。
    fn label(&self) -> String;
}

/// 串口链路：USB CDC 虚拟串口，或系统为蓝牙 SPP 建出来的串口节点。
pub struct SerialLink {
    port: Box<dyn serialport::SerialPort>,
    name: String,
}

impl SerialLink {
    pub fn open(path: &str) -> Result<Self, String> {
        let port = serialport::new(path, crate::agentcore::BAUD_RATE)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|error| format!("failed to open {}: {error}", path))?;
        Ok(Self {
            port,
            name: path.to_string(),
        })
    }
}

impl Read for SerialLink {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.port.read(buffer)
    }
}

impl Write for SerialLink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.port.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.port.flush()
    }
}

impl LineLink for SerialLink {
    fn try_clone_link(&self) -> Result<Self, String> {
        let port = self
            .port
            .try_clone()
            .map_err(|error| format!("failed to clone {}: {error}", self.name))?;
        Ok(Self {
            port,
            name: self.name.clone(),
        })
    }

    fn prepare(&mut self) -> Result<(), String> {
        // USB CDC 需要 DTR 才算真正“打开”；RTS 保持低电平。
        let _ = self.port.write_data_terminal_ready(true);
        let _ = self.port.write_request_to_send(false);
        std::thread::sleep(Duration::from_millis(crate::agentcore::CDC_SETTLE_MS));
        let _ = self.port.clear(serialport::ClearBuffer::Input);
        Ok(())
    }

    fn label(&self) -> String {
        format!("serial:{}", self.name)
    }
}

/// AgentCore-Light 传输：USB CDC / 蓝牙串口 / SPP(RFCOMM) / BLE 之外的文本行协议链路。
///
/// 打开后按协议要求（CDC 还要置 `DTR=1 / RTS=0`、等 1.5 s、清缓冲）发 `HELLO` 并等
/// `HELLO:OK`（1.5 s 超时）。握手失败直接报错，不会把别的蓝牙串口当成灯。
///
/// 设备每 5 s 主动发一次 `CHG:`，必须有人读走，否则系统缓冲会积压。这里用
/// `try_clone_link()` 起一个后台读线程，把整行放进共享队列，由 `drain_lines` 取走。
pub struct AgentCoreTransport<L: LineLink = SerialLink> {
    link: L,
    lines: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
    name: String,
}

impl<L: LineLink> AgentCoreTransport<L> {
    /// 在任意 `LineLink` 上打开链路并完成 `HELLO` 握手。
    pub fn open_link(mut link: L, name: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        link.prepare()?;

        link.write_all(crate::agentcore::HANDSHAKE.as_bytes())
            .and_then(|()| link.write_all(b"\n"))
            .and_then(|()| link.flush())
            .map_err(|error| format!("failed to send HELLO to {}: {error}", name))?;

        let deadline =
            Instant::now() + Duration::from_millis(crate::agentcore::HANDSHAKE_TIMEOUT_MS);
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 256];
        let mut handshake: Option<String> = None;
        while Instant::now() < deadline {
            match link.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    // SPP 下可能先冒出 `STATUS:`/`CHG:` 之类，跳过继续等握手。
                    for line in split_lines(&mut buffer, &chunk[..count]) {
                        if crate::agentcore::parse_report(&line) == crate::agentcore::Report::Hello
                        {
                            handshake = Some(line);
                            break;
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => return Err(format!("failed to read from {}: {error}", name)),
            }
            if handshake.is_some() {
                break;
            }
        }
        if handshake.is_none() {
            return Err(format!(
                "{} did not answer HELLO within {} ms; not an AgentCore-Light",
                name,
                crate::agentcore::HANDSHAKE_TIMEOUT_MS
            ));
        }

        let lines = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let mut reader_link = link.try_clone_link()?;
            let lines = lines.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("agentcore-reader".to_string())
                .spawn(move || {
                    let mut buffer = Vec::new();
                    let mut chunk = [0u8; 512];
                    while !stop.load(Ordering::Relaxed) {
                        match reader_link.read(&mut chunk) {
                            Ok(0) => std::thread::sleep(Duration::from_millis(50)),
                            Ok(count) => {
                                let new_lines = split_lines(&mut buffer, &chunk[..count]);
                                if new_lines.is_empty() {
                                    continue;
                                }
                                if let Ok(mut shared) = lines.lock() {
                                    shared.extend(new_lines);
                                    // 没人消费时不要无限增长；电量心跳最多留最近 200 行。
                                    if shared.len() > 200 {
                                        let excess = shared.len() - 200;
                                        shared.drain(..excess);
                                    }
                                }
                            }
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                                ) => {}
                            Err(_) => break,
                        }
                    }
                })
                .map_err(|error| format!("failed to start reader thread: {error}"))?
        };

        Ok(Self {
            link,
            lines,
            stop,
            reader: Some(reader),
            name,
        })
    }
}

impl AgentCoreTransport<SerialLink> {
    /// 打开串口链路（USB CDC 或系统蓝牙串口节点）。
    pub fn open(path: &str) -> Result<Self, String> {
        let link = SerialLink::open(path)?;
        Self::open_link(link, path)
    }
}

impl AgentCoreTransport<crate::spp::RfcommLink> {
    /// 免 root 的 SPP 直连：`AA:BB:CC:DD:EE:FF[:channel]`。
    pub fn open_spp(target: &str) -> Result<Self, String> {
        let link = crate::spp::open(target)?;
        let label = format!("spp:{target}");
        Self::open_link(link, label)
    }
}

impl<L: LineLink> Transport for AgentCoreTransport<L> {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.link
            .write_all(bytes)
            .map_err(|error| format!("failed to write {}: {error}", self.name))?;
        self.link
            .flush()
            .map_err(|error| format!("failed to flush {}: {error}", self.name))
    }

    fn describe(&self) -> String {
        format!("agentcore:{}", self.name)
    }

    fn drain_lines(&mut self) -> Vec<String> {
        self.lines
            .lock()
            .map(|mut lines| std::mem::take(&mut *lines))
            .unwrap_or_default()
    }
}

impl<L: LineLink> Drop for AgentCoreTransport<L> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
    }
}

/// AgentCore-Light 的一次握手探测结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentCoreIdentity {
    pub firmware: Option<String>,
    pub state: Option<String>,
    pub brightness_level: Option<u16>,
}

/// 打开链路并握手，确认这是 AgentCore-Light；用于蓝牙等“端口名不可靠”的场景。
pub fn probe_agentcore(path: &str) -> Result<AgentCoreIdentity, String> {
    query_identity(AgentCoreTransport::open(path)?, path)
}

/// 同上，但走免 root 的 SPP(RFCOMM) 直连。
pub fn probe_agentcore_spp(target: &str) -> Result<AgentCoreIdentity, String> {
    query_identity(AgentCoreTransport::open_spp(target)?, target)
}

fn query_identity<L: LineLink>(
    mut transport: AgentCoreTransport<L>,
    name: &str,
) -> Result<AgentCoreIdentity, String> {
    let mut identity = AgentCoreIdentity::default();
    // 设备在握手后会先推一条 STATUS；读不到也不算失败。
    transport
        .write(b"STATUS\n")
        .map_err(|error| format!("failed to query STATUS on {name}: {error}"))?;
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        for line in transport.drain_lines() {
            if let crate::agentcore::Report::Status(status) = crate::agentcore::parse_report(&line)
            {
                identity.firmware = status.firmware;
                identity.state = status.state;
                identity.brightness_level = status.brightness_level;
                return Ok(identity);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(identity)
}

/// YH50 传输：Linux/macOS 先尝试 libusb 直接 Bulk，失败回退串口；Windows 仅串口。
pub struct Yh50Transport {
    inner: Inner,
}

enum Inner {
    Serial(SerialTransport),
    #[cfg(not(windows))]
    Usb(UsbBulk),
}

impl Yh50Transport {
    pub fn open(path: &str) -> Result<Self, String> {
        #[cfg(not(windows))]
        {
            match UsbBulk::open() {
                Ok(usb) => Ok(Self {
                    inner: Inner::Usb(usb),
                }),
                Err(usb_error) => {
                    let serial = SerialTransport::open(path, 9600).map_err(|serial_error| {
                        format!("USB direct connection failed ({usb_error}); serial fallback also failed ({serial_error})")
                    })?;
                    Ok(Self {
                        inner: Inner::Serial(serial),
                    })
                }
            }
        }
        #[cfg(windows)]
        {
            SerialTransport::open(path, 9600).map(|serial| Self {
                inner: Inner::Serial(serial),
            })
        }
    }
}

impl Transport for Yh50Transport {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        match &mut self.inner {
            Inner::Serial(serial) => serial.write(bytes),
            #[cfg(not(windows))]
            Inner::Usb(usb) => usb.write(bytes),
        }
    }

    fn describe(&self) -> String {
        match &self.inner {
            Inner::Serial(serial) => serial.describe(),
            #[cfg(not(windows))]
            Inner::Usb(_) => "usb:1A86:7523".to_string(),
        }
    }
}

#[cfg(not(windows))]
struct UsbBulk {
    handle: rusb::DeviceHandle<rusb::Context>,
    endpoint: u8,
}

#[cfg(not(windows))]
impl UsbBulk {
    fn open() -> Result<Self, String> {
        let context = rusb::Context::new()
            .map_err(|error| format!("libusb initialization failed: {error}"))?;
        let devices = context
            .devices()
            .map_err(|error| format!("libusb device enumeration failed: {error}"))?;
        for device in devices.iter() {
            let descriptor = match device.device_descriptor() {
                Ok(descriptor) => descriptor,
                Err(_) => continue,
            };
            if descriptor.vendor_id() != 0x1A86 || descriptor.product_id() != 0x7523 {
                continue;
            }
            let endpoint = find_bulk_out(&device)
                .ok_or_else(|| "YH50 has no available Bulk OUT endpoint".to_string())?;
            let handle = device
                .open()
                .map_err(|error| format!("failed to open YH50: {error}"))?;
            let _ = handle.set_active_configuration(1);
            handle
                .claim_interface(0)
                .map_err(|error| format!("failed to claim YH50 interface: {error}"))?;
            return Ok(Self { handle, endpoint });
        }
        Err("YH50 device not found (1A86:7523)".to_string())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        let written = self
            .handle
            .write_bulk(self.endpoint, bytes, Duration::from_millis(1000))
            .map_err(|error| format!("YH50 bulk write failed: {error}"))?;
        if written != bytes.len() {
            return Err(format!(
                "incomplete YH50 write: {written}/{} bytes",
                bytes.len()
            ));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
impl Drop for UsbBulk {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(0);
    }
}

#[cfg(not(windows))]
fn find_bulk_out(device: &rusb::Device<rusb::Context>) -> Option<u8> {
    let config = device.config_descriptor(0).ok()?;
    let interface = config.interfaces().next()?;
    let descriptor = interface.descriptors().next()?;
    descriptor
        .endpoint_descriptors()
        .find(|endpoint| {
            endpoint.transfer_type() == rusb::TransferType::Bulk && endpoint.address() & 0x80 == 0
        })
        .map(|endpoint| endpoint.address())
}
