//! 每设备一个 actor：latest-wins 执行命令计划，失败后退避重连并重放最后灯效。

use std::sync::Arc;
use std::time::Duration;

use agent_beacon_core::{DeviceProfile, Effect};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::device::{mark_offline, mark_online, update, DeviceRegistry};
use crate::transport::Transport;
use crate::{plan_for, DesiredState, PlanOptions};

pub type TransportFactory = Arc<dyn Fn() -> Result<Box<dyn Transport>, String> + Send + Sync>;

/// 空闲时多久醒一次去读设备上报。
const REPORT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// 每次成功连上后调用一次；由上层决定要不要把设备放进可见列表。
pub type OnlineHook = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct ActorConfig {
    pub id: String,
    pub port: String,
    pub profile: DeviceProfile,
    pub factory: TransportFactory,
    pub desired: watch::Receiver<DesiredState>,
    pub registry: DeviceRegistry,
    /// 重连退避的上限：有线设备可以频繁重试，蓝牙要给适配器留余地。
    pub max_backoff_secs: u64,
    /// 连上时回调（设备首次连上才需要出现在列表里时用）。
    pub on_online: Option<OnlineHook>,
    /// 设备上报发生变化时回调（电量、充电状态、固件、错误），由上层广播给控制台。
    pub on_change: Option<OnlineHook>,
}

pub fn spawn(config: ActorConfig) -> JoinHandle<()> {
    tokio::spawn(run(config))
}

async fn run(mut config: ActorConfig) {
    let mut transport: Option<Box<dyn Transport>> = None;
    let mut applied: Option<(Effect, bool)> = None;
    let mut applied_brightness: Option<u8> = None;
    let mut backoff = 1u64;
    let _ = &config.port;

    loop {
        if transport.is_none() {
            match (config.factory)() {
                Ok(mut opened) => {
                    backoff = 1;
                    applied = None;
                    let desired = config.desired.borrow().clone();
                    let target = desired.for_device(&config.id);
                    let mut failed = None;
                    if config.profile.has_brightness_command() {
                        let bytes = brightness_bytes(config.profile, target.brightness);
                        if let Err(error) = opened.write(&bytes) {
                            failed = Some(error);
                        }
                    }
                    if let Some(error) = failed {
                        mark_error(&config.registry, &config.id, error);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    if config.profile.has_brightness_command() {
                        applied_brightness = Some(target.brightness);
                    }
                    mark_online(&config.registry, &config.id, agent_beacon_core::now_ms());
                    if let Some(hook) = config.on_online.as_ref() {
                        hook();
                    }
                    transport = Some(opened);
                }
                Err(error) => {
                    mark_error(&config.registry, &config.id, error);
                    notify(&config);
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(config.max_backoff_secs.max(1));
                    continue;
                }
            }
        }

        // 设备主动上报（`STATUS:`/`CHG:`）是异步到达的，每轮先读一次再决定要不要写。
        if let Some(writer) = transport.as_mut() {
            if ingest_reports(writer, &config.registry, &config.id) {
                notify(&config);
            }
        }

        let desired = config.desired.borrow_and_update().clone();
        let target = desired.for_device(&config.id);
        let effect = target.effect;
        let key = (effect, target.buzzer_on_alert);
        let brightness_changed = config.profile.has_brightness_command()
            && applied_brightness != Some(target.brightness);

        if applied == Some(key) && !brightness_changed {
            // 设备会主动上报（`CHG:` 每 5 s、`STATUS:` 变更后），所以即使没有新的
            // 期望状态也要定期醒一次把上报读走，顺便发现断链。
            tokio::select! {
                changed = config.desired.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                _ = tokio::time::sleep(REPORT_POLL_INTERVAL) => {}
            }
            // BLE 断链时设备侧不会主动写失败，靠链路自检发现，避免界面一直显示“已连接”。
            let down = transport
                .as_ref()
                .is_some_and(|writer| !writer.is_connected());
            if down {
                mark_offline(
                    &config.registry,
                    &config.id,
                    agent_beacon_core::now_ms(),
                    Some(format!("{} link is down", config.port)),
                );
                transport = None;
                applied = None;
                applied_brightness = None;
            }
            continue;
        }

        if brightness_changed {
            let Some(writer) = transport.as_mut() else {
                applied_brightness = None;
                continue;
            };
            if let Err(error) = writer.write(&brightness_bytes(config.profile, target.brightness)) {
                mark_error(&config.registry, &config.id, error);
                transport = None;
                applied = None;
                applied_brightness = None;
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            applied_brightness = Some(target.brightness);

            if applied == Some(key) {
                // Brightness-only change: put the light back into its current
                // state without the OFF prelude, so the effect does not flicker.
                let replay_plan = replay_plan(config.profile, effect);
                let mut replay_failed = false;
                for command in replay_plan.commands {
                    if command.delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(command.delay_ms)).await;
                    }
                    let Some(writer) = transport.as_mut() else {
                        replay_failed = true;
                        break;
                    };
                    if let Err(error) = writer.write(&command.bytes) {
                        mark_error(&config.registry, &config.id, error);
                        transport = None;
                        applied = None;
                        applied_brightness = None;
                        replay_failed = true;
                        break;
                    }
                }
                if replay_failed {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                continue;
            }
        }

        let plan = plan_for(
            config.profile,
            effect,
            PlanOptions {
                brightness: target.brightness,
                buzzer_on_alert: target.buzzer_on_alert,
            },
        );

        let mut interrupted = false;
        for command in plan.commands {
            if command.delay_ms > 0 {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(command.delay_ms)) => {}
                    _ = config.desired.changed() => {
                        interrupted = true;
                        break;
                    }
                }
            }
            if interrupted {
                break;
            }

            let Some(writer) = transport.as_mut() else {
                interrupted = true;
                break;
            };

            match writer.write(&command.bytes) {
                Ok(()) => {
                    ingest_reports(writer, &config.registry, &config.id);
                }
                Err(error) => {
                    mark_error(&config.registry, &config.id, error);
                    transport = None;
                    applied = None;
                    applied_brightness = None;
                    interrupted = true;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    break;
                }
            }
        }

        if interrupted {
            continue;
        }

        applied = Some(key);
        update(&config.registry, &config.id, |info| {
            info.effect = effect;
            info.connected = true;
            info.last_error = None;
            info.last_seen_ms = Some(agent_beacon_core::now_ms());
        });
    }
}

/// 亮度命令按协议分派：MLight 用 `BRIGHTNESS_n`，AgentCore-Light 用 `BRIGHTNESS_LEVEL:n(×3)`。
fn brightness_bytes(profile: DeviceProfile, percent: u8) -> Vec<u8> {
    if profile.is_agentcore() {
        crate::agentcore::brightness_command(percent)
    } else {
        crate::mlight::brightness_command(percent)
    }
}

/// 亮度调整后的重放：MLight 不带 `OFF` 清场，AgentCore-Light 重发一次状态名。
fn replay_plan(profile: DeviceProfile, effect: Effect) -> crate::CommandPlan {
    if profile.is_agentcore() {
        crate::agentcore::replay(effect)
    } else {
        crate::mlight::replay(effect, profile == DeviceProfile::Mlight3Led)
    }
}

/// 读走设备主动上报（AgentCore 的 `STATUS:` / `CHG:` 心跳）。
///
/// 不读会把系统缓冲撑满，所以即使不展示也必须消费；电量与固件版本顺手写进
/// `DeviceInfo`，控制台就能看到硬件信息。
fn ingest_reports(writer: &mut Box<dyn Transport>, registry: &DeviceRegistry, id: &str) -> bool {
    let mut changed = false;
    for line in writer.drain_lines() {
        match crate::agentcore::parse_report(&line) {
            crate::agentcore::Report::Battery(battery) => {
                let now = agent_beacon_core::now_ms();
                let reading = crate::BatteryInfo {
                    percent: display_percent(battery.percent, battery.millivolts),
                    usb_online: battery.usb_online,
                    millivolts: battery.millivolts,
                    current_ma: battery.current_ma,
                    updated_at_ms: now,
                };
                // 心跳每 5 秒一次，只有读数真的变了才广播，避免无谓地刷 UI。
                changed |=
                    !current_battery(registry, id).is_some_and(|old| old.same_reading(&reading));
                update(registry, id, |info| {
                    info.battery = Some(reading);
                    // 设备能上报电量说明链路是活的。
                    info.last_seen_ms = Some(now);
                });
                tracing::debug!(
                    device = id,
                    usb_online = battery.usb_online,
                    millivolts = battery.millivolts,
                    percent = battery.percent,
                    "agentcore battery report"
                );
            }
            crate::agentcore::Report::Status(status) => {
                if let Some(firmware) = status.firmware.clone() {
                    let differs = registry
                        .lock()
                        .ok()
                        .and_then(|devices| {
                            devices
                                .get(id)
                                .map(|device| device.firmware.as_deref() != Some(firmware.as_str()))
                        })
                        .unwrap_or(false);
                    changed |= differs;
                    update(registry, id, |info| {
                        info.firmware = Some(firmware);
                    });
                }
                tracing::debug!(
                    device = id,
                    state = status.state.as_deref(),
                    firmware = status.firmware.as_deref(),
                    ble_state = status.ble_state,
                    "agentcore status report"
                );
            }
            crate::agentcore::Report::State(state) => {
                tracing::debug!(device = id, state = %state, "agentcore state echo")
            }
            crate::agentcore::Report::Settings(settings) => tracing::debug!(
                device = id,
                brightness_level = settings.brightness_level,
                buzzer = settings.buzzer,
                stored_in_vm = settings.stored_in_vm,
                "agentcore settings report"
            ),
            crate::agentcore::Report::Sleep(sleep) => tracing::debug!(
                device = id,
                idle_sleep_secs = sleep.idle_sleep_secs,
                sleeping = sleep.sleeping,
                "agentcore sleep settings report"
            ),
            crate::agentcore::Report::Error(error) => {
                tracing::warn!(device = id, error = %error, "agentcore rejected a command")
            }
            _ => {}
        }
    }
    changed
}

/// 界面上显示的电量：优先用电压换算。
///
/// 电量计芯片的百分比不一定校准过（协议文档里的实测样例就是 `VBAT:311,PER:0`），
/// 厂商上位机也是按 2.80–4.02 V 线性换算：`(VBAT - 280) * 100 / (402 - 280)`。
pub fn display_percent(reported: Option<u8>, millivolts: Option<u32>) -> Option<u8> {
    const EMPTY_CENTIVOLTS: u32 = 280;
    const FULL_CENTIVOLTS: u32 = 402;
    match millivolts {
        Some(voltage) => {
            let span = FULL_CENTIVOLTS - EMPTY_CENTIVOLTS;
            let offset = voltage.saturating_sub(EMPTY_CENTIVOLTS);
            Some(((offset * 100 + span / 2) / span).min(100) as u8)
        }
        None => reported,
    }
}

/// 读当前登记的电量读数（没有登记过则为 `None`）。
fn current_battery(registry: &DeviceRegistry, id: &str) -> Option<crate::BatteryInfo> {
    registry
        .lock()
        .ok()
        .and_then(|devices| devices.get(id).and_then(|info| info.battery.clone()))
}

fn notify(config: &ActorConfig) {
    if let Some(hook) = config.on_change.as_ref() {
        hook();
    }
}

fn mark_error(registry: &DeviceRegistry, id: &str, error: String) {
    tracing::warn!(device = id, error = %error, "device unavailable");
    let now = agent_beacon_core::now_ms();
    update(registry, id, |info| {
        if info.connected {
            info.last_seen_ms = Some(now);
        }
    });
    mark_offline(registry, id, now, Some(error));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{new_registry, snapshot, upsert};
    use crate::DeviceInfo;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Clone, Default)]
    struct RecordingLog {
        entries: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingLog {
        fn record(&self, bytes: &[u8]) {
            if let Ok(mut entries) = self.entries.lock() {
                entries.push(crate::hex(bytes));
            }
        }

        fn entries(&self) -> Vec<String> {
            self.entries
                .lock()
                .map(|entries| entries.clone())
                .unwrap_or_default()
        }
    }

    struct RecordingTransport {
        log: RecordingLog,
    }

    impl RecordingTransport {
        fn new(log: RecordingLog) -> Self {
            Self { log }
        }
    }

    impl Transport for RecordingTransport {
        fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
            self.log.record(bytes);
            Ok(())
        }

        fn describe(&self) -> String {
            "test".to_string()
        }
    }

    /// 每次 `drain_lines` 依次吐出一行上报，用完之后安静（模拟设备心跳）。
    struct ReportingTransport {
        lines: Arc<Mutex<Vec<String>>>,
    }

    impl ReportingTransport {
        fn new(lines: Vec<String>) -> Self {
            Self {
                lines: Arc::new(Mutex::new(lines)),
            }
        }
    }

    impl Transport for ReportingTransport {
        fn write(&mut self, _bytes: &[u8]) -> Result<(), String> {
            Ok(())
        }

        fn describe(&self) -> String {
            "test-reporting".to_string()
        }

        fn drain_lines(&mut self) -> Vec<String> {
            let mut lines = self.lines.lock().expect("lines");
            if lines.is_empty() {
                return Vec::new();
            }
            vec![lines.remove(0)]
        }
    }

    fn desired(effect: Effect, brightness: u8) -> DesiredState {
        DesiredState {
            default: crate::DeviceDesired {
                effect,
                brightness,
                buzzer_on_alert: false,
            },
            ..DesiredState::default()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn online_hook_fires_on_each_successful_connect() {
        let registry = new_registry();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Calm, 50));
        let hook_hits = Arc::new(Mutex::new(0usize));
        let hook_counter = hook_hits.clone();
        let hook_registry = registry.clone();

        let _actor = spawn(ActorConfig {
            id: "ble:aa".into(),
            port: "ble:aa".into(),
            profile: DeviceProfile::AgentCore,
            factory: Arc::new(|| Ok(Box::new(RecordingTransport::new(RecordingLog::default())))),
            desired: desired_rx,
            registry: registry.clone(),
            max_backoff_secs: 60,
            on_change: None,
            on_online: Some(Arc::new(move || {
                if let Ok(mut hits) = hook_counter.lock() {
                    *hits += 1;
                }
                // 与 worker 一样：连上才登记，于是设备只会在真的可用时出现在列表里。
                let mut info = DeviceInfo::new("ble:aa", "ble:aa", DeviceProfile::AgentCore);
                info.connected = true;
                upsert(&hook_registry, info);
            })),
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        // 这里没有真实设备：上层据此把设备放进可见列表，所以必须每连上就触发一次。
        assert_eq!(*hook_hits.lock().unwrap(), 1);
        let devices = snapshot(&registry);
        assert_eq!(devices.len(), 1);
        assert!(devices[0].connected);
        drop(desired_tx);
    }

    #[test]
    fn displayed_percent_prefers_the_voltage_curve() {
        // 未校准的电量计：PER 报到 0，但 3.11 V 实际约 25%。
        assert_eq!(display_percent(Some(0), Some(311)), Some(25));
        // 满电：4.07 V 超过 4.02 V 上限，钳到 100。
        assert_eq!(display_percent(Some(100), Some(407)), Some(100));
        // 空电。
        assert_eq!(display_percent(Some(0), Some(280)), Some(0));
        // 没有电压时才退回电量计读数。
        assert_eq!(display_percent(Some(42), None), Some(42));
    }

    #[tokio::test(start_paused = true)]
    async fn battery_heartbeat_notifies_only_when_the_reading_changes() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("ac:0", "/dev/fake", DeviceProfile::AgentCore),
        );
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Calm, 50));
        let hits = Arc::new(Mutex::new(0usize));
        let counter = hits.clone();

        let _actor = spawn(ActorConfig {
            id: "ac:0".into(),
            port: "/dev/fake".into(),
            profile: DeviceProfile::AgentCore,
            factory: Arc::new(|| {
                Ok(Box::new(ReportingTransport::new(vec![
                    "CHG:1,VBAT:387,PER:82,CUR:100".to_string(),
                    // 同一条心跳重复上报：不应触发刷新。
                    "CHG:1,VBAT:387,PER:82,CUR:100".to_string(),
                    // 充电结束：电量/充电状态都变了。
                    "CHG:0,VBAT:390,PER:83,CUR:0".to_string(),
                ])))
            }),
            desired: desired_rx,
            registry: registry.clone(),
            max_backoff_secs: 30,
            on_online: None,
            on_change: Some(Arc::new(move || {
                if let Ok(mut hits) = counter.lock() {
                    *hits += 1;
                }
            })),
        });

        // 心跳每秒被读一次，三行分别在第 1/2/3 轮消费。
        tokio::time::sleep(Duration::from_millis(3_500)).await;
        let devices = snapshot(&registry);
        let battery = devices[0].battery.clone().expect("电量应已登记");
        // 3.90 V 按电量曲线换算约 90%。
        assert_eq!(battery.percent, Some(90));
        assert!(!battery.usb_online);
        // 只有两次真正的变化会通知控制台刷新。
        assert_eq!(*hits.lock().unwrap(), 2);
        drop(desired_tx);
    }

    #[tokio::test(start_paused = true)]
    async fn latest_wins_aborts_stale_plan() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("ml:0", "/dev/fake", DeviceProfile::Mlight3Led),
        );
        let log = RecordingLog::default();
        let factory_log = log.clone();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 50));

        let actor = spawn(ActorConfig {
            id: "ml:0".into(),
            port: "/dev/fake".into(),
            profile: DeviceProfile::Mlight3Led,
            factory: Arc::new(move || Ok(Box::new(RecordingTransport::new(factory_log.clone())))),
            desired: desired_rx,
            registry,
            max_backoff_secs: 30,
            on_online: None,
            on_change: None,
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        desired_tx.send(desired(Effect::Alert, 50)).unwrap();
        tokio::time::sleep(Duration::from_millis(1_500)).await;

        let entries = log.entries();
        assert!(
            entries.iter().any(|entry| entry == "4F 46 46 0A"),
            "{entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry == "52 45 44 5F 42 4C 49 4E 4B 0A"),
            "{entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|entry| entry == "47 52 45 45 4E 5F 4F 4E 0A"),
            "旧计划不应继续执行：{entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|entry| entry == "59 45 4C 4C 4F 57 5F 42 52 45 41 54 48 0A"),
            "旧计划不应继续执行：{entries:?}"
        );

        actor.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn agentcore_sends_state_names_and_scaled_brightness() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("ac:0", "/dev/fake", DeviceProfile::AgentCore),
        );
        let log = RecordingLog::default();
        let factory_log = log.clone();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 50));

        let actor = spawn(ActorConfig {
            id: "ac:0".into(),
            port: "/dev/fake".into(),
            profile: DeviceProfile::AgentCore,
            factory: Arc::new(move || Ok(Box::new(RecordingTransport::new(factory_log.clone())))),
            desired: desired_rx,
            registry: registry.clone(),
            max_backoff_secs: 30,
            on_online: None,
            on_change: None,
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        let entries = log.entries();
        // 50% → "BRIGHTNESS_LEVEL:150\n"
        assert!(
            entries
                .iter()
                .any(|entry| entry
                    == "42 52 49 47 48 54 4E 45 53 53 5F 4C 45 56 45 4C 3A 31 35 30 0A"),
            "{entries:?}"
        );
        assert!(
            entries.iter().any(|entry| entry == "42 55 53 59 0A"),
            "{entries:?}"
        );

        desired_tx.send(desired(Effect::Attention, 100)).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entries = log.entries();
        // "WAIT_CONFIRM\n"
        assert!(
            entries
                .iter()
                .any(|entry| entry == "57 41 49 54 5F 43 4F 4E 46 49 52 4D 0A"),
            "{entries:?}"
        );
        // "BRIGHTNESS_LEVEL:300\n"
        assert!(
            entries
                .iter()
                .any(|entry| entry
                    == "42 52 49 47 48 54 4E 45 53 53 5F 4C 45 56 45 4C 3A 33 30 30 0A"),
            "{entries:?}"
        );
        assert_eq!(snapshot(&registry)[0].effect, Effect::Attention);

        actor.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn brightness_change_replays_current_state_without_off() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("ml:0", "/dev/fake", DeviceProfile::Mlight3Led),
        );
        let log = RecordingLog::default();
        let factory_log = log.clone();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 100));

        let actor = spawn(ActorConfig {
            id: "ml:0".into(),
            port: "/dev/fake".into(),
            profile: DeviceProfile::Mlight3Led,
            factory: Arc::new(move || Ok(Box::new(RecordingTransport::new(factory_log.clone())))),
            desired: desired_rx,
            registry,
            max_backoff_secs: 30,
            on_online: None,
            on_change: None,
        });

        // Let the initial effect plan finish.
        tokio::time::sleep(Duration::from_millis(1_000)).await;

        desired_tx.send(desired(Effect::Busy, 50)).unwrap();
        tokio::time::sleep(Duration::from_millis(1_000)).await;

        let entries = log.entries();
        let brightness = "42 52 49 47 48 54 4E 45 53 53 5F 35 30 0A";
        let index = entries
            .iter()
            .rposition(|entry| entry == brightness)
            .expect("应下发 BRIGHTNESS_50");

        assert!(
            entries[index + 1..]
                .iter()
                .any(|entry| entry == "47 52 45 45 4E 5F 4F 4E 0A"),
            "亮度调整后应重放 GREEN_ON：{entries:?}"
        );
        assert!(
            entries[index + 1..]
                .iter()
                .any(|entry| entry == "59 45 4C 4C 4F 57 5F 42 52 45 41 54 48 0A"),
            "亮度调整后应重放 YELLOW_BREATH：{entries:?}"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| *entry == "4F 46 46 0A")
                .count(),
            1,
            "亮度调整不应再次发送 OFF：{entries:?}"
        );

        actor.abort();
    }
}
