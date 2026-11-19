//! 电量与充电状态。
//!
//! 基础算法来自厂商上位机 **AgentCoreHalo v2.5.2**（反编译
//! `service/agentcore_serial_service.py` 的 `ingest_device_line` / `_update_charge_state` /
//! `_filtered_battery_voltage` / `_percent_from_voltage`），并按实测做了三处收紧：
//!
//! 1. **不用 `CHG` 判断充电**。协议里它只是固件的“充电输入检测”标志，实测 AgentCore-Light
//!    (`v0.5.13-jl-ota`) 拔掉 USB 也一直是 `CHG:1`；Halo (`v0.7.1-jl-one-led-halo`) 又可能
//!    一直是 `CHG:0`。所以它只作为诊断信息（`chg_flag`）展示，不参与任何判定。
//! 2. **不把“接近满电”当充电证据**。厂商的 `near_full`（≥3.95 V）在 Light 上永远成立
//!    （静置 4.01 V），会得出“没插电也一直充电中”。
//! 3. **上升证据只认小幅**。拔电后读数会从充电通路的低值（Light 实测 2.66 V）弹回 4.01 V，
//!    1.35 V 的跳变不能算“正在充电”。
//!
//! 于是充电判定只用“读数是否被充电扰动”：
//!
//! * 持续跟踪未充电时的**静置基线**（稳定读数）；
//! * 读数被拉低（`< 2.95 V`，或比基线低 ≥0.10 V）、或比基线小幅上升（0.01–0.60 V）并
//!   持续 [`CHARGE_EVIDENCE_MS`] → 判定充电；
//! * 读数回到基线并持续 [`CHARGE_SETTLED_MS`] → 判为不在充电；
//! * 电量按最近 [`VOLTAGE_FILTER_SAMPLES`] 个电压样本的中值换算，样本不稳定时保持上次值；
//!   充电期间再叠一个按容量/充电电流推算的模型（上限 99%，100% 只能来自实测电压）。

use std::collections::VecDeque;

/// 电池容量（mAh）。
pub const BATTERY_CAPACITY_MAH: f64 = 500.0;
/// 空电电压（`VBAT` 单位 0.01 V）。
pub const BATTERY_EMPTY_CENTIVOLTS: u32 = 280;
/// 满电电压。
pub const BATTERY_FULL_CENTIVOLTS: u32 = 402;
/// 合法的最大电压读数（超过就不收进滤波窗口）。
pub const BATTERY_VALID_MAX_CENTIVOLTS: u32 = 420;
/// 电压滤波窗口样本数。
pub const VOLTAGE_FILTER_SAMPLES: usize = 5;
/// 至少这么多样本才算“稳定”。
pub const VOLTAGE_MIN_SAMPLES: usize = 3;
/// 充电电流模型的折扣系数（厂商保守取值）。
pub const CHARGE_PERCENT_MODEL_FACTOR: f64 = 0.5;
/// 两次样本超过这个间隔就不参与模型累加。
pub const CHARGE_PERCENT_MAX_SAMPLE_GAP_MS: u64 = 30_000;
/// 需要持续多久的充电证据。
pub const CHARGE_EVIDENCE_MS: u64 = 20_000;
/// 比静置基线上升多少算证据（要高于静置读数的正常抖动）。
pub const CHARGE_EVIDENCE_RISE_CENTIVOLTS: u32 = 3;
/// 静置基线允许跟随的偏差：读数在这个范围内就认为“还是静置状态”。
pub const BASELINE_FOLLOW_TOLERANCE_CENTIVOLTS: u32 = 2;
/// 上升证据的上限：更大的“上升”是读数从干扰值弹回来，不是充电。
pub const CHARGE_EVIDENCE_RISE_MAX_CENTIVOLTS: u32 = 60;
/// 比基线低多少算“被充电通路拉低”。
pub const CHARGE_EVIDENCE_DEVIATION_CENTIVOLTS: u32 = 10;
/// 证据消失多久后判为不在充电。
pub const CHARGE_SETTLED_MS: u64 = 30_000;
/// 低于这个电压的读数不可信（充电电流会把 VBAT 拉低）。
pub const CHARGE_MIN_CREDIBLE_CENTIVOLTS: u32 = 295;
/// 滤波窗口内极差不超过这个值才算稳定。
pub const CHARGE_EVIDENCE_MAX_SPREAD_CENTIVOLTS: u32 = 20;
/// 充电模型的百分比上限：100% 只能来自实测电压。
pub const MODEL_PERCENT_CAP: f64 = 99.0;

/// 一次上报算出来的电量状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BatteryReading {
    /// 展示用百分比。
    pub percent: Option<u8>,
    /// 百分比是否来自充电模型（界面加 `≈`）。
    pub percent_estimated: bool,
    /// 展示用电压（滤波值 / 充电前的基线），0.01 V。
    pub voltage_centivolts: Option<u32>,
    /// 状态机判定的“在充电”。
    pub charge_active: bool,
}

/// 电压 → 百分比（协议区间 2.80–4.02 V 线性，四舍五入并钳到 0–100）。
pub fn percent_from_voltage(voltage: u32) -> u8 {
    let span = BATTERY_FULL_CENTIVOLTS - BATTERY_EMPTY_CENTIVOLTS;
    let offset = voltage.saturating_sub(BATTERY_EMPTY_CENTIVOLTS);
    u8::try_from(((offset * 100 + span / 2) / span).min(100)).unwrap_or(100)
}

/// 每台设备的电量状态机。
#[derive(Debug, Default)]
pub struct BatteryModel {
    /// 最近几次电压样本 `(采样时间, 电压)`。
    voltage_samples: VecDeque<(u64, u32)>,
    /// 未充电时的静置基线。
    resting_voltage: Option<u32>,
    /// 当前展示值。
    battery_percent: Option<u8>,
    battery_voltage: Option<u32>,

    charge_active: bool,
    /// 证据累计起点与当时的基线。
    charge_evidence_at: u64,
    charge_baseline_voltage: Option<u32>,
    charge_baseline_percent: Option<u8>,
    /// 证据消失的起点。
    charge_settled_since: u64,
    /// 充电百分比模型。
    charge_model_percent: Option<f64>,
    charge_model_at: u64,
}

impl BatteryModel {
    /// 喂一条 `CHG:` 上报，返回这次要展示的状态。
    ///
    /// `voltage` 是原始 `VBAT`（0.01 V），`current_ma` 是固件配置的充电电流目标（`CUR`）。
    pub fn observe(
        &mut self,
        now_ms: u64,
        voltage: Option<u32>,
        current_ma: Option<u32>,
    ) -> BatteryReading {
        if let Some(voltage) = voltage {
            if voltage <= BATTERY_VALID_MAX_CENTIVOLTS {
                self.voltage_samples
                    .push_back((now_ms, voltage.max(BATTERY_EMPTY_CENTIVOLTS)));
                while self.voltage_samples.len() > VOLTAGE_FILTER_SAMPLES {
                    self.voltage_samples.pop_front();
                }
            }
        }

        let filtered = self.filtered_voltage();
        // 窗口里极差够小才算稳定（充电时读数会摆动）。
        let stable = if self.voltage_samples.len() < VOLTAGE_MIN_SAMPLES {
            false
        } else {
            let mut min = u32::MAX;
            let mut max = 0;
            for (_, value) in &self.voltage_samples {
                min = min.min(*value);
                max = max.max(*value);
            }
            max - min <= CHARGE_EVIDENCE_MAX_SPREAD_CENTIVOLTS
        };

        // 充电状态机（只看电压）。
        if self.update_charge_state(now_ms, filtered, stable) {
            // 刚进入充电：记下基线电量，模型从这里开始涨。
            if let Some(baseline) = self.resting_voltage {
                self.charge_baseline_voltage = Some(baseline);
                self.charge_baseline_percent = Some(percent_from_voltage(baseline));
                self.charge_model_percent = Some(f64::from(percent_from_voltage(baseline)));
                self.charge_model_at = now_ms;
            }
        }

        let mut filtered = filtered;
        let mut percent = self.battery_percent;
        if let (Some(voltage), true) = (filtered, stable) {
            let mut measured = percent_from_voltage(voltage);
            if self.charge_active {
                if let Some(baseline) = self.charge_baseline_percent {
                    if measured < baseline {
                        measured = baseline;
                        filtered = self.charge_baseline_voltage.or(filtered);
                    }
                }
            }
            percent = Some(measured);
            self.battery_percent = Some(measured);
            self.battery_voltage = filtered;
        } else if percent.is_none() {
            // 第一条就不可用：没有历史，只能空着。
        }

        // 充电百分比模型（按容量与充电电流目标推算）。
        let mut estimated = false;
        if self.charge_active {
            if let Some(mut model) = self.charge_model_percent {
                let elapsed = now_ms.saturating_sub(self.charge_model_at);
                let current = f64::from(current_ma.unwrap_or(0));
                if elapsed > 0
                    && elapsed <= CHARGE_PERCENT_MAX_SAMPLE_GAP_MS
                    && current > 0.0
                    && model < MODEL_PERCENT_CAP
                {
                    let gain = current * (elapsed as f64 / 1000.0) / 3600.0 / BATTERY_CAPACITY_MAH
                        * 100.0
                        * CHARGE_PERCENT_MODEL_FACTOR;
                    model = (model + gain).min(MODEL_PERCENT_CAP);
                    self.charge_model_percent = Some(model);
                }
                self.charge_model_at = now_ms;
                let model_percent = model.round() as u8;
                if percent.is_none_or(|value| model_percent > value) {
                    percent = Some(model_percent);
                    self.battery_percent = Some(model_percent);
                    if self.charge_baseline_voltage.is_some() {
                        self.battery_voltage = self.charge_baseline_voltage;
                    }
                }
                estimated = true;
            }
        }

        BatteryReading {
            percent,
            percent_estimated: estimated,
            voltage_centivolts: self.battery_voltage,
            charge_active: self.charge_active,
        }
    }

    /// 滤波后的电压：窗口内样本的中值（偶数是中间两个的平均）。
    fn filtered_voltage(&self) -> Option<u32> {
        if self.voltage_samples.is_empty() {
            return None;
        }
        let mut values: Vec<u32> = self
            .voltage_samples
            .iter()
            .map(|(_, value)| *value)
            .collect();
        values.sort_unstable();
        let middle = values.len() / 2;
        if values.len() % 2 == 1 {
            Some(values[middle])
        } else {
            Some((values[middle - 1] + values[middle]) / 2)
        }
    }

    /// 充电状态机。返回 `true` 表示这一步刚进入充电状态。
    ///
    /// 判据只看“读数相对静置基线是否被充电扰动”：
    /// 被拉低（`< 2.95 V` 或低 ≥0.10 V）、或小幅上升（0.01–0.60 V），并持续 20 秒。
    fn update_charge_state(&mut self, now_ms: u64, filtered: Option<u32>, stable: bool) -> bool {
        let Some(voltage) = filtered else {
            return false;
        };

        if !self.charge_active {
            // 没在充电：读数看起来还是静置状态（偏差在抖动范围内）才刷新基线。
            let still_resting = self.resting_voltage.is_none_or(|baseline| {
                voltage.abs_diff(baseline) <= BASELINE_FOLLOW_TOLERANCE_CENTIVOLTS
            });
            if stable && self.charge_evidence_at == 0 && still_resting {
                self.resting_voltage = Some(voltage);
            }
            let Some(baseline) = self.resting_voltage else {
                return false;
            };
            if !Self::has_charge_evidence(voltage, baseline) {
                self.charge_evidence_at = 0;
                return false;
            }
            if self.charge_evidence_at == 0 {
                self.charge_evidence_at = now_ms;
                return false;
            }
            if now_ms.saturating_sub(self.charge_evidence_at) < CHARGE_EVIDENCE_MS {
                return false;
            }
            self.charge_active = true;
            self.charge_settled_since = 0;
            return true;
        }

        // 已在充电：证据消失后要持续 30 秒才撤销。
        let baseline = self.charge_baseline_voltage.unwrap_or(voltage);
        if Self::has_charge_evidence(voltage, baseline) {
            self.charge_settled_since = 0;
            return false;
        }
        if self.charge_settled_since == 0 {
            self.charge_settled_since = now_ms;
            return false;
        }
        if now_ms.saturating_sub(self.charge_settled_since) < CHARGE_SETTLED_MS {
            return false;
        }
        // 充电结束：清空会话，让基线重新跟上当前读数。
        self.charge_active = false;
        self.charge_evidence_at = 0;
        self.charge_baseline_voltage = None;
        self.charge_baseline_percent = None;
        self.charge_settled_since = 0;
        self.charge_model_percent = None;
        self.charge_model_at = 0;
        if stable {
            self.resting_voltage = Some(voltage);
        }
        false
    }

    /// 这条读数是否可以证明“正在充电”。
    fn has_charge_evidence(voltage: u32, baseline: u32) -> bool {
        let rise = voltage.saturating_sub(baseline);
        let rising =
            (CHARGE_EVIDENCE_RISE_CENTIVOLTS..=CHARGE_EVIDENCE_RISE_MAX_CENTIVOLTS).contains(&rise);
        let pulled_down = baseline.saturating_sub(voltage) >= CHARGE_EVIDENCE_DEVIATION_CENTIVOLTS;
        let low_voltage = voltage < CHARGE_MIN_CREDIBLE_CENTIVOLTS;
        rising || pulled_down || low_voltage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 厂商设备每 5 秒上报一次。
    const STEP_MS: u64 = 5_000;

    /// 连续喂同样的读数，返回最后一次的结果。
    fn feed(
        model: &mut BatteryModel,
        from_step: u64,
        steps: u64,
        voltage: u32,
        current: u32,
    ) -> BatteryReading {
        let mut reading = BatteryReading::default();
        for step in 0..steps {
            let at = (from_step + step) * STEP_MS;
            reading = model.observe(at, Some(voltage), Some(current));
        }
        reading
    }

    /// 先让模型看到一段静置读数（未充电）。
    fn settle(model: &mut BatteryModel, voltage: u32) {
        feed(model, 1, 6, voltage, 100);
    }

    #[test]
    fn percent_from_voltage_matches_the_protocol_range() {
        assert_eq!(percent_from_voltage(280), 0);
        assert_eq!(percent_from_voltage(311), 25);
        assert_eq!(percent_from_voltage(401), 99);
        assert_eq!(percent_from_voltage(402), 100);
        assert_eq!(percent_from_voltage(420), 100);
    }

    #[test]
    fn a_stable_resting_voltage_is_not_charging() {
        // Light 实测：没插电、静置 4.01 V（`CHG` 是 1，但判定不看它）。
        let mut model = BatteryModel::default();
        let reading = feed(&mut model, 1, 40, 401, 100);
        assert!(!reading.charge_active, "静置电压稳定不能算充电");
        assert_eq!(reading.percent, Some(99));
    }

    #[test]
    fn a_voltage_pulled_down_by_the_charger_is_evidence() {
        // Light 实测：插电后 VBAT 掉到 2.66 V（充电通路把读数拉低）。
        let mut model = BatteryModel::default();
        settle(&mut model, 401);
        let mut reading = BatteryReading::default();
        for step in 0..40 {
            let at = (7 + step) * STEP_MS;
            reading = model.observe(at, Some(266), Some(100));
        }
        assert!(reading.charge_active, "2.66 V 应判成充电");
        // 读数不可信，但电量不能掉到 0%：保持基线附近。
        assert!(
            reading.percent.unwrap() >= 99,
            "应保持基线电量，实际 {:?}",
            reading.percent
        );
    }

    #[test]
    fn a_small_voltage_rise_is_evidence() {
        // Halo 实测：插电后 4.00 V → 4.04 V。
        let mut model = BatteryModel::default();
        settle(&mut model, 400);
        let mut reading = BatteryReading::default();
        for step in 0..40 {
            let at = (7 + step) * STEP_MS;
            reading = model.observe(at, Some(404), Some(100));
        }
        assert!(reading.charge_active, "小幅抬压应判成充电");
    }

    #[test]
    fn returning_to_the_resting_voltage_stops_charging() {
        let mut model = BatteryModel::default();
        settle(&mut model, 401);
        // 插电：读数被拉低 → 充电。
        let mut reading = feed(&mut model, 7, 30, 266, 100);
        assert!(reading.charge_active);
        // 拔电：读数回到 4.01 V（比基线高 135 个点，属于“跳回去”不是充电）。
        for at in 37..57 {
            reading = model.observe(at * STEP_MS, Some(401), Some(100));
        }
        assert!(!reading.charge_active, "回到静置电压后应停止显示充电");
        assert_eq!(reading.percent, Some(99));
    }

    #[test]
    fn plugging_in_again_is_detected_after_the_charge_ends() {
        let mut model = BatteryModel::default();
        settle(&mut model, 401);
        let mut reading = feed(&mut model, 7, 30, 266, 100);
        assert!(reading.charge_active);
        // 拔电并等到状态复位。
        for at in 37..57 {
            reading = model.observe(at * STEP_MS, Some(401), Some(100));
        }
        assert!(!reading.charge_active);
        // 再插上：仍然能被识别。
        for at in 57..87 {
            reading = model.observe(at * STEP_MS, Some(266), Some(100));
        }
        assert!(reading.charge_active, "再次插电应重新判成充电");
    }

    #[test]
    fn unstable_voltage_keeps_the_previous_percentage() {
        let mut model = BatteryModel::default();
        settle(&mut model, 401);
        let before = model.observe(7 * STEP_MS, Some(401), Some(100)).percent;
        assert_eq!(before, Some(99));
        // 充电通路把读数拉到 2.66 V：窗口极差 135 > 20 → 不稳定 → 保持上次值。
        let reading = model.observe(8 * STEP_MS, Some(266), Some(100));
        assert_eq!(reading.percent, Some(99));
    }

    #[test]
    fn charge_model_grows_slowly_and_never_reaches_100() {
        let mut model = BatteryModel::default();
        settle(&mut model, 380);
        // 插电后抬到 3.85 V：实测 86%，模型从基线 82% 按 10%/小时往上爬。
        let reading = feed(&mut model, 7, 720, 385, 100);
        assert!(reading.charge_active);
        assert!(reading.percent_estimated, "模型给的读数要标成估算值");
        let percent = reading.percent.unwrap();
        assert!(
            (90..=94).contains(&percent),
            "一小时大约涨 10 个点（82 → 92），实际 {percent}"
        );
        assert!(percent <= 99, "模型不能自己涨到 100%，实际 {percent}");
    }
}
