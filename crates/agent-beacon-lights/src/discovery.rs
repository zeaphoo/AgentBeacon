//! 设备发现：VID/PID 识别、序列号/端口绑定与 profile 覆盖。

use agent_beacon_core::{DeviceBinding, DeviceProfile};

pub const MLIGHT_VID: u16 = 0x2E8A;
pub const MLIGHT_PIDS: [u16; 2] = [0x0003, 0x000A];
/// 同厂商的温度显示设备，不是状态灯。
pub const MLIGHT_EXCLUDED_PID: u16 = 0x101F;
pub const YH50_VID: u16 = 0x1A86;
pub const YH50_PID: u16 = 0x7523;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    pub port_name: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub serial: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedDevice {
    pub id: String,
    pub port: String,
    pub profile: DeviceProfile,
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
}

/// 设备 id 优先使用 USB 序列号，避免重插后端口名变化。
pub fn device_id(info: &PortInfo) -> String {
    match info.serial.as_deref().map(str::trim) {
        Some(serial) if !serial.is_empty() => format!("serial:{serial}"),
        _ => format!("port:{}", info.port_name),
    }
}

pub fn classify(info: &PortInfo) -> Option<DeviceProfile> {
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

pub fn detect(info: &PortInfo, bindings: &[DeviceBinding]) -> Option<DetectedDevice> {
    let detected = classify(info)?;
    let profile = resolve_profile(detected, info, bindings);
    Some(DetectedDevice {
        id: device_id(info),
        port: info.port_name.clone(),
        profile,
        vid: info.vid?,
        pid: info.pid?,
        serial: info.serial.clone(),
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
