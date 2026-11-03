//! 配置模型。文件读写属于服务层，这里只定义结构与校验。

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DeviceProfile, Mapping, DEFAULT_PORT};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub server: ServerConfig,
    pub mapping: Mapping,
    pub manual_override_default_secs: u64,
    pub shutdown: ShutdownConfig,
    pub mlight: MlightConfig,
    pub yh50: Yh50Config,
    pub devices: Vec<DeviceBinding>,
    pub mock: MockConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            server: ServerConfig::default(),
            mapping: Mapping::default(),
            manual_override_default_secs: 60,
            shutdown: ShutdownConfig::default(),
            mlight: MlightConfig::default(),
            yh50: Yh50Config::default(),
            devices: Vec::new(),
            mock: MockConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: DEFAULT_PORT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownConfig {
    pub turn_off_lights: bool,
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            turn_off_lights: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MlightConfig {
    pub brightness: u8,
}

impl Default for MlightConfig {
    fn default() -> Self {
        Self { brightness: 50 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Yh50Config {
    pub buzzer_on_alert: bool,
}

impl Default for Yh50Config {
    fn default() -> Self {
        Self {
            buzzer_on_alert: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MockConfig {
    pub enabled: bool,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

/// 按 USB 序列号或端口名把设备绑定到指定 profile。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceBinding {
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub port: Option<String>,
    pub profile: DeviceProfile,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("unsupported config version: {0}")]
    UnsupportedVersion(u32),
    #[error("server.host must not be empty")]
    EmptyHost,
    #[error("server.port must not be 0")]
    InvalidPort,
    #[error("manual_override_default_secs must be between 1 and 3600")]
    InvalidOverrideSeconds,
    #[error("mlight.brightness must be between 0 and 100")]
    InvalidBrightness,
    #[error("device binding must provide serial or port")]
    MissingBindingKey,
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion(self.version));
        }
        if self.server.host.trim().is_empty() {
            return Err(ConfigError::EmptyHost);
        }
        if self.server.port == 0 {
            return Err(ConfigError::InvalidPort);
        }
        if !(1..=3600).contains(&self.manual_override_default_secs) {
            return Err(ConfigError::InvalidOverrideSeconds);
        }
        if self.mlight.brightness > 100 {
            return Err(ConfigError::InvalidBrightness);
        }
        for binding in &self.devices {
            let has_serial = binding
                .serial
                .as_deref()
                .is_some_and(|v| !v.trim().is_empty());
            let has_port = binding
                .port
                .as_deref()
                .is_some_and(|v| !v.trim().is_empty());
            if !has_serial && !has_port {
                return Err(ConfigError::MissingBindingKey);
            }
        }
        Ok(())
    }

    pub fn bindings(&self) -> &[DeviceBinding] {
        &self.devices
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Effect;

    #[test]
    fn default_config_is_valid_and_roundtrips() {
        let config = Config::default();
        config.validate().unwrap();
        assert_eq!(config.mlight.brightness, 50);
        let text = serde_json::to_string_pretty(&config).unwrap();
        let parsed: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn invalid_values_are_rejected() {
        let mut config = Config::default();
        config.manual_override_default_secs = 0;
        assert_eq!(config.validate(), Err(ConfigError::InvalidOverrideSeconds));

        config.manual_override_default_secs = 60;
        config.mlight.brightness = 101;
        assert_eq!(config.validate(), Err(ConfigError::InvalidBrightness));

        config.mlight.brightness = 100;
        config.server.port = 0;
        assert_eq!(config.validate(), Err(ConfigError::InvalidPort));
    }

    #[test]
    fn unknown_effect_or_profile_is_rejected() {
        let bad_effect = r#"{"mapping":{"error":"purple"}}"#;
        assert!(serde_json::from_str::<Config>(bad_effect).is_err());

        let bad_profile = r#"{"devices":[{"port":"/dev/ttyUSB0","profile":"unknown"}]}"#;
        assert!(serde_json::from_str::<Config>(bad_profile).is_err());
    }

    #[test]
    fn custom_mapping_is_kept() {
        let config: Config = serde_json::from_str(
            r#"{"mapping":{"idle":"off","none":"calm","working":"busy","waiting":"attention","error":"alert"}}"#,
        )
        .unwrap();
        assert_eq!(config.mapping.idle, Effect::Off);
        assert_eq!(config.mapping.none, Effect::Calm);
    }

    #[test]
    fn binding_requires_a_key() {
        let mut config = Config::default();
        config.devices.push(DeviceBinding {
            serial: None,
            port: Some("  ".into()),
            profile: DeviceProfile::Yh50,
        });
        assert_eq!(config.validate(), Err(ConfigError::MissingBindingKey));
    }
}
