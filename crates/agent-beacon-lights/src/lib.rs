//! 设备发现、协议编码与每设备 actor。
//!
//! 协议差异只体现在 `DeviceProfile` 对应的命令计划里；上层只传语义灯效。

pub mod actor;
pub mod agentcore;
pub mod ble;
pub mod device;
pub mod discovery;
pub mod mlight;
pub mod spp;
pub mod transport;
pub mod yh50;

use std::collections::HashMap;

use agent_beacon_core::{DeviceCapabilities, DeviceProfile, Effect};
use serde::{Deserialize, Serialize};

/// 一条待写入设备的具体命令，`delay_ms` 是写这条命令之前的等待。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCommand {
    pub delay_ms: u64,
    pub bytes: Vec<u8>,
    pub label: String,
}

/// 一个语义灯效展开后的命令序列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    pub dedupe_key: String,
    pub commands: Vec<PlannedCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanOptions {
    pub brightness: u8,
    pub buzzer_on_alert: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            brightness: 50,
            buzzer_on_alert: false,
        }
    }
}

/// 把语义灯效展开成某型号的具体命令。
pub fn plan_for(profile: DeviceProfile, effect: Effect, options: PlanOptions) -> CommandPlan {
    match profile {
        DeviceProfile::MlightSingle => mlight::plan(effect, false),
        DeviceProfile::Mlight3Led => mlight::plan(effect, true),
        DeviceProfile::Yh50 => yh50::plan(effect, options.buzzer_on_alert),
        DeviceProfile::AgentCore | DeviceProfile::AgentCoreHalo => {
            agentcore::plan(effect, options.buzzer_on_alert)
        }
    }
}

/// 一台设备的目标状态。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceDesired {
    pub effect: Effect,
    pub brightness: u8,
    pub buzzer_on_alert: bool,
}

impl Default for DeviceDesired {
    fn default() -> Self {
        Self {
            effect: Effect::Off,
            brightness: 50,
            buzzer_on_alert: false,
        }
    }
}

/// 期望状态：每台设备一份目标状态 + 新设备兜底。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DesiredState {
    /// 已注册设备的目标状态，键为设备 id。
    pub devices: HashMap<String, DeviceDesired>,
    /// 新设备在下一轮刷新前使用的默认状态。
    pub default: DeviceDesired,
    /// 退出关灯：忽略分配与覆盖，所有设备强制 off。
    pub forced_off: bool,
}

impl DesiredState {
    pub fn for_device(&self, id: &str) -> DeviceDesired {
        if self.forced_off {
            return DeviceDesired {
                effect: Effect::Off,
                ..self.default
            };
        }
        self.devices.get(id).copied().unwrap_or(self.default)
    }
}

/// 设备上报的电量遥测（AgentCore 系列通过 `CHG:` 每 5 秒上报一次）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryInfo {
    /// 电量百分比（设备电量计读数）。
    #[serde(default)]
    pub percent: Option<u8>,
    /// 是否检测到充电输入（`CHG:1`）。它不等于“正在充电”——充满后仍然为 true，
    /// 界面要结合电量再判断。
    pub usb_online: bool,
    /// 电池电压，单位 0.01 V。
    #[serde(default)]
    pub millivolts: Option<u32>,
    /// 充电电流，单位 mA（固件配置的目标电流）。
    #[serde(default)]
    pub current_ma: Option<u32>,
    pub updated_at_ms: u64,
}

impl BatteryInfo {
    /// 两次读数在界面上是否等价（忽略采样时间）。
    pub fn same_reading(&self, other: &Self) -> bool {
        self.percent == other.percent
            && self.usb_online == other.usb_online
            && self.millivolts == other.millivolts
            && self.current_ma == other.current_ma
    }
}

/// 设备在 API 与 SSE 中的视图。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub port: String,
    pub profile: DeviceProfile,
    #[serde(default)]
    pub label: Option<String>,
    /// 设备自报的名字（BLE 广播名 / USB 产品名），例如 `AgentCore-Halo-BLE`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 固件版本（AgentCore 的 `STATUS:` 上报里的 `FW:` 字段）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware: Option<String>,
    /// 最近一次电量读数，设备不支持或还没上报时为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery: Option<BatteryInfo>,
    #[serde(default)]
    pub capabilities: DeviceCapabilities,
    pub connected: bool,
    #[serde(default)]
    pub last_seen_ms: Option<u64>,
    /// 掉线（或首次登记后一直没连上）的时间；控制台据此在保留期后移除设备。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_since_ms: Option<u64>,
    pub effect: Effect,
    pub last_error: Option<String>,
}

impl DeviceInfo {
    pub fn new(id: impl Into<String>, port: impl Into<String>, profile: DeviceProfile) -> Self {
        Self {
            id: id.into(),
            port: port.into(),
            profile,
            label: None,
            name: None,
            firmware: None,
            battery: None,
            capabilities: profile.capabilities(),
            connected: false,
            last_seen_ms: None,
            offline_since_ms: None,
            effect: Effect::Off,
            last_error: None,
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_to_profile_plans() {
        let options = PlanOptions::default();
        let plan = plan_for(DeviceProfile::Yh50, Effect::Alert, options);
        assert_eq!(
            plan.commands,
            vec![PlannedCommand {
                delay_ms: 0,
                bytes: vec![0xFF, 0x04, 0x01, 0x03, 0xAA],
                label: "yh50:alert".to_string(),
            }]
        );
    }

    #[test]
    fn desired_is_per_device_with_a_default() {
        let mut desired = DesiredState::default();
        desired.default.effect = Effect::Calm;
        desired.devices.insert(
            "serial:abc".into(),
            DeviceDesired {
                effect: Effect::Alert,
                ..DeviceDesired::default()
            },
        );
        assert_eq!(desired.for_device("serial:abc").effect, Effect::Alert);
        assert_eq!(desired.for_device("serial:other").effect, Effect::Calm);
    }

    #[test]
    fn forced_off_wins_over_device_desired() {
        let mut desired = DesiredState::default();
        desired.devices.insert(
            "serial:abc".into(),
            DeviceDesired {
                effect: Effect::Alert,
                ..DeviceDesired::default()
            },
        );
        desired.forced_off = true;
        assert_eq!(desired.for_device("serial:abc").effect, Effect::Off);
    }
}
