//! 蓝牙 SPP（RFCOMM）直连链路。
//!
//! 与「系统里的蓝牙串口节点」这条路不同：这里直接对设备的**经典蓝牙地址**开一个
//! RFCOMM socket，因此
//!
//! * 不需要 `sudo rfcomm bind`（那一步要走 `RFCOMMCREATEDEV` ioctl，内核里检查
//!   `CAP_NET_ADMIN`），普通用户即可；
//! * 不依赖 `/dev/rfcommN` 节点存在，也就不依赖 udev/BlueZ 的 serial profile；
//! * 不依赖设备广播——只要设备**配对过**就能连，正好补上「已连接/已配对设备不广播，
//!   BLE 扫描扫不到」这个缺口。
//!
//! LF/CRLF 与 `HELLO` 握手等文本协议细节由 `AgentCoreTransport` 复用（协议文档 §2.2）。
//!
//! 目前只在 Linux 上实现（`AF_BLUETOOTH` / `BTPROTO_RFCOMM`）。其它平台返回明确的
//! 不支持错误，调用方可以退回到串口或 BLE。

use std::fmt;

/// SPP 连接目标：经典蓝牙地址 + RFCOMM 通道号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SppTarget {
    /// 大写、带冒号的地址，例如 `AF:C3:A4:E1:D8:95`。
    pub address: String,
    /// RFCOMM 通道号。实测该设备是 2（通道 1 返回 `ECONNREFUSED`）。
    pub channel: u8,
}

/// 实测通道号（协议文档 §2.2）。
pub const DEFAULT_CHANNEL: u8 = 2;

impl fmt::Display for SppTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.address, self.channel)
    }
}

/// 解析 `AA:BB:CC:DD:EE:FF`（默认通道）或 `AA:BB:CC:DD:EE:FF:<channel>`。
pub fn parse_target(text: &str) -> Result<SppTarget, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty SPP target".to_string());
    }
    let mut parts: Vec<&str> = text.split(':').collect();
    let mut channel = DEFAULT_CHANNEL;
    if parts.len() == 7 {
        let last = parts.pop().unwrap_or_default();
        channel = last
            .parse::<u8>()
            .map_err(|_| format!("invalid RFCOMM channel in {text}"))?;
        if channel == 0 {
            return Err(format!("invalid RFCOMM channel in {text}"));
        }
    }
    if parts.len() != 6 {
        return Err(format!(
            "invalid Bluetooth address {text}; expected AA:BB:CC:DD:EE:FF[:channel]"
        ));
    }
    let mut bytes = [0u8; 6];
    for (index, part) in parts.iter().enumerate() {
        if part.len() != 2 {
            return Err(format!("invalid Bluetooth address {text}"));
        }
        bytes[index] = u8::from_str_radix(part, 16)
            .map_err(|_| format!("invalid Bluetooth address {text}"))?;
    }
    Ok(SppTarget {
        address: format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]
        ),
        channel,
    })
}

/// 该平台是否支持免 root 的 RFCOMM 直连。
pub const fn supported() -> bool {
    cfg!(target_os = "linux")
}

#[cfg(target_os = "linux")]
pub use linux::{open, RfcommLink};

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    use super::{parse_target, SppTarget};

    /// Linux `struct sockaddr_rc`（libc 里没有，按内核头文件定义）。
    #[repr(C)]
    struct SockaddrRc {
        rc_family: libc::sa_family_t,
        rc_bdaddr: [u8; 6],
        rc_channel: u8,
    }

    const AF_BLUETOOTH: libc::c_int = 31;
    const BTPROTO_RFCOMM: libc::c_int = 3;
    /// 读超时：让读线程能定期检查退出标志，也不至于把 `HELLO` 应答等丢。
    const READ_TIMEOUT: libc::timeval = libc::timeval {
        tv_sec: 0,
        tv_usec: 100_000,
    };

    /// 一条已建立的 RFCOMM 链路（`Read`/`Write` 直接对应字节流）。
    pub struct RfcommLink {
        fd: OwnedFd,
        target: SppTarget,
    }

    impl RfcommLink {
        pub fn target(&self) -> &SppTarget {
            &self.target
        }
    }

    fn io_error() -> io::Error {
        io::Error::last_os_error()
    }

    /// 连接 `target`（地址或 `地址:通道`），返回一条 RFCOMM 字节流。
    pub fn open(target: &str) -> Result<RfcommLink, String> {
        let target = parse_target(target)?;
        // BlueZ/kernel 使用「反序」存放的 BD_ADDR（`str2ba()` 就是这个语义）。
        let mut bytes: Vec<&str> = target.address.split(':').collect();
        bytes.reverse();
        let mut bdaddr = [0u8; 6];
        for (index, part) in bytes.iter().enumerate() {
            bdaddr[index] = u8::from_str_radix(part, 16)
                .map_err(|_| format!("invalid Bluetooth address {}", target.address))?;
        }

        // SAFETY: 参数与返回值都按 POSIX socket 语义使用；失败时读 errno。
        let raw = unsafe { libc::socket(AF_BLUETOOTH, libc::SOCK_STREAM, BTPROTO_RFCOMM) };
        if raw < 0 {
            return Err(format!("failed to create RFCOMM socket: {}", io_error()));
        }
        // SAFETY: `raw` 刚由 socket() 返回，所有权转交 OwnedFd，后续不再单独 close。
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };

        // SAFETY: 设置本 socket 的读超时，参数指向本函数栈上的 timeval。
        let applied = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                &READ_TIMEOUT as *const libc::timeval as *const libc::c_void,
                std::mem::size_of::<libc::timeval>() as libc::socklen_t,
            )
        };
        if applied < 0 {
            return Err(format!("failed to configure RFCOMM socket: {}", io_error()));
        }

        let address = SockaddrRc {
            rc_family: AF_BLUETOOTH as libc::sa_family_t,
            rc_bdaddr: bdaddr,
            rc_channel: target.channel,
        };
        // SAFETY: connect 只读取 `address`，长度按 sockaddr_rc 传入。
        let connected = unsafe {
            libc::connect(
                fd.as_raw_fd(),
                &address as *const SockaddrRc as *const libc::sockaddr,
                std::mem::size_of::<SockaddrRc>() as libc::socklen_t,
            )
        };
        if connected < 0 {
            return Err(format!(
                "failed to connect to SPP {}: {}",
                target,
                io_error()
            ));
        }

        Ok(RfcommLink { fd, target })
    }

    impl Read for RfcommLink {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            // SAFETY: 读取至少 buffer.len() 字节的可用空间。
            let count = unsafe {
                libc::read(
                    self.fd.as_raw_fd(),
                    buffer.as_mut_ptr() as *mut _,
                    buffer.len(),
                )
            };
            if count < 0 {
                let error = io::Error::last_os_error();
                // 读超时在 Linux 上表现为 EAGAIN/EWOULDBLOCK，统一按 TimedOut 上抛。
                if error.raw_os_error() == Some(libc::EAGAIN) {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, error));
                }
                return Err(error);
            }
            Ok(count as usize)
        }
    }

    impl crate::transport::LineLink for RfcommLink {
        fn try_clone_link(&self) -> Result<Self, String> {
            // SAFETY: dup 返回新的 fd，所有权转交 OwnedFd。
            let duplicated = unsafe { libc::dup(self.fd.as_raw_fd()) };
            if duplicated < 0 {
                return Err(format!(
                    "failed to duplicate RFCOMM socket: {}",
                    io::Error::last_os_error()
                ));
            }
            Ok(Self {
                // SAFETY: `duplicated` 是刚由 dup() 返回的有效 fd。
                fd: unsafe { OwnedFd::from_raw_fd(duplicated) },
                target: self.target.clone(),
            })
        }

        fn label(&self) -> String {
            format!("spp:{}", self.target)
        }
    }

    impl Write for RfcommLink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            // SAFETY: 发送 bytes[..len]；MSG_NOSIGNAL 避免对端断开时进程被 SIGPIPE 杀掉。
            let count = unsafe {
                libc::send(
                    self.fd.as_raw_fd(),
                    bytes.as_ptr() as *const _,
                    bytes.len(),
                    libc::MSG_NOSIGNAL,
                )
            };
            if count < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(count as usize)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub use unsupported::{open, RfcommLink};

#[cfg(not(target_os = "linux"))]
mod unsupported {
    use std::io::{self, Read, Write};

    /// 非 Linux 平台占位类型：`open()` 永远失败，调用方退回串口/BLE。
    pub struct RfcommLink;

    pub fn open(target: &str) -> Result<RfcommLink, String> {
        Err(format!(
            "direct SPP (RFCOMM socket) is not implemented on this platform; \
             pair the light and use its Bluetooth serial port instead (target was {target})"
        ))
    }

    impl Read for RfcommLink {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("RFCOMM unsupported"))
        }
    }

    impl Write for RfcommLink {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("RFCOMM unsupported"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl crate::transport::LineLink for RfcommLink {
    fn try_clone_link(&self) -> Result<Self, String> {
        Err("RFCOMM unsupported".to_string())
    }

    fn label(&self) -> String {
        "spp:unsupported".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_address_with_default_channel() {
        let target = parse_target("af:c3:a4:e1:d8:95").unwrap();
        assert_eq!(target.address, "AF:C3:A4:E1:D8:95");
        assert_eq!(target.channel, DEFAULT_CHANNEL);
    }

    #[test]
    fn parses_explicit_channel() {
        let target = parse_target("AF:C3:A4:E1:D8:95:3").unwrap();
        assert_eq!(target.channel, 3);
        assert_eq!(target.to_string(), "AF:C3:A4:E1:D8:95:3");
    }

    #[test]
    fn rejects_invalid_addresses() {
        for input in ["", "AF:C3:A4", "AF:C3:A4:E1:D8:95:0", "AF:C3:A4:E1:D8:ZZ"] {
            assert!(parse_target(input).is_err(), "{input} should be rejected");
        }
    }
}
