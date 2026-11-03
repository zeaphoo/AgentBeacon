//! 源煌 YH50-RGY-J 的 5 字节二进制帧协议。

use agent_beacon_core::Effect;

use crate::{CommandPlan, PlannedCommand};

pub const HEADER: u8 = 0xFF;
pub const TAIL: u8 = 0xAA;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LedMode {
    Off = 0x01,
    Green = 0x02,
    Yellow = 0x03,
    Red = 0x04,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BuzzerMode {
    Off = 0x01,
    On = 0x02,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FlashFrequency {
    Off = 0x01,
    Fast = 0x02,
    Medium = 0x03,
    Slow = 0x04,
}

/// 帧：`FF | LED | 蜂鸣器 | 闪光频率 | AA`。
pub fn frame(led: LedMode, buzzer: BuzzerMode, frequency: FlashFrequency) -> [u8; 5] {
    [HEADER, led as u8, buzzer as u8, frequency as u8, TAIL]
}

pub fn effect_frame(effect: Effect, buzzer_on_alert: bool) -> [u8; 5] {
    match effect {
        Effect::Off => frame(LedMode::Off, BuzzerMode::Off, FlashFrequency::Off),
        Effect::Calm => frame(LedMode::Green, BuzzerMode::Off, FlashFrequency::Off),
        Effect::Busy => frame(LedMode::Yellow, BuzzerMode::Off, FlashFrequency::Off),
        Effect::Attention => frame(LedMode::Yellow, BuzzerMode::Off, FlashFrequency::Medium),
        Effect::Alert if buzzer_on_alert => {
            frame(LedMode::Red, BuzzerMode::On, FlashFrequency::Medium)
        }
        Effect::Alert => frame(LedMode::Red, BuzzerMode::Off, FlashFrequency::Medium),
    }
}

pub fn plan(effect: Effect, buzzer_on_alert: bool) -> CommandPlan {
    CommandPlan {
        dedupe_key: format!("yh50-rgy-j:{}", effect.as_str()),
        commands: vec![PlannedCommand {
            delay_ms: 0,
            bytes: effect_frame(effect, buzzer_on_alert).to_vec(),
            label: format!("yh50:{}", effect.as_str()),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_uses_medium_flash() {
        assert_eq!(
            effect_frame(Effect::Alert, false),
            [0xFF, 0x04, 0x01, 0x03, 0xAA]
        );
    }

    #[test]
    fn buzzer_flag_only_touches_alert() {
        assert_eq!(
            effect_frame(Effect::Alert, true),
            [0xFF, 0x04, 0x02, 0x03, 0xAA]
        );
        assert_eq!(
            effect_frame(Effect::Busy, true),
            [0xFF, 0x03, 0x01, 0x01, 0xAA]
        );
    }

    #[test]
    fn frames_cover_every_effect() {
        assert_eq!(
            effect_frame(Effect::Off, false),
            [0xFF, 0x01, 0x01, 0x01, 0xAA]
        );
        assert_eq!(
            effect_frame(Effect::Calm, false),
            [0xFF, 0x02, 0x01, 0x01, 0xAA]
        );
        assert_eq!(
            effect_frame(Effect::Busy, false),
            [0xFF, 0x03, 0x01, 0x01, 0xAA]
        );
        assert_eq!(
            effect_frame(Effect::Attention, false),
            [0xFF, 0x03, 0x01, 0x03, 0xAA]
        );
    }
}
