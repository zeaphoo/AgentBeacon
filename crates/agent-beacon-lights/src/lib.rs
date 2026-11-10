//! 设备发现、协议编码与每设备 actor。
//!
//! 协议差异只体现在 `DeviceProfile` 对应的命令计划里；上层只传语义灯效。

pub mod actor;
pub mod agentcore;
pub mod ble;
pub mod device;
pub mod discovery;
pub mod mlight;
pub mod mock;
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
        DeviceProfile::AgentCore => agentcore::plan(effect, options.buzzer_on_alert),
        DeviceProfile::Mock => mock::plan(effect),
    }
}

/// 期望状态：全局灯效 + 按设备的手动覆盖。
#[derive(Debug, Clone, PartialEq)]
pub struct DesiredState {
    pub effect: Effect,
    pub overrides: HashMap<String, Effect>,
    pub brightness: u8,
    pub buzzer_on_alert: bool,
}

impl Default for DesiredState {
    fn default() -> Self {
        Self {
            effect: Effect::Off,
            overrides: HashMap::new(),
            brightness: 50,
            buzzer_on_alert: false,
        }
    }
}

impl DesiredState {
    pub fn for_device(&self, id: &str) -> Effect {
        self.overrides.get(id).copied().unwrap_or(self.effect)
    }
}

/// 设备在 API 与 SSE 中的视图。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub port: String,
    pub profile: DeviceProfile,
    #[serde(default)]
    pub capabilities: DeviceCapabilities,
    pub connected: bool,
    pub effect: Effect,
    pub last_error: Option<String>,
    #[serde(default)]
    pub mock_commands: Vec<String>,
}

impl DeviceInfo {
    pub fn new(id: impl Into<String>, port: impl Into<String>, profile: DeviceProfile) -> Self {
        Self {
            id: id.into(),
            port: port.into(),
            profile,
            capabilities: profile.capabilities(),
            connected: false,
            effect: Effect::Off,
            last_error: None,
            mock_commands: Vec::new(),
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

        let plan = plan_for(DeviceProfile::Mock, Effect::Busy, options);
        assert_eq!(plan.dedupe_key, "mock:busy");
    }

    #[test]
    fn overrides_are_per_device() {
        let mut desired = DesiredState::default();
        desired.effect = Effect::Calm;
        desired.overrides.insert("serial:abc".into(), Effect::Alert);
        assert_eq!(desired.for_device("serial:abc"), Effect::Alert);
        assert_eq!(desired.for_device("serial:other"), Effect::Calm);
    }
}
