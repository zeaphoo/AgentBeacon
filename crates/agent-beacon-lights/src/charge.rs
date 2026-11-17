//! 充电状态判定。
//!
//! AgentCore 的 `CHG` 字段只是固件的“充电输入检测”标志：充满后仍为 1，检测脚悬空时也会
//! 误报（实测一台 AgentCore-Light 未接 USB 却一直上报 `CHG:1`，同时电压在缓慢下降）。
//! 厂商上位机对此的注释是：
//!
//! > `CHG` comes from the firmware's charge-input flag and can be true with a floating
//! > detect pin. A steady reading at the resting voltage is not evidence.
//!
//! 所以这里只看**电压确实在往上走**才认为在充电：在一个滑动窗口里，当前电压比窗口内的
//! 最低电压高出至少 [`RISE_CENTIVOLTS`]，并且这个最低点已经至少有 [`EVIDENCE_MS`] 那么旧
//! （避免单次抖动误判）。电压稳住超过 [`HOLD_MS`] 就不再声称正在充电（充满后的恒压阶段）。

use std::collections::VecDeque;

/// 观察窗口：只和最近这段时间的电压比较。
pub const WINDOW_MS: u64 = 300_000;
/// 判定“在上升”所需的最小涨幅（0.01 V 单位）。
pub const RISE_CENTIVOLTS: u32 = 3;
/// 最低点至少要这么旧，才算“持续上升”的证据。
pub const EVIDENCE_MS: u64 = 20_000;
/// 已经判为充电后，电压稳住多久算充满/停止充电。
pub const HOLD_MS: u64 = 120_000;

/// 单路设备的充电状态机。
#[derive(Debug, Default)]
pub struct ChargeTracker {
    /// `(采样时间, 电压 0.01 V)`，按时间递增。
    samples: VecDeque<(u64, u32)>,
    charging: bool,
    charging_since: u64,
}

impl ChargeTracker {
    /// 喂一条电量心跳，返回“是否正在充电”。
    pub fn observe(&mut self, now_ms: u64, chg_present: bool, voltage: Option<u32>) -> bool {
        if !chg_present {
            self.samples.clear();
            self.charging = false;
            self.charging_since = 0;
            return false;
        }
        let Some(voltage) = voltage else {
            return self.charging;
        };
        self.samples.push_back((now_ms, voltage));
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now_ms.saturating_sub(*at) > WINDOW_MS)
        {
            self.samples.pop_front();
        }

        let (min_voltage, min_at) =
            self.samples
                .iter()
                .fold((voltage, now_ms), |acc, (at, value)| {
                    if *value < acc.0 {
                        (*value, *at)
                    } else {
                        acc
                    }
                });
        let rising = voltage.saturating_sub(min_voltage) >= RISE_CENTIVOLTS
            && now_ms.saturating_sub(min_at) >= EVIDENCE_MS;

        if rising {
            if !self.charging {
                self.charging = true;
            }
            self.charging_since = now_ms;
        } else if self.charging && now_ms.saturating_sub(self.charging_since) >= HOLD_MS {
            // 电压不再上升：要么进入恒压阶段，要么早就充满了。
            self.charging = false;
        }
        self.charging
    }

    pub fn charging(&self) -> bool {
        self.charging
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steady_reading_is_not_charging() {
        // 检测脚误报：CHG=1 但电压稳定（甚至缓慢下降），不能判成充电。
        let mut tracker = ChargeTracker::default();
        for step in 0..60 {
            let voltage = 407 - (step / 20) as u32; // 5 分钟内从 4.07 V 掉到 4.05 V
            assert!(
                !tracker.observe(step * 5_000, true, Some(voltage)),
                "step {step}"
            );
        }
    }

    #[test]
    fn a_rising_voltage_is_charging_once_the_evidence_is_old_enough() {
        let mut tracker = ChargeTracker::default();
        // 插上电：3.97 V 起步，10 秒后开始往上爬。
        assert!(!tracker.observe(0, true, Some(397)));
        assert!(!tracker.observe(10_000, true, Some(398)));
        // 最低点还不到 20 秒，先不算。
        assert!(!tracker.observe(15_000, true, Some(402)));
        assert!(tracker.observe(25_000, true, Some(403)));
        assert!(tracker.charging());
    }

    #[test]
    fn unplugging_clears_the_state() {
        let mut tracker = ChargeTracker::default();
        tracker.observe(0, true, Some(390));
        assert!(tracker.observe(30_000, true, Some(395)));

        assert!(!tracker.observe(35_000, false, Some(395)));
        assert!(!tracker.charging());
        // 再插上时从新的基线开始，不会因为旧样本立刻判成充电。
        assert!(!tracker.observe(40_000, true, Some(395)));
    }

    #[test]
    fn charging_stops_after_the_voltage_settles() {
        let mut tracker = ChargeTracker::default();
        tracker.observe(0, true, Some(390));
        assert!(tracker.observe(30_000, true, Some(395)));
        // 恒压阶段：每 5 秒继续上报 3.95 V，等低点滑出观察窗口、再过一段稳定期后
        // 就不再声称“正在充电”。
        let mut at = 35_000;
        while at <= 500_000 {
            tracker.observe(at, true, Some(395));
            at += 5_000;
        }
        assert!(!tracker.charging());
    }
}
