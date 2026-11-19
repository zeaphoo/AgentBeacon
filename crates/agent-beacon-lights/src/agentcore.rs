//! AgentCore-Light 的 ASCII 文本行协议（USB CDC / 蓝牙 SPP / BLE 共用）。
//!
//! 主机 → 设备：`<COMMAND>[:<arg>[,<arg>...]]\n`
//! 设备 → 主机：`<KEY>:<VALUE>[,<KEY>:<VALUE>...]\n`
//!
//! 与 MLight 的区别：
//!
//! * 状态不是颜色命令，而是设备自己的状态名（`IDLE` / `BUSY` / `SUCCESS` / `ERROR` / `WAIT_CONFIRM`）；
//! * 开链要先做 `HELLO` 握手（BLE 不需要）；
//! * 设备会主动上报 `STATUS:` 与每 5 秒一次的 `CHG:`（电量），主机必须把读数读走；
//! * 亮度是 `BRIGHTNESS_LEVEL:0-300`（百分比 ×3），不是 `BRIGHTNESS_n`。
//!
//! 协议细节见 `docs/AgentCore-Light-serial-protocol.md`。

use agent_beacon_core::Effect;

use crate::{CommandPlan, PlannedCommand};

/// 设备链路固定参数。
pub const BAUD_RATE: u32 = 115_200;
/// USB CDC 打开后要等设备就绪再握手。
pub const CDC_SETTLE_MS: u64 = 1_500;
/// `HELLO` 应答超时。
pub const HANDSHAKE_TIMEOUT_MS: u64 = 1_500;
pub const HANDSHAKE: &str = "HELLO";
pub const HANDSHAKE_OK: &str = "HELLO:OK";

/// 亮度等级上限（百分比 ×3）。
pub const MAX_BRIGHTNESS_LEVEL: u16 = 300;

/// 语义灯效 → 设备状态命令。
///
/// 厂商上位机把 `THINKING`/`AI`/`WRITING`/`RUNNING` 归一到 `BUSY`，
/// `DONE` 归一到 `SUCCESS`；这里按语义灯效直接给最终状态名。
pub fn state_command(effect: Effect) -> &'static str {
    match effect {
        Effect::Off => "OFF",
        Effect::Calm => "IDLE",
        Effect::Busy => "BUSY",
        Effect::Attention => "WAIT_CONFIRM",
        Effect::Alert => "ERROR",
    }
}

/// 主机侧白名单：只允许协议文档列出的命令。
pub fn validate_command(command: &str) -> bool {
    let head = command.split(':').next().unwrap_or_default();
    // `LC:` 是唯一带 `,` / `;` 的多字段命令，单独校验。
    if head == "LC" {
        return valid_custom_light(command);
    }
    match head {
        "HELLO" | "STATUS" | "GET_SETTINGS" | "GET_BT_NAME" => return !command.contains(':'),
        "OFF" | "IDLE" | "BUSY" | "SUCCESS" | "ERROR" | "WAIT_CONFIRM" | "CONFIRM" | "WAITING"
        | "WAIT" => return !command.contains(':'),
        "BUZZER"
        | "LOW_BATTERY_ALERT"
        | "IDLE_SLEEP_SEC"
        | "BT_DISCONNECT_OFF_SEC"
        | "SET_BT_NAME" => {}
        "BRIGHTNESS" | "BRIGHTNESS_LEVEL" | "BUZZER_VOLUME" => {}
        "SOUND" => {}
        _ => return false,
    }
    let Some((_, value)) = command.split_once(':') else {
        return false;
    };
    if value.is_empty() || value.contains(',') {
        return false;
    }
    match head {
        "BRIGHTNESS" => value.parse::<u16>().is_ok_and(|level| level <= 100),
        "BRIGHTNESS_LEVEL" => value
            .parse::<u16>()
            .is_ok_and(|level| level <= MAX_BRIGHTNESS_LEVEL),
        "BUZZER" => value == "ON" || value == "OFF",
        "BUZZER_VOLUME" => value.parse::<u16>().is_ok_and(|volume| volume <= 100),
        "LOW_BATTERY_ALERT" => value == "0" || value == "1",
        "IDLE_SLEEP_SEC" | "BT_DISCONNECT_OFF_SEC" => value.parse::<u32>().is_ok(),
        "SET_BT_NAME" => valid_bluetooth_name(value),
        "SOUND" => {
            let Some((name, mode)) = value.split_once(':') else {
                return false;
            };
            is_known_sound(name) && (mode == "once" || mode == "loop")
        }
        _ => false,
    }
}

/// 设备自报名字（`GET_BT_NAME`）对应的型号前缀。
pub const LIGHT_NAME_PREFIX: &str = "AgentCore-Light-";
pub const HALO_NAME_PREFIX: &str = "AgentCore-Halo-";

/// 蓝牙名规则（两家固件分别校验）：
///
/// * AgentCore-Light：`AgentCore-Light-` + 1–15 个 `[A-Za-z0-9_-]`，总长 ≤ 31；
/// * AgentCore-Halo：`AgentCore-Halo-` + 同样的后缀规则，总长 ≤ 30。
pub fn valid_bluetooth_name(name: &str) -> bool {
    let (suffix, max_len) = if let Some(suffix) = name.strip_prefix(LIGHT_NAME_PREFIX) {
        (suffix, 31)
    } else if let Some(suffix) = name.strip_prefix(HALO_NAME_PREFIX) {
        (suffix, 30)
    } else {
        return false;
    };
    let length_ok = !suffix.is_empty() && suffix.len() <= 15;
    length_ok
        && name.len() <= max_len
        && suffix
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

/// 自定义灯效 `LC:` 的状态索引（AgentCore-Halo 特性，见协议文档 §3）。
pub const LC_STATE_IDLE: u8 = 0;
pub const LC_STATE_BUSY: u8 = 1;
pub const LC_STATE_SUCCESS: u8 = 2;
pub const LC_STATE_ATTENTION: u8 = 3;
pub const LC_STATE_ERROR: u8 = 4;
pub const LC_STATE_OFF: u8 = 5;

/// `LC:` 的动画类型：`0` default、`1` solid、`2` blink、`3` breathe、`4` off。
///
/// 注意：`default` 在固件里是“用内置灯效”，**颜色会被忽略**（还是要黄色），
/// 所以想改颜色必须显式指定动画。
pub const LC_EFFECT_DEFAULT: u8 = 0;
pub const LC_EFFECT_SOLID: u8 = 1;
pub const LC_EFFECT_BLINK: u8 = 2;
pub const LC_EFFECT_BREATHE: u8 = 3;
pub const LC_EFFECT_OFF: u8 = 4;

/// `运行中` 与 `需要关注` 共用的橙色：固件默认 `#FFD400` 偏黄，实机上两者互相分不开，
/// 也看不出“正在跑”，所以统一改成橙色 `#FF6A00`，用动画（呼吸 / 常亮）区分状态。
pub const ORANGE_RGB: (u8, u8, u8) = (0xFF, 0x6A, 0x00);
/// `BUSY`（运行中）的动画：呼吸；周期用厂商的“中速” 2400 ms。
pub const BUSY_EFFECT: u8 = LC_EFFECT_BREATHE;
pub const BUSY_PERIOD_MS: u16 = 2400;
/// `WAIT_CONFIRM`（需要关注）的动画：常亮。
pub const ATTENTION_EFFECT: u8 = LC_EFFECT_SOLID;

/// 固件默认灯色：`IDLE` / `SUCCESS` 绿、`ERROR` 红、`OFF` 黑；
/// `BUSY` / `WAIT_CONFIRM` 换成橙色（见 [`ORANGE_RGB`]）。
const IDLE_RGB: (u8, u8, u8) = (0x00, 0xFF, 0x00);
const SUCCESS_RGB: (u8, u8, u8) = (0x00, 0xFF, 0x00);
const ERROR_RGB: (u8, u8, u8) = (0xFF, 0x00, 0x00);
const OFF_RGB: (u8, u8, u8) = (0x00, 0x00, 0x00);

/// 实机调试用：`AGENT_BEACON_HALO_LC` 里的整条 `LC:` 行会覆盖内置灯效表，
/// 改完不用重编译（不合法的值会被忽略并记一条 warn）。
pub const LC_OVERRIDE_ENV: &str = "AGENT_BEACON_HALO_LC";

/// `LC:` 的一个状态行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomLight {
    pub state: u8,
    pub effect: u8,
    pub rgb: (u8, u8, u8),
    pub period_ms: u16,
    pub on_ms: u16,
    pub off_ms: u16,
}

impl CustomLight {
    /// 用固件默认动画（颜色会被固件忽略）。
    pub const fn with_default_effect(state: u8, rgb: (u8, u8, u8)) -> Self {
        Self {
            state,
            effect: LC_EFFECT_DEFAULT,
            rgb,
            period_ms: 0,
            on_ms: 0,
            off_ms: 0,
        }
    }

    /// 显式动画 + 颜色（呼吸用 `period_ms`，闪烁用 `on_ms` / `off_ms`）。
    pub const fn with_effect(
        state: u8,
        effect: u8,
        rgb: (u8, u8, u8),
        period_ms: u16,
        on_ms: u16,
        off_ms: u16,
    ) -> Self {
        Self {
            state,
            effect,
            rgb,
            period_ms,
            on_ms,
            off_ms,
        }
    }

    fn fields(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{}",
            self.state,
            self.effect,
            self.rgb.0,
            self.rgb.1,
            self.rgb.2,
            self.period_ms,
            self.on_ms,
            self.off_ms
        )
    }
}

/// Halo 的整表灯效（`LC:` 的 6 行）。
///
/// 与固件默认值的差别在 `BUSY` 与 `WAIT_CONFIRM`：两者都改成橙色，并且必须显式写动画
/// ——`default` 动画会让固件用自己的内置颜色（`#FFD400`），改不出橙色。为了还能分辨，
/// `BUSY` 用呼吸、`WAIT_CONFIRM` 用常亮。
pub fn halo_custom_lights() -> [CustomLight; 6] {
    [
        CustomLight::with_default_effect(LC_STATE_IDLE, IDLE_RGB),
        CustomLight::with_effect(LC_STATE_BUSY, BUSY_EFFECT, ORANGE_RGB, BUSY_PERIOD_MS, 0, 0),
        CustomLight::with_default_effect(LC_STATE_SUCCESS, SUCCESS_RGB),
        CustomLight::with_effect(LC_STATE_ATTENTION, ATTENTION_EFFECT, ORANGE_RGB, 0, 0, 0),
        CustomLight::with_default_effect(LC_STATE_ERROR, ERROR_RGB),
        CustomLight::with_default_effect(LC_STATE_OFF, OFF_RGB),
    ]
}

/// 内置的状态灯效行（不读环境变量，便于测试）。
///
/// 每个元素是 `LC:` 后面那串字段：`state,effect,r,g,b,period,on,off`。
pub fn default_custom_light_rows() -> Vec<String> {
    halo_custom_lights()
        .iter()
        .map(|light| light.fields())
        .collect()
}

/// 每个状态一条 `LC:` 命令（`LF` 结尾）。
///
/// 固件的行读取器把 `;` 也当成命令结束符：一条 `LC:a;b;c;` 里除第一段之外的
/// 每一段都会被当成独立命令回报 `ERR UNKNOWN:<...>`（实机日志确认），所以灯表
/// 必须拆成 6 条命令逐条下发。
///
/// 注意：Halo 固件收到 `LC:` 会覆盖厂商上位机里保存的该状态灯效，所以只在连接
/// 建立后下发一次（每次重连都重发）。
pub fn custom_light_commands() -> Vec<Vec<u8>> {
    custom_light_rows()
        .iter()
        .filter_map(|row| encode(&format!("LC:{row}")))
        .collect()
}

/// 灯表的 6 行字段：默认取内置表，`AGENT_BEACON_HALO_LC` 有值时整表替换。
fn custom_light_rows() -> Vec<String> {
    let Ok(value) = std::env::var(LC_OVERRIDE_ENV) else {
        return default_custom_light_rows();
    };
    let rows: Vec<String> = value
        .trim()
        .split(';')
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(|row| {
            let row = row.strip_prefix("LC:").unwrap_or(row);
            row.to_string()
        })
        .collect();
    let complete = rows.len() == 6 && rows.iter().all(|row| valid_custom_light_row(row));
    if !complete {
        tracing::warn!(
            value = value.trim(),
            "ignoring {LC_OVERRIDE_ENV}: expected 6 `LC:` rows of 8 fields"
        );
        return default_custom_light_rows();
    }
    rows
}

/// `LC:` 字段个数：`state,effect,r,g,b,period,on,off`。
const CUSTOM_LIGHT_FIELDS: usize = 8;

/// `LC:<state>,<effect>,<r>,<g>,<b>,<period>,<on>,<off>` 的单行校验。
fn valid_custom_light(command: &str) -> bool {
    command
        .strip_prefix("LC:")
        .is_some_and(valid_custom_light_row)
}

fn valid_custom_light_row(row: &str) -> bool {
    let fields: Vec<&str> = row.split(',').collect();
    if fields.len() != CUSTOM_LIGHT_FIELDS {
        return false;
    }
    let values: Option<Vec<u32>> = fields
        .iter()
        .map(|field| field.parse::<u32>().ok())
        .collect();
    let Some(values) = values else {
        return false;
    };
    values[0] <= u32::from(LC_STATE_OFF)
        && values[1] <= u32::from(LC_EFFECT_OFF)
        && values[2..5].iter().all(|value| *value <= 255)
        && values[5..].iter().all(|value| *value <= 60_000)
}

/// 固件内置音效名（文档 §5.4）。
pub const SOUNDS: [&str; 32] = [
    "OFF",
    "OK",
    "DONE",
    "POP",
    "SOFT",
    "UP",
    "DOWN",
    "ALERT",
    "CHIME",
    "CLASSIC",
    "MELODY_SCALE",
    "MELODY_JUMP",
    "MELODY_COIN",
    "MELODY_LEVELUP",
    "MELODY_READY",
    "MELODY_STAGE",
    "MELODY_BONUS",
    "MELODY_ARCADE",
    "MELODY_ARCADE2",
    "MELODY_CHIPTUNE",
    "MELODY_GAMEOVER",
    "MELODY_FANFARE",
    "MELODY_HAPPY",
    "MELODY_CALM",
    "MELODY_ODE",
    "MELODY_FRERE",
    "MELODY_TWINKLE",
    "MELODY_TURKISH",
    "MELODY_FUR_ELISE",
    "MELODY_MINUET",
    "MELODY_PUZZLE",
    "MELODY_RISE",
];

pub fn is_known_sound(name: &str) -> bool {
    SOUNDS.contains(&name)
}

/// 把一行命令编码为 `LF` 结尾的 ASCII；非法命令返回 `None`。
pub fn encode(command: &str) -> Option<Vec<u8>> {
    if !validate_command(command) {
        return None;
    }
    let mut bytes = command.as_bytes().to_vec();
    bytes.push(b'\n');
    Some(bytes)
}

/// 百分比 → `BRIGHTNESS_LEVEL`（0–300）。
pub fn brightness_level(percent: u8) -> u16 {
    u16::from(percent.min(100)) * 3
}

pub fn brightness_command(percent: u8) -> Vec<u8> {
    format!("BRIGHTNESS_LEVEL:{}\n", brightness_level(percent)).into_bytes()
}

pub fn buzzer_command(enabled: bool) -> Vec<u8> {
    format!("BUZZER:{}\n", if enabled { "ON" } else { "OFF" }).into_bytes()
}

pub fn buzzer_volume_command(volume: u8) -> Vec<u8> {
    format!("BUZZER_VOLUME:{}\n", volume.min(100)).into_bytes()
}

pub fn sound_command(name: &str, mode: &str) -> Option<Vec<u8>> {
    encode(&format!("SOUND:{name}:{mode}"))
}

fn command(label: &str, delay_ms: u64) -> PlannedCommand {
    PlannedCommand {
        delay_ms,
        bytes: encode(label).expect("内置命令必须通过白名单"),
        label: label.to_string(),
    }
}

/// 语义灯效 → AgentCore-Light 命令计划。
///
/// 设备自己维护灯效，主机只发状态名，因此每条计划只有一条命令。
/// 开启“告警蜂鸣”时，`attention` 追加一次固件内置提示音
/// （厂商默认绑定 `WAIT_CONFIRM → CLASSIC:once`，这里保持一致）。
pub fn plan(effect: Effect, buzzer_on_alert: bool) -> CommandPlan {
    let mut commands = vec![command(state_command(effect), 0)];
    if buzzer_on_alert && effect == Effect::Attention {
        commands.push(command("SOUND:CLASSIC:once", 120));
    }
    CommandPlan {
        dedupe_key: format!("agentcore-light:{}", effect.as_str()),
        commands,
    }
}

/// 仅重放当前状态，不带额外清场。
///
/// 设备的状态命令是幂等的；亮度调整后重发一次当前状态即可让新亮度立刻生效。
pub fn replay(effect: Effect) -> CommandPlan {
    CommandPlan {
        dedupe_key: format!("agentcore-light:{}:replay", effect.as_str()),
        commands: vec![command(state_command(effect), 0)],
    }
}

/// 设备上报的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Hello,
    State(String),
    Status(StatusReport),
    Settings(SettingsReport),
    Sleep(SleepReport),
    BluetoothName(String),
    Sound { name: String, mode: String },
    Battery(BatteryReport),
    Error(String),
    Unknown(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusReport {
    pub state: Option<String>,
    pub firmware: Option<String>,
    pub brightness_level: Option<u16>,
    pub buzzer: Option<bool>,
    pub buzzer_volume: Option<u8>,
    pub ble_state: Option<u8>,
    pub ble_advertising: Option<bool>,
    pub ble_connected: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsReport {
    pub brightness_level: Option<u16>,
    pub buzzer: Option<bool>,
    pub buzzer_volume: Option<u8>,
    pub low_battery_alert: Option<bool>,
    pub stored_in_vm: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SleepReport {
    pub idle_sleep_secs: Option<u32>,
    pub bt_disconnect_off_secs: Option<u32>,
    pub sleeping: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatteryReport {
    /// 固件的充电输入标志（`CHG`）。注意：它只代表“充电口有电/被检测到”，
    /// 不等于是“正在充电”——厂商上位机也要结合电压变化才判断真正在充电。
    pub usb_online: bool,
    /// 电池电压，单位 0.01 V。
    pub millivolts: Option<u32>,
    /// 电量计芯片给出的百分比。
    pub percent: Option<u8>,
    pub current_ma: Option<u32>,
}

/// 按 `,` 切字段，每段按第一个 `:` 切 key/value。
fn fields(input: &str) -> Vec<(Option<&str>, &str)> {
    input
        .split(',')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(|segment| match segment.split_once(':') {
            Some((key, value)) => (Some(key.trim()), value.trim()),
            None => (None, segment),
        })
        .collect()
}

fn find<'a>(items: &'a [(Option<&str>, &str)], key: &str) -> Option<&'a str> {
    items
        .iter()
        .find(|(name, _)| *name == Some(key))
        .map(|(_, value)| *value)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "ON" | "1" | "on" | "true" => Some(true),
        "OFF" | "0" | "off" | "false" => Some(false),
        _ => None,
    }
}

fn parse_u8(value: &str) -> Option<u8> {
    value.trim().parse::<u8>().ok()
}

fn parse_u16(value: &str) -> Option<u16> {
    value.trim().parse::<u16>().ok()
}

fn parse_u32(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok()
}

/// 解析设备上报的一行；不认识的行返回 `Report::Unknown`。
///
/// 兼容 SPP 的 `\r\n`（调用方按 `\n` 切行后可能残留 `\r`）。
pub fn parse_report(line: &str) -> Report {
    let line = line.trim().trim_end_matches('\r');
    if line.is_empty() {
        return Report::Unknown(String::new());
    }
    if line == HANDSHAKE_OK {
        return Report::Hello;
    }
    if let Some(rest) = line.strip_prefix("ERR") {
        return Report::Error(rest.trim_start_matches([' ', ':']).to_string());
    }

    let Some((key, rest)) = line.split_once(':') else {
        return Report::Unknown(line.to_string());
    };
    match key.trim() {
        "STATE" => Report::State(rest.trim().to_string()),
        "STATUS" => {
            let items = fields(rest);
            let state = items
                .iter()
                .find(|(name, _)| name.is_none())
                .map(|(_, value)| value.to_string());
            Report::Status(StatusReport {
                state,
                firmware: find(&items, "FW").map(str::to_string),
                brightness_level: find(&items, "BRIGHTNESS_LEVEL").and_then(parse_u16),
                buzzer: find(&items, "BUZZER").and_then(parse_bool),
                buzzer_volume: find(&items, "BUZZER_VOLUME").and_then(parse_u8),
                ble_state: find(&items, "BLE_STATE").and_then(parse_u8),
                ble_advertising: find(&items, "BLE_ADV").and_then(parse_bool),
                ble_connected: find(&items, "BLE_CONN").and_then(parse_bool),
            })
        }
        "SETTINGS" => {
            let items = fields(rest);
            Report::Settings(SettingsReport {
                brightness_level: find(&items, "BRIGHTNESS_LEVEL").and_then(parse_u16),
                buzzer: find(&items, "BUZZER").and_then(parse_bool),
                buzzer_volume: find(&items, "BUZZER_VOLUME").and_then(parse_u8),
                low_battery_alert: find(&items, "LOW_BATTERY_ALERT").and_then(parse_bool),
                stored_in_vm: find(&items, "STORE").is_some_and(|value| value == "VM"),
            })
        }
        "SLEEP_SETTINGS" => {
            let items = fields(rest);
            Report::Sleep(SleepReport {
                idle_sleep_secs: find(&items, "IDLE_SLEEP_SEC").and_then(parse_u32),
                bt_disconnect_off_secs: find(&items, "BT_DISCONNECT_OFF_SEC").and_then(parse_u32),
                sleeping: find(&items, "SLEEP").and_then(parse_bool),
            })
        }
        "BT_NAME" => {
            let items = fields(rest);
            Report::BluetoothName(find(&items, "CURRENT").unwrap_or(rest.trim()).to_string())
        }
        "SOUND" => {
            // 设备原样回显 `SOUND:<NAME>:<mode>`，音效名本身不含 `:`。
            let (name, mode) = rest.split_once(':').unwrap_or((rest, ""));
            Report::Sound {
                name: name.trim().to_string(),
                mode: mode.trim().to_string(),
            }
        }
        "CHG" => {
            let items = fields(rest);
            Report::Battery(BatteryReport {
                usb_online: items
                    .iter()
                    .find(|(name, _)| name.is_none())
                    .and_then(|(_, value)| parse_bool(value))
                    .unwrap_or(false),
                millivolts: find(&items, "VBAT").and_then(parse_u32),
                percent: find(&items, "PER").and_then(parse_u8),
                current_ma: find(&items, "CUR").and_then(parse_u32),
            })
        }
        _ => Report::Unknown(line.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_map_to_device_states() {
        assert_eq!(state_command(Effect::Off), "OFF");
        assert_eq!(state_command(Effect::Calm), "IDLE");
        assert_eq!(state_command(Effect::Busy), "BUSY");
        assert_eq!(state_command(Effect::Attention), "WAIT_CONFIRM");
        assert_eq!(state_command(Effect::Alert), "ERROR");
    }

    #[test]
    fn plan_is_single_line_with_lf() {
        let plan = plan(Effect::Busy, false);
        assert_eq!(plan.dedupe_key, "agentcore-light:busy");
        assert_eq!(
            plan.commands,
            vec![PlannedCommand {
                delay_ms: 0,
                bytes: b"BUSY\n".to_vec(),
                label: "BUSY".to_string(),
            }]
        );
    }

    #[test]
    fn alert_buzzer_only_touches_attention() {
        let attention = plan(Effect::Attention, true);
        let labels: Vec<&str> = attention
            .commands
            .iter()
            .map(|command| command.label.as_str())
            .collect();
        assert_eq!(labels, ["WAIT_CONFIRM", "SOUND:CLASSIC:once"]);

        let alert = plan(Effect::Alert, true);
        assert_eq!(alert.commands.len(), 1);
        assert_eq!(alert.commands[0].label, "ERROR");
    }

    #[test]
    fn brightness_is_percent_times_three() {
        assert_eq!(brightness_level(0), 0);
        assert_eq!(brightness_level(50), 150);
        assert_eq!(brightness_level(100), 300);
        assert_eq!(brightness_level(255), 300);
        assert_eq!(brightness_command(40), b"BRIGHTNESS_LEVEL:120\n".to_vec());
        assert_eq!(buzzer_command(true), b"BUZZER:ON\n".to_vec());
        assert_eq!(buzzer_volume_command(120), b"BUZZER_VOLUME:100\n".to_vec());
    }

    #[test]
    fn whitelist_matches_protocol() {
        for valid in [
            "HELLO",
            "STATUS",
            "IDLE",
            "BUSY",
            "WAIT_CONFIRM",
            "OFF",
            "BRIGHTNESS_LEVEL:300",
            "BRIGHTNESS:100",
            "BUZZER:ON",
            "BUZZER_VOLUME:10",
            "LOW_BATTERY_ALERT:1",
            "SOUND:DONE:once",
            "SOUND:MELODY_TWINKLE:loop",
            "SET_BT_NAME:AgentCore-Light-Desk",
            "SET_BT_NAME:AgentCore-Halo-BLE",
            "IDLE_SLEEP_SEC:600",
        ] {
            assert!(validate_command(valid), "{valid} 应通过");
            assert!(encode(valid).unwrap().ends_with(b"\n"));
        }
        for invalid in [
            "",
            "HELP",
            "HELLO:OK",
            "BRIGHTNESS_LEVEL:301",
            "BRIGHTNESS:101",
            "BUZZER:MAYBE",
            "BUZZER_VOLUME:101",
            "LOW_BATTERY_ALERT:2",
            "SOUND:NOPE:once",
            "SOUND:DONE:twice",
            "SET_BT_NAME:AgentCore-Light",
            "SET_BT_NAME:AgentCore-Halo",
            "SET_BT_NAME:Desk-Light",
            "BRIGHTNESS_LEVEL:120,STORE:VM",
            "idle",
        ] {
            assert!(!validate_command(invalid), "{invalid} 不应通过");
            assert!(encode(invalid).is_none());
        }
    }

    #[test]
    fn halo_busy_and_attention_are_orange() {
        // BUSY=橙色呼吸、WAIT_CONFIRM=橙色常亮，其余仍是固件默认动画与颜色。
        let rows = default_custom_light_rows();
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[LC_STATE_IDLE as usize], "0,0,0,255,0,0,0,0");
        assert_eq!(rows[LC_STATE_BUSY as usize], "1,3,255,106,0,2400,0,0");
        assert_eq!(rows[LC_STATE_ATTENTION as usize], "3,1,255,106,0,0,0,0");

        let busy = halo_custom_lights()[LC_STATE_BUSY as usize];
        assert_eq!(busy.rgb, ORANGE_RGB);
        assert_eq!(busy.effect, LC_EFFECT_BREATHE);
        assert_eq!(busy.period_ms, 2400);

        let attention = halo_custom_lights()[LC_STATE_ATTENTION as usize];
        assert_eq!(attention.rgb, ORANGE_RGB, "需要关注也用橙色");
        assert_eq!(attention.effect, LC_EFFECT_SOLID, "需要关注要常亮");
        assert_eq!(attention.period_ms, 0);

        // 固件把 `;` 当命令分隔符（实机回 `ERR UNKNOWN:`），所以必须一条一个状态。
        let commands = custom_light_commands();
        assert_eq!(commands.len(), 6);
        assert_eq!(
            commands[LC_STATE_BUSY as usize],
            b"LC:1,3,255,106,0,2400,0,0\n".to_vec()
        );
        assert_eq!(
            commands[LC_STATE_ATTENTION as usize],
            b"LC:3,1,255,106,0,0,0,0\n".to_vec()
        );
        for command in &commands {
            let line = String::from_utf8_lossy(command);
            assert!(line.starts_with("LC:"), "{line} 应带 LC: 前缀");
            assert!(!line.contains(';'), "{line} 不能带 `;`：固件会拆成多条命令");
            assert!(validate_command(line.trim_end()), "{line} 应通过白名单");
        }

        for invalid in [
            "LC:",
            // 字段不全
            "LC:1,3,255,106,0,2400",
            // 多一个字段
            "LC:1,3,255,106,0,2400,0,0,0",
            // 多个状态塞进一条（旧的错误写法）
            "LC:0,0,0,255,0,0,0,0;1,3,255,106,0,2400,0,0;",
            // state 越界
            "LC:6,0,0,255,0,0,0,0",
            // 颜色越界
            "LC:1,3,256,106,0,2400,0,0",
            // 动画类型越界
            "LC:1,9,255,106,0,2400,0,0",
        ] {
            assert!(!validate_command(invalid), "{invalid} 不应通过");
        }
    }

    #[test]
    fn parses_handshake_and_status() {
        assert_eq!(parse_report("HELLO:OK"), Report::Hello);
        assert_eq!(
            parse_report("HELLO:OK\r"),
            Report::Hello,
            "SPP 链路会用 CRLF"
        );

        let report = parse_report(
            "STATUS:IDLE,FW:v0.5.13-jl-ota,BRIGHTNESS_LEVEL:30,BUZZER:ON,BUZZER_VOLUME:10,BLE_STATE:5,BLE_ADV:0,BLE_CONN:0",
        );
        let Report::Status(status) = report else {
            panic!("应为 STATUS");
        };
        assert_eq!(status.state.as_deref(), Some("IDLE"));
        assert_eq!(status.firmware.as_deref(), Some("v0.5.13-jl-ota"));
        assert_eq!(status.brightness_level, Some(30));
        assert_eq!(status.buzzer, Some(true));
        assert_eq!(status.buzzer_volume, Some(10));
        assert_eq!(status.ble_state, Some(5));
        assert_eq!(status.ble_advertising, Some(false));
        assert_eq!(status.ble_connected, Some(false));
    }

    #[test]
    fn parses_settings_sleep_battery_and_errors() {
        let Report::Settings(settings) = parse_report(
            "SETTINGS:BRIGHTNESS_LEVEL:30,BUZZER:ON,BUZZER_VOLUME:10,LOW_BATTERY_ALERT:1,STORE:VM",
        ) else {
            panic!("应为 SETTINGS");
        };
        assert_eq!(settings.brightness_level, Some(30));
        assert_eq!(settings.low_battery_alert, Some(true));
        assert!(settings.stored_in_vm);

        let Report::Sleep(sleep) = parse_report(
            "SLEEP_SETTINGS:IDLE_SLEEP_SEC:600,BT_DISCONNECT_OFF_SEC:300,SLEEP:OFF,STORE:VM",
        ) else {
            panic!("应为 SLEEP_SETTINGS");
        };
        assert_eq!(sleep.idle_sleep_secs, Some(600));
        assert_eq!(sleep.bt_disconnect_off_secs, Some(300));
        assert_eq!(sleep.sleeping, Some(false));

        let Report::Battery(battery) = parse_report("CHG:1,VBAT:311,PER:0,CUR:100") else {
            panic!("应为 CHG");
        };
        assert!(battery.usb_online);
        assert_eq!(battery.millivolts, Some(311));
        assert_eq!(battery.percent, Some(0));
        assert_eq!(battery.current_ma, Some(100));

        assert_eq!(
            parse_report("ERR UNKNOWN:HELP"),
            Report::Error("UNKNOWN:HELP".to_string())
        );
        assert_eq!(
            parse_report("ERR BRIGHTNESS_LEVEL"),
            Report::Error("BRIGHTNESS_LEVEL".to_string())
        );
        assert_eq!(
            parse_report("BT_NAME:CURRENT:AgentCore-Light"),
            Report::BluetoothName("AgentCore-Light".to_string())
        );
        assert_eq!(
            parse_report("SOUND:DONE:once"),
            Report::Sound {
                name: "DONE".to_string(),
                mode: "once".to_string()
            }
        );
        assert_eq!(
            parse_report("STATE:BUSY"),
            Report::State("BUSY".to_string())
        );
    }

    #[test]
    fn replay_keeps_state_without_extra_commands() {
        let plan = replay(Effect::Attention);
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].label, "WAIT_CONFIRM");
        assert_eq!(plan.dedupe_key, "agentcore-light:attention:replay");
    }
}
