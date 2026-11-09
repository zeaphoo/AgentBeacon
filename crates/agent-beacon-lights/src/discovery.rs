//! 设备发现：VID/PID 识别、序列号/端口绑定与 profile 覆盖。

use agent_beacon_core::{DeviceBinding, DeviceProfile};

pub const MLIGHT_VID: u16 = 0x2E8A;
pub const MLIGHT_PIDS: [u16; 2] = [0x0003, 0x000A];
/// 同厂商的温度显示设备，不是状态灯。
pub const MLIGHT_EXCLUDED_PID: u16 = 0x101F;
pub const YH50_VID: u16 = 0x1A86;
pub const YH50_PID: u16 = 0x7523;

/// AgentCore-Light：杰理 AC696N 的 USB CDC（早期固件是 Espressif 0x303A）。
pub const AGENTCORE_VID: u16 = 0x4C4A;
pub const AGENTCORE_PID: u16 = 0x4155;
pub const AGENTCORE_ALT_VID: u16 = 0x303A;
/// 备用 VID 下要按产品名/序列号加分，避免把同厂 ESP32 开发板当成灯。
const AGENTCORE_KEYWORDS: [&str; 3] = ["agentcore", "jieli", "ac696"];
/// 蓝牙 SPP 的端口名关键字；Windows 上还有“标准串行”之类的本地化名字。
const BLUETOOTH_PORT_KEYWORDS: [&str; 6] = [
    "bluetooth",
    "jl_spp",
    "rfcomm",
    "serial over bluetooth",
    "标准串行",
    "bth",
];

/// 端口来自哪条物理链路。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortKind {
    #[default]
    Usb,
    Bluetooth,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PortInfo {
    pub port_name: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub serial: Option<String>,
    /// 设备/接口的产品名，用于备用 VID 与蓝牙名的关键字判断。
    pub product: Option<String>,
    pub kind: PortKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedDevice {
    pub id: String,
    pub port: String,
    pub profile: DeviceProfile,
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
    /// 仅凭端口名/链路类型识别出来的设备，还要用 `HELLO` 握手确认。
    ///
    /// 蓝牙 SPP 的端口名是最不可靠的（同名的其它蓝牙串口也占 COM 口），
    /// 因此 `AgentCore-Light` 的蓝牙候选一律先探测再挂 actor。
    pub probe_required: bool,
}

/// 设备 id 优先使用 USB 序列号，避免重插后端口名变化。
pub fn device_id(info: &PortInfo) -> String {
    match info.serial.as_deref().map(str::trim) {
        Some(serial) if !serial.is_empty() => format!("serial:{serial}"),
        _ => format!("port:{}", info.port_name),
    }
}

fn lowercase(value: Option<&str>) -> String {
    value.unwrap_or_default().to_lowercase()
}

/// 产品名 / 序列号 / 端口名里是否出现 AgentCore 关键字。
fn agentcore_named(info: &PortInfo) -> bool {
    let mut haystack = String::new();
    for value in [
        info.product.as_deref(),
        info.serial.as_deref(),
        Some(info.port_name.as_str()),
    ] {
        haystack.push_str(&lowercase(value));
        haystack.push(' ');
    }
    AGENTCORE_KEYWORDS
        .iter()
        .any(|keyword| haystack.contains(keyword))
}

fn bluetooth_port_name(info: &PortInfo) -> bool {
    let name = info.port_name.to_lowercase();
    BLUETOOTH_PORT_KEYWORDS
        .iter()
        .any(|keyword| name.contains(keyword))
}

/// 端口是不是 AgentCore-Light 的候选，以及是否需要握手确认。
///
/// * 杰理 CDC（`4C4A:4155`）直接认定，不需要探测；
/// * 备用 VID（Espressif）必须命中产品名关键字；
/// * 蓝牙 SPP 端口一律是候选，但必须 `HELLO` 握手成功才算数。
pub fn classify_agentcore(info: &PortInfo) -> Option<bool> {
    let usb = matches!(info.kind, PortKind::Usb);
    if usb {
        if info.vid == Some(AGENTCORE_VID) && info.pid == Some(AGENTCORE_PID) {
            return Some(false);
        }
        if info.vid == Some(AGENTCORE_ALT_VID) && agentcore_named(info) {
            return Some(false);
        }
        return None;
    }
    if matches!(info.kind, PortKind::Bluetooth) || bluetooth_port_name(info) {
        return Some(true);
    }
    None
}

pub fn classify(info: &PortInfo) -> Option<DeviceProfile> {
    if classify_agentcore(info).is_some() {
        return Some(DeviceProfile::AgentCore);
    }
    match (info.vid?, info.pid?) {
        (MLIGHT_VID, pid) if MLIGHT_PIDS.contains(&pid) => Some(DeviceProfile::Mlight3Led),
        (YH50_VID, YH50_PID) => Some(DeviceProfile::Yh50),
        _ => None,
    }
}

fn serial_matches(binding: &DeviceBinding, info: &PortInfo) -> bool {
    let Some(serial) = binding.serial.as_deref().map(str::trim) else {
        return false;
    };
    !serial.is_empty() && info.serial.as_deref() == Some(serial)
}

fn port_matches(binding: &DeviceBinding, info: &PortInfo) -> bool {
    let Some(port) = binding.port.as_deref().map(str::trim) else {
        return false;
    };
    !port.is_empty() && info.port_name == port
}

/// 序列号绑定优先于端口名绑定；都未命中时使用自动识别结果。
pub fn resolve_profile(
    default: DeviceProfile,
    info: &PortInfo,
    bindings: &[DeviceBinding],
) -> DeviceProfile {
    bindings
        .iter()
        .find(|binding| serial_matches(binding, info))
        .or_else(|| bindings.iter().find(|binding| port_matches(binding, info)))
        .map(|binding| binding.profile)
        .unwrap_or(default)
}

/// 端口名不可靠时（Windows 的蓝牙 COM 口），允许用配置绑定把端口指成 AgentCore-Light。
///
/// 只对 AgentCore-Light 开放这条路：它必须通过 `HELLO` 握手，绑错了也不会误判成灯。
fn bound_agentcore(info: &PortInfo, bindings: &[DeviceBinding]) -> Option<DeviceProfile> {
    bindings
        .iter()
        .filter(|binding| binding.profile.is_agentcore())
        .find(|binding| serial_matches(binding, info) || port_matches(binding, info))
        .map(|binding| binding.profile)
}

/// 识别设备；`probe_required` 表示还要握手确认才能确定是 AgentCore-Light。
pub fn detect(info: &PortInfo, bindings: &[DeviceBinding]) -> Option<DetectedDevice> {
    let detected = classify(info).or_else(|| bound_agentcore(info, bindings))?;
    let profile = resolve_profile(detected, info, bindings);
    // USB VID/PID 已确认的设备不用再探测；其余（蓝牙端口、绑定端口）都要握手。
    let confirmed_usb = matches!(classify_agentcore(info), Some(false));
    let probe_required = profile.is_agentcore() && !confirmed_usb;
    Some(DetectedDevice {
        id: device_id(info),
        port: info.port_name.clone(),
        profile,
        vid: info.vid.unwrap_or_default(),
        pid: info.pid.unwrap_or_default(),
        serial: info.serial.clone(),
        probe_required,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, vid: u16, pid: u16, serial: Option<&str>) -> PortInfo {
        PortInfo {
            port_name: name.to_string(),
            vid: Some(vid),
            pid: Some(pid),
            serial: serial.map(str::to_string),
            product: None,
            kind: PortKind::Usb,
        }
    }

    fn bluetooth_port(name: &str) -> PortInfo {
        PortInfo {
            port_name: name.to_string(),
            vid: None,
            pid: None,
            serial: None,
            product: None,
            kind: PortKind::Bluetooth,
        }
    }

    fn named_port(name: &str, vid: u16, pid: u16, product: &str) -> PortInfo {
        PortInfo {
            product: Some(product.to_string()),
            ..port(name, vid, pid, None)
        }
    }

    #[test]
    fn identifies_both_light_families() {
        assert_eq!(
            classify(&port("/dev/ttyACM0", 0x2E8A, 0x0003, None)),
            Some(DeviceProfile::Mlight3Led)
        );
        assert_eq!(
            classify(&port("/dev/ttyACM0", 0x2E8A, 0x000A, None)),
            Some(DeviceProfile::Mlight3Led)
        );
        assert_eq!(
            classify(&port("/dev/ttyUSB0", 0x1A86, 0x7523, None)),
            Some(DeviceProfile::Yh50)
        );
    }

    #[test]
    fn ignores_temperature_display_and_unknown() {
        assert_eq!(classify(&port("/dev/ttyACM0", 0x2E8A, 0x101F, None)), None);
        assert_eq!(classify(&port("COM3", 0x1234, 0x5678, None)), None);
        assert_eq!(classify(&port("/dev/ttyS0", 0, 0, None)), None);
    }

    #[test]
    fn identifies_agentcore_usb_cdc() {
        let info = named_port("/dev/ttyACM0", AGENTCORE_VID, AGENTCORE_PID, "AgentCore-Light");
        assert_eq!(classify(&info), Some(DeviceProfile::AgentCore));
        let device = detect(&info, &[]).unwrap();
        assert!(!device.probe_required, "USB PID 已确认，不必再握手");
        assert_eq!(device.profile, DeviceProfile::AgentCore);
    }

    #[test]
    fn espressif_vid_needs_a_product_keyword() {
        let generic = named_port("/dev/ttyACM1", AGENTCORE_ALT_VID, 0x1001, "ESP32-S3 DevKit");
        assert_eq!(classify(&generic), None, "别的 ESP32 开发板不能被当成灯");

        let light = named_port("/dev/ttyACM1", AGENTCORE_ALT_VID, 0x1001, "AgentCore-Light");
        assert_eq!(classify(&light), Some(DeviceProfile::AgentCore));

        let jieli = PortInfo {
            serial: Some("AC696N-0001".to_string()),
            ..named_port("/dev/ttyACM2", AGENTCORE_ALT_VID, 0x4001, "USB Serial")
        };
        assert_eq!(classify(&jieli), Some(DeviceProfile::AgentCore));
    }

    #[test]
    fn bluetooth_spp_ports_are_probed_before_use() {
        for name in [
            "/dev/rfcomm0",
            "COM5 Bluetooth",
            "/dev/tty.Bluetooth-Incoming-Port",
        ] {
            let info = bluetooth_port(name);
            assert_eq!(classify(&info), Some(DeviceProfile::AgentCore), "{name}");
            let device = detect(&info, &[]).unwrap();
            assert!(device.probe_required, "{name} 必须握手确认");
        }

        // 普通串口不是候选，避免抢用户的其它串口设备。
        let plain = PortInfo {
            kind: PortKind::Other,
            ..bluetooth_port("/dev/ttyS0")
        };
        assert_eq!(classify(&plain), None);
    }

    #[test]
    fn agentcore_bindings_still_need_handshake_on_plain_ports() {
        let info = PortInfo {
            kind: PortKind::Other,
            ..bluetooth_port("COM9")
        };
        assert_eq!(classify(&info), None);
        let bindings = vec![DeviceBinding {
            serial: None,
            port: Some("COM9".into()),
            profile: DeviceProfile::AgentCore,
        }];
        let device = detect(&info, &bindings).unwrap();
        assert!(device.probe_required, "靠绑定认出来的端口也要握手");
    }

    #[test]
    fn serial_binding_beats_port_binding() {
        let info = port("/dev/ttyUSB0", 0x1A86, 0x7523, Some("ABC123"));
        let bindings = vec![
            DeviceBinding {
                serial: None,
                port: Some("/dev/ttyUSB0".into()),
                profile: DeviceProfile::MlightSingle,
            },
            DeviceBinding {
                serial: Some("ABC123".into()),
                port: None,
                profile: DeviceProfile::Yh50,
            },
        ];
        assert_eq!(
            resolve_profile(DeviceProfile::Mlight3Led, &info, &bindings),
            DeviceProfile::Yh50
        );
    }

    #[test]
    fn detected_id_prefers_serial() {
        let info = port("/dev/ttyUSB0", 0x1A86, 0x7523, Some("ABC123"));
        assert_eq!(device_id(&info), "serial:ABC123");
        let info = port("/dev/ttyUSB0", 0x1A86, 0x7523, None);
        assert_eq!(device_id(&info), "port:/dev/ttyUSB0");
    }

    #[test]
    fn detect_returns_binding_override() {
        let info = port("/dev/ttyACM0", 0x2E8A, 0x0003, None);
        let bindings = vec![DeviceBinding {
            serial: None,
            port: Some("/dev/ttyACM0".into()),
            profile: DeviceProfile::MlightSingle,
        }];
        let device = detect(&info, &bindings).unwrap();
        assert_eq!(device.profile, DeviceProfile::MlightSingle);
    }
}
