//! 语义灯效与状态映射。

use serde::{Deserialize, Serialize};

use crate::Aggregate;

/// 与具体设备协议解耦的语义灯效。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Off,
    Calm,
    Busy,
    Attention,
    Alert,
}

impl Effect {
    pub const ALL: [Effect; 5] = [
        Effect::Off,
        Effect::Calm,
        Effect::Busy,
        Effect::Attention,
        Effect::Alert,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Effect::Off => "off",
            Effect::Calm => "calm",
            Effect::Busy => "busy",
            Effect::Attention => "attention",
            Effect::Alert => "alert",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Effect::Off => "关闭",
            Effect::Calm => "平静",
            Effect::Busy => "运行中",
            Effect::Attention => "需要关注",
            Effect::Alert => "告警",
        }
    }
}

/// 聚合状态到语义灯效的映射，可通过配置覆盖。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mapping {
    pub none: Effect,
    pub idle: Effect,
    pub working: Effect,
    pub waiting: Effect,
    pub error: Effect,
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            none: Effect::Off,
            idle: Effect::Calm,
            working: Effect::Busy,
            waiting: Effect::Attention,
            error: Effect::Alert,
        }
    }
}

impl Mapping {
    pub fn effect_for(&self, aggregate: Aggregate) -> Effect {
        match aggregate {
            Aggregate::None => self.none,
            Aggregate::Idle => self.idle,
            Aggregate::Working => self.working,
            Aggregate::Waiting => self.waiting,
            Aggregate::Error => self.error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mapping_covers_five_aggregates() {
        let mapping = Mapping::default();
        assert_eq!(mapping.effect_for(Aggregate::None), Effect::Off);
        assert_eq!(mapping.effect_for(Aggregate::Idle), Effect::Calm);
        assert_eq!(mapping.effect_for(Aggregate::Working), Effect::Busy);
        assert_eq!(mapping.effect_for(Aggregate::Waiting), Effect::Attention);
        assert_eq!(mapping.effect_for(Aggregate::Error), Effect::Alert);
    }

    #[test]
    fn unknown_effect_is_rejected() {
        let result: Result<Mapping, _> = serde_json::from_str(r#"{"error":"purple"}"#);
        assert!(result.is_err());
    }
}
