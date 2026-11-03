//! 物理传输：串口与 YH50 的 USB Bulk 直连。

use std::io::Write as _;
use std::time::Duration;

#[cfg(not(windows))]
use rusb::UsbContext;

pub trait Transport: Send {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String>;
    fn describe(&self) -> String;
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
            .map_err(|error| format!("打开串口 {path} 失败：{error}"))?;
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
            .map_err(|error| format!("写入串口 {} 失败：{error}", self.name))?;
        self.port
            .flush()
            .map_err(|error| format!("刷新串口 {} 失败：{error}", self.name))
    }

    fn describe(&self) -> String {
        format!("serial:{}", self.name)
    }
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
                        format!("USB 直连失败（{usb_error}），串口回退也失败（{serial_error}）")
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
        let context =
            rusb::Context::new().map_err(|error| format!("libusb 初始化失败：{error}"))?;
        let devices = context
            .devices()
            .map_err(|error| format!("libusb 枚举设备失败：{error}"))?;
        for device in devices.iter() {
            let descriptor = match device.device_descriptor() {
                Ok(descriptor) => descriptor,
                Err(_) => continue,
            };
            if descriptor.vendor_id() != 0x1A86 || descriptor.product_id() != 0x7523 {
                continue;
            }
            let endpoint = find_bulk_out(&device)
                .ok_or_else(|| "YH50 没有可用的 Bulk OUT 端点".to_string())?;
            let handle = device
                .open()
                .map_err(|error| format!("打开 YH50 失败：{error}"))?;
            let _ = handle.set_active_configuration(1);
            handle
                .claim_interface(0)
                .map_err(|error| format!("占用 YH50 接口失败：{error}"))?;
            return Ok(Self { handle, endpoint });
        }
        Err("未找到 YH50 设备（1A86:7523）".to_string())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        let written = self
            .handle
            .write_bulk(self.endpoint, bytes, Duration::from_millis(1000))
            .map_err(|error| format!("YH50 Bulk 写入失败：{error}"))?;
        if written != bytes.len() {
            return Err(format!("YH50 写入不完整：{written}/{} 字节", bytes.len()));
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
