//! 设备型号（profile）。协议差异都收敛在这个枚举后面。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DeviceCapabilities {
    #[serde(default)]
    pub brightness: bool,
    #[serde(default)]
    pub buzzer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceProfileInfo {
    pub profile: DeviceProfile,
    pub capabilities: DeviceCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceProfile {
    #[serde(rename = "mlight-single")]
    MlightSingle,
    #[serde(rename = "mlight-3led")]
    Mlight3Led,
    #[serde(rename = "yh50-rgy-j")]
    Yh50,
    #[serde(rename = "agentcore-light")]
    AgentCore,
}

impl DeviceProfile {
    pub const ALL: [DeviceProfile; 4] = [
        DeviceProfile::MlightSingle,
        DeviceProfile::Mlight3Led,
        DeviceProfile::Yh50,
        DeviceProfile::AgentCore,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DeviceProfile::MlightSingle => "mlight-single",
            DeviceProfile::Mlight3Led => "mlight-3led",
            DeviceProfile::Yh50 => "yh50-rgy-j",
            DeviceProfile::AgentCore => "agentcore-light",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "mlight-single" => Some(DeviceProfile::MlightSingle),
            "mlight-3led" => Some(DeviceProfile::Mlight3Led),
            "yh50-rgy-j" => Some(DeviceProfile::Yh50),
            "agentcore-light" | "agentcore" => Some(DeviceProfile::AgentCore),
            _ => None,
        }
    }

    pub fn is_mlight(self) -> bool {
        matches!(
            self,
            DeviceProfile::MlightSingle | DeviceProfile::Mlight3Led
        )
    }

    /// AgentCore-Light 是 USB CDC / 蓝牙 SPP / BLE 三种链路共用的文本行协议。
    pub fn is_agentcore(self) -> bool {
        self == DeviceProfile::AgentCore
    }

    /// 亮度是否用独立命令下发（而不是折进灯效命令里）。
    pub fn has_brightness_command(self) -> bool {
        self.is_mlight() || self.is_agentcore()
    }

    pub fn capabilities(self) -> DeviceCapabilities {
        match self {
            DeviceProfile::MlightSingle | DeviceProfile::Mlight3Led => DeviceCapabilities {
                brightness: true,
                buzzer: false,
            },
            DeviceProfile::Yh50 => DeviceCapabilities {
                brightness: false,
                buzzer: true,
            },
            DeviceProfile::AgentCore => DeviceCapabilities {
                brightness: true,
                buzzer: true,
            },
        }
    }

    pub fn info(self) -> DeviceProfileInfo {
        DeviceProfileInfo {
            profile: self,
            capabilities: self.capabilities(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_roundtrip() {
        for profile in DeviceProfile::ALL {
            let json = serde_json::to_string(&profile).unwrap();
            assert_eq!(json, format!("\"{}\"", profile.as_str()));
            assert_eq!(
                serde_json::from_str::<DeviceProfile>(&json).unwrap(),
                profile
            );
            assert_eq!(DeviceProfile::parse(profile.as_str()), Some(profile));
        }
        assert_eq!(DeviceProfile::parse("unknown"), None);
    }

    #[test]
    fn capabilities_describe_supported_features() {
        let single = DeviceProfile::MlightSingle.capabilities();
        assert!(single.brightness);
        assert!(!single.buzzer);

        let three = DeviceProfile::Mlight3Led.capabilities();
        assert!(three.brightness);
        assert!(!three.buzzer);

        let yh50 = DeviceProfile::Yh50.capabilities();
        assert!(!yh50.brightness);
        assert!(yh50.buzzer);

        let agentcore = DeviceProfile::AgentCore.capabilities();
        assert!(agentcore.brightness);
        assert!(agentcore.buzzer);
    }
}
