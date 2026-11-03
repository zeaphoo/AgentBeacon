//! 设备型号（profile）。协议差异都收敛在这个枚举后面。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceProfile {
    #[serde(rename = "mlight-single")]
    MlightSingle,
    #[serde(rename = "mlight-3led")]
    Mlight3Led,
    #[serde(rename = "yh50-rgy-j")]
    Yh50,
    #[serde(rename = "mock")]
    Mock,
}

impl DeviceProfile {
    pub const ALL: [DeviceProfile; 4] = [
        DeviceProfile::MlightSingle,
        DeviceProfile::Mlight3Led,
        DeviceProfile::Yh50,
        DeviceProfile::Mock,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DeviceProfile::MlightSingle => "mlight-single",
            DeviceProfile::Mlight3Led => "mlight-3led",
            DeviceProfile::Yh50 => "yh50-rgy-j",
            DeviceProfile::Mock => "mock",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "mlight-single" => Some(DeviceProfile::MlightSingle),
            "mlight-3led" => Some(DeviceProfile::Mlight3Led),
            "yh50-rgy-j" => Some(DeviceProfile::Yh50),
            "mock" => Some(DeviceProfile::Mock),
            _ => None,
        }
    }

    pub fn is_mlight(self) -> bool {
        matches!(
            self,
            DeviceProfile::MlightSingle | DeviceProfile::Mlight3Led
        )
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
}
