//! MLight 的 ASCII 文本行协议。

use agent_beacon_core::Effect;

use crate::{CommandPlan, PlannedCommand};

const PRELUDE_MS: u64 = 350;
const COMPOSITE_GAP_MS: u64 = 300;
const SINGLE_PRELUDE_MS: u64 = 80;

/// 主机侧白名单：只允许协议文档列出的命令。
pub fn validate_command(command: &str) -> bool {
    if command == "OFF" {
        return true;
    }
    if let Some(level) = command.strip_prefix("BRIGHTNESS_") {
        return level
            .parse::<u8>()
            .map(|value| value <= 100)
            .unwrap_or(false);
    }
    let Some((color, mode)) = command.split_once('_') else {
        return false;
    };
    matches!(color, "RED" | "YELLOW" | "GREEN") && matches!(mode, "ON" | "OFF" | "BLINK" | "BREATH")
}

/// 把一条命令编码为 `LF` 结尾的 ASCII；非法命令返回 `None`。
pub fn encode(command: &str) -> Option<Vec<u8>> {
    if !validate_command(command) {
        return None;
    }
    let mut bytes = command.as_bytes().to_vec();
    bytes.push(b'\n');
    Some(bytes)
}

pub fn brightness_command(level: u8) -> Vec<u8> {
    let level = level.min(100);
    format!("BRIGHTNESS_{level}\n").into_bytes()
}

fn command(label: &str, delay_ms: u64) -> PlannedCommand {
    PlannedCommand {
        delay_ms,
        bytes: encode(label).expect("内置命令必须通过白名单"),
        label: label.to_string(),
    }
}

/// 语义灯效 → MLight 命令计划。
///
/// 每次切换都以 `OFF` 清场，等待 350ms 再发新命令；组合命令间隔 300ms。
pub fn plan(effect: Effect, three_led: bool) -> CommandPlan {
    let profile = if three_led {
        "mlight-3led"
    } else {
        "mlight-single"
    };
    let dedupe_key = format!("{profile}:{}", effect.as_str());

    let targets: Vec<&str> = match effect {
        Effect::Off => {
            return CommandPlan {
                dedupe_key,
                commands: vec![command("OFF", 0)],
            };
        }
        Effect::Calm => vec!["GREEN_ON"],
        Effect::Attention => vec!["YELLOW_BLINK"],
        Effect::Alert => vec!["RED_BLINK"],
        Effect::Busy if three_led => vec!["GREEN_ON", "YELLOW_BREATH"],
        Effect::Busy => vec!["YELLOW_ON", "YELLOW_BREATH"],
    };

    let mut commands = vec![command("OFF", 0)];
    for (index, target) in targets.iter().enumerate() {
        let delay = if index == 0 {
            PRELUDE_MS
        } else if !three_led && effect == Effect::Busy && index == 1 {
            SINGLE_PRELUDE_MS
        } else {
            COMPOSITE_GAP_MS
        };
        commands.push(command(target, delay));
    }

    CommandPlan {
        dedupe_key,
        commands,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_matches_protocol() {
        for valid in [
            "OFF",
            "RED_ON",
            "YELLOW_BREATH",
            "GREEN_BLINK",
            "BRIGHTNESS_0",
            "BRIGHTNESS_100",
        ] {
            assert!(validate_command(valid), "{valid} 应通过");
            assert!(encode(valid).unwrap().ends_with(b"\n"));
        }
        for invalid in [
            "FOO",
            "BLUE_ON",
            "RED_",
            "BRIGHTNESS_101",
            "red_on",
            "RED_ON\r",
        ] {
            assert!(!validate_command(invalid), "{invalid} 不应通过");
            assert!(encode(invalid).is_none());
        }
    }

    #[test]
    fn brightness_uses_lf_only() {
        assert_eq!(brightness_command(60), b"BRIGHTNESS_60\n".to_vec());
        assert_eq!(brightness_command(255), b"BRIGHTNESS_100\n".to_vec());
    }

    #[test]
    fn three_led_busy_is_composite() {
        let plan = plan(Effect::Busy, true);
        let labels: Vec<_> = plan.commands.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["OFF", "GREEN_ON", "YELLOW_BREATH"]);
        let delays: Vec<_> = plan.commands.iter().map(|c| c.delay_ms).collect();
        assert_eq!(delays, [0, 350, 300]);
        assert_eq!(plan.dedupe_key, "mlight-3led:busy");
    }

    #[test]
    fn single_led_busy_keeps_only_last_animation() {
        let plan = plan(Effect::Busy, false);
        let labels: Vec<_> = plan.commands.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["OFF", "YELLOW_ON", "YELLOW_BREATH"]);
        let delays: Vec<_> = plan.commands.iter().map(|c| c.delay_ms).collect();
        assert_eq!(delays, [0, 350, 80]);
    }

    #[test]
    fn off_is_a_single_command() {
        let plan = plan(Effect::Off, true);
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].bytes, b"OFF\n".to_vec());
    }
}
