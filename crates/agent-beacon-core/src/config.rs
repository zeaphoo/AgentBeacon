//! 配置模型。文件读写属于服务层，这里只定义结构与校验。

use std::collections::{HashMap, HashSet};

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
    pub routing: RoutingConfig,
    pub devices: Vec<DeviceBinding>,
    pub integrations: IntegrationsConfig,
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
            routing: RoutingConfig::default(),
            devices: Vec::new(),
            integrations: IntegrationsConfig::default(),
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

/// 多灯控制策略。用户只需要在四种模式里选一个。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingStrategy {
    /// 统一显示：所有灯显示同一个全局状态。
    #[default]
    Unified,
    /// 按 Agent 自动分配：每个 Agent 自动分到一台灯。
    AutoAgent,
    /// 按 Agent 手动分配：手动把 Agent 分到灯。
    ManualAgent,
    /// 按 Session 自动分配：同一个 Agent 的多个 session 自动分散到不同灯。
    AutoSession,
}

impl RoutingStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            RoutingStrategy::Unified => "unified",
            RoutingStrategy::AutoAgent => "auto_agent",
            RoutingStrategy::ManualAgent => "manual_agent",
            RoutingStrategy::AutoSession => "auto_session",
        }
    }
}

/// 服务内部推导出的设备角色，不直接出现在用户配置里。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceAssignment {
    /// 显示全局聚合。
    #[default]
    Mirror,
    /// 参与自动分配。
    Auto,
    /// 只显示手动分配的 Agent。
    Manual,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoutingConfig {
    pub strategy: RoutingStrategy,
    /// 手动模式：设备 id -> Agent 列表。
    pub manual: HashMap<String, Vec<String>>,
}

/// 按设备 id、USB 序列号或端口名把设备绑定到指定 profile，并承载展示设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceBinding {
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub port: Option<String>,
    /// 直接按设备 id 绑定：`serial:…`、`port:…`、`ble:…`、`spp:…`。
    #[serde(default)]
    pub id: Option<String>,
    pub profile: DeviceProfile,
    /// 展示名，仅用于 UI，不参与设备匹配。
    #[serde(default)]
    pub label: Option<String>,
    /// 每设备亮度覆盖；仅对具备亮度能力的 profile 生效。
    #[serde(default)]
    pub brightness: Option<u8>,
    /// 每设备蜂鸣覆盖；仅对具备蜂鸣能力的 profile 生效。
    #[serde(default)]
    pub buzzer_on_alert: Option<bool>,
}

impl DeviceBinding {
    /// 绑定上所有非空的设备键。
    pub fn device_keys(&self) -> impl Iterator<Item = &str> {
        [
            self.id.as_deref(),
            self.serial.as_deref(),
            self.port.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IntegrationsConfig {
    /// 服务启动时自动安装新检测到的 agent 集成。
    pub auto_install: bool,
    /// 首次启动的接入提示是否已经处理过（安装或跳过）。
    pub onboarded: bool,
}

impl Default for IntegrationsConfig {
    fn default() -> Self {
        Self {
            auto_install: false,
            onboarded: false,
        }
    }
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
    #[error("device binding must provide id, serial or port")]
    MissingBindingKey,
    #[error("device label must not exceed 64 characters")]
    LabelTooLong,
    #[error("device brightness must be between 0 and 100")]
    InvalidDeviceBrightness,
    #[error("manual assignment device id must not be empty")]
    EmptyManualDevice,
    #[error("manual assignment agent must not be empty")]
    EmptyManualAgent,
    #[error("agent {0} is assigned to more than one light")]
    AgentAssignedTwice(String),
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
            if binding.device_keys().next().is_none() {
                return Err(ConfigError::MissingBindingKey);
            }
            if binding
                .label
                .as_deref()
                .is_some_and(|label| label.chars().count() > 64)
            {
                return Err(ConfigError::LabelTooLong);
            }
            if binding
                .brightness
                .is_some_and(|brightness| brightness > 100)
            {
                return Err(ConfigError::InvalidDeviceBrightness);
            }
        }

        let mut assigned = HashSet::new();
        for (device, agents) in &self.routing.manual {
            if device.trim().is_empty() {
                return Err(ConfigError::EmptyManualDevice);
            }
            for agent in agents {
                let agent = agent.trim();
                if agent.is_empty() {
                    return Err(ConfigError::EmptyManualAgent);
                }
                if !assigned.insert(agent.to_string()) {
                    return Err(ConfigError::AgentAssignedTwice(agent.to_string()));
                }
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
        assert_eq!(config.routing.strategy, RoutingStrategy::Unified);
        assert!(config.routing.manual.is_empty());
        let text = serde_json::to_string_pretty(&config).unwrap();
        let parsed: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn integrations_default_to_manual_install() {
        let config = Config::default();
        assert!(!config.integrations.auto_install);
        assert!(!config.integrations.onboarded);

        // 老配置文件缺少 integrations 段时按默认值补齐。
        let parsed: Config = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert_eq!(parsed.integrations, IntegrationsConfig::default());
    }

    #[test]
    fn old_config_gets_default_routing() {
        let parsed: Config =
            serde_json::from_str(r#"{"devices":[{"serial":"ABC","profile":"mlight-3led"}]}"#)
                .unwrap();
        assert_eq!(parsed.routing, RoutingConfig::default());
        assert!(parsed.devices[0].id.is_none());
        assert!(parsed.devices[0].label.is_none());
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
    fn unknown_effect_profile_or_strategy_is_rejected() {
        let bad_effect = r#"{"mapping":{"error":"purple"}}"#;
        assert!(serde_json::from_str::<Config>(bad_effect).is_err());

        let bad_profile = r#"{"devices":[{"port":"/dev/ttyUSB0","profile":"unknown"}]}"#;
        assert!(serde_json::from_str::<Config>(bad_profile).is_err());

        let bad_strategy = r#"{"routing":{"strategy":"split"}}"#;
        assert!(serde_json::from_str::<Config>(bad_strategy).is_err());
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
            id: None,
            profile: DeviceProfile::Yh50,
            label: None,
            brightness: None,
            buzzer_on_alert: None,
        });
        assert_eq!(config.validate(), Err(ConfigError::MissingBindingKey));
    }

    #[test]
    fn manual_assignment_is_validated() {
        let mut config = Config::default();
        config.devices.push(DeviceBinding {
            serial: Some("ABC".into()),
            port: None,
            id: None,
            profile: DeviceProfile::Mlight3Led,
            label: Some("Claude".into()),
            brightness: Some(101),
            buzzer_on_alert: None,
        });
        assert_eq!(config.validate(), Err(ConfigError::InvalidDeviceBrightness));

        config.devices[0].brightness = Some(50);
        config
            .routing
            .manual
            .insert("  ".into(), vec!["claude".into()]);
        assert_eq!(config.validate(), Err(ConfigError::EmptyManualDevice));

        config.routing.manual.clear();
        config
            .routing
            .manual
            .insert("serial:ABC".into(), vec!["  ".into()]);
        assert_eq!(config.validate(), Err(ConfigError::EmptyManualAgent));

        config.routing.manual.clear();
        config
            .routing
            .manual
            .insert("serial:A".into(), vec!["claude".into()]);
        config
            .routing
            .manual
            .insert("serial:B".into(), vec!["claude".into()]);
        assert_eq!(
            config.validate(),
            Err(ConfigError::AgentAssignedTwice("claude".into()))
        );

        config.routing.manual.clear();
        config.devices[0].label = Some("x".repeat(65));
        assert_eq!(config.validate(), Err(ConfigError::LabelTooLong));
    }

    #[test]
    fn routing_roundtrips() {
        let mut config = Config::default();
        config.routing.strategy = RoutingStrategy::ManualAgent;
        config.routing.manual.insert(
            "serial:A".into(),
            vec!["claude-code".into(), "codex".into()],
        );
        config
            .routing
            .manual
            .insert("ble:af:c3:a4:e1:d8:95".into(), vec!["pi".into()]);
        config.devices.push(DeviceBinding {
            serial: Some("A".into()),
            port: None,
            id: None,
            profile: DeviceProfile::Mlight3Led,
            label: Some("左灯".into()),
            brightness: None,
            buzzer_on_alert: Some(true),
        });
        let text = serde_json::to_string(&config).unwrap();
        let parsed: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(config, parsed);
    }
}
