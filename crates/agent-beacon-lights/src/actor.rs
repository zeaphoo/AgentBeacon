//! 每设备一个 actor：latest-wins 执行命令计划，失败后退避重连并重放最后灯效。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_beacon_core::{DeviceProfile, Effect};
use tokio::sync::watch;

use crate::device::{mark_offline, mark_online, update, DeviceRegistry};
use crate::transport::Transport;
use crate::{plan_for, DesiredState, PlanOptions};

pub type TransportFactory = Arc<dyn Fn() -> Result<Box<dyn Transport>, String> + Send + Sync>;

/// 空闲时多久醒一次去读设备上报。
const REPORT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// AgentCore 设备每 5 s 发一次 `CHG:` 心跳；超过这个窗口一条上报都没有，
/// 就认为链路已经死了。断链不一定能触发读写错误（SPP/BLE 常见），
/// 必须靠心跳判活，否则界面会一直显示“已连接”。
const REPORT_TIMEOUT: Duration = Duration::from_secs(20);

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

/// 设备 actor 句柄。
///
/// actor 跑在自己的 OS 线程和 current-thread runtime 上，串口/BLE 的同步阻塞
/// （打开、握手、写超时、断链 join）不会占用主 tokio runtime 的 worker，
/// 因此 API/SSE 不会被某台掉线设备拖慢。
pub struct ActorHandle {
    shutdown: watch::Sender<bool>,
    finished: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// 线程退出（包括 panic）时把 `finished` 置位，避免管理循环永远等一个已死的 actor。
struct FinishGuard(Arc<AtomicBool>);

impl Drop for FinishGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl ActorHandle {
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    /// 请求 actor 退出。
    ///
    /// 不 `join`：它可能正卡在驱动 I/O 里，强行等待会把调用方（管理循环）拖住。
    /// 线程会在当前阻塞调用返回后看到停止信号并退出。
    pub fn abort(&mut self) {
        let _ = self.shutdown.send(true);
        self.thread.take();
    }
}

impl Drop for ActorHandle {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.thread.take();
    }
}

pub fn spawn(config: ActorConfig) -> ActorHandle {
    let (shutdown, shutdown_rx) = watch::channel(false);
    let finished = Arc::new(AtomicBool::new(false));
    let thread_finished = finished.clone();
    let id = config.id.clone();

    let thread = std::thread::Builder::new()
        .name(format!("device-{id}"))
        .spawn(move || {
            let _finish = FinishGuard(thread_finished);
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    tracing::error!(device = %config.id, error = %error, "failed to start device actor runtime");
                    return;
                }
            };
            runtime.block_on(run(config, shutdown_rx));
        });

    match thread {
        Ok(thread) => ActorHandle {
            shutdown,
            finished,
            thread: Some(thread),
        },
        Err(error) => {
            tracing::error!(device = %id, error = %error, "failed to spawn device actor thread");
            finished.store(true, Ordering::Relaxed);
            ActorHandle {
                shutdown,
                finished,
                thread: None,
            }
        }
    }
}

async fn run(mut config: ActorConfig, mut shutdown: watch::Receiver<bool>) {
    let mut transport: Option<Box<dyn Transport>> = None;
    // 电量/充电状态按厂商 AgentCoreHalo v2.5.2 的算法维护。
    let mut battery = crate::battery::BatteryModel::default();
    let mut applied: Option<(Effect, bool)> = None;
    let mut applied_brightness: Option<u8> = None;
    // 最近一次收到设备上报（含 `CHG:` / `STATUS:` / 状态回显）的时间。
    let mut last_rx = Instant::now();
    let mut backoff = 1u64;
    let _ = &config.port;

    loop {
        if *shutdown.borrow() {
            return;
        }
        if transport.is_none() {
            match (config.factory)() {
                Ok(mut opened) => {
                    backoff = 1;
                    applied = None;
                    let desired = config.desired.borrow().clone();
                    let target = desired.for_device(&config.id);
                    let mut failed = None;
                    // Halo 固件把 `BUSY` 默认画成偏黄的 `#FFD400`，连接时先下发灯效表
                    //（每个状态一条 `LC:`，`BUSY` = 橙色）；其余型号没有这条命令。
                    for bytes in initial_light_config(config.profile) {
                        if failed.is_some() {
                            break;
                        }
                        tracing::info!(
                            device = %config.id,
                            command = %String::from_utf8_lossy(&bytes).trim_end(),
                            "sending halo custom light table (LC:)"
                        );
                        if let Err(error) = opened.write(&bytes) {
                            failed = Some(error);
                        }
                    }
                    if failed.is_none() && config.profile.has_brightness_command() {
                        let bytes = brightness_bytes(config.profile, target.brightness);
                        if let Err(error) = opened.write(&bytes) {
                            failed = Some(error);
                        }
                    }
                    if let Some(error) = failed {
                        mark_error(&config.registry, &config.id, error);
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                            _ = shutdown.changed() => return,
                        }
                        continue;
                    }
                    if config.profile.has_brightness_command() {
                        applied_brightness = Some(target.brightness);
                    }
                    mark_online(&config.registry, &config.id, agent_beacon_core::now_ms());
                    // 重新计时：刚连上时设备还没发心跳，不能立即判超时。
                    last_rx = Instant::now();
                    if let Some(hook) = config.on_online.as_ref() {
                        hook();
                    }
                    transport = Some(opened);
                }
                Err(error) => {
                    mark_error(&config.registry, &config.id, error);
                    notify(&config);
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
                        _ = shutdown.changed() => return,
                    }
                    backoff = (backoff * 2).min(config.max_backoff_secs.max(1));
                    continue;
                }
            }
        }

        // 设备主动上报（`STATUS:`/`CHG:`）是异步到达的，每轮先读一次再决定要不要写。
        if let Some(writer) = transport.as_mut() {
            if ingest_reports(
                writer,
                &mut battery,
                &config.registry,
                &config.id,
                &mut last_rx,
            ) {
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
                _ = shutdown.changed() => return,
            }
            // 断链有两种来源：传输层自检失败（读线程 EOF/写失败），或者
            // AgentCore 心跳超时。只看 `is_connected()` 会漏掉“socket 还在但设备
            // 已经走了”的 SPP/BLE 场景。
            let link_ok = transport
                .as_ref()
                .is_none_or(|writer| writer.is_connected());
            if let Some(reason) = lost_link_reason(config.profile, link_ok, last_rx.elapsed()) {
                mark_offline(
                    &config.registry,
                    &config.id,
                    agent_beacon_core::now_ms(),
                    Some(format!("{} {reason}", config.port)),
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
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    _ = shutdown.changed() => return,
                }
                continue;
            }
            applied_brightness = Some(target.brightness);

            if applied == Some(key) {
                // Brightness-only change: put the light back into its current
                // state without the OFF prelude, so the effect does not flicker.
                let replay_plan = replay_plan(config.profile, effect);
                let mut replay_failed = false;
                for command in replay_plan.commands {
                    if *shutdown.borrow() {
                        return;
                    }
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
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                        _ = shutdown.changed() => return,
                    }
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
            if *shutdown.borrow() {
                return;
            }
            if command.delay_ms > 0 {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(command.delay_ms)) => {}
                    _ = config.desired.changed() => {
                        interrupted = true;
                        break;
                    }
                    _ = shutdown.changed() => return,
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
                    if ingest_reports(
                        writer,
                        &mut battery,
                        &config.registry,
                        &config.id,
                        &mut last_rx,
                    ) {
                        notify(&config);
                    }
                }
                Err(error) => {
                    mark_error(&config.registry, &config.id, error);
                    transport = None;
                    applied = None;
                    applied_brightness = None;
                    interrupted = true;
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                        _ = shutdown.changed() => return,
                    }
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

/// 连接时先写的“初始化”命令：Halo 需要逐条下发 `LC:` 灯效表（`BUSY` 橙色），
/// Light 与其他型号没有这条命令，直接跳过。
///
/// 固件把 `;` 当命令分隔符，一条 `LC:` 只能带一个状态，所以要发 6 条。
fn initial_light_config(profile: DeviceProfile) -> Vec<Vec<u8>> {
    if profile.is_agentcore_halo() {
        crate::agentcore::custom_light_commands()
    } else {
        Vec::new()
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

/// 链路是否已经不可用：传输层自检失败，或 AgentCore 电量心跳超时。
///
/// MLight / YH50 是二进制协议，没有心跳；它们靠写失败与串口枚举（worker 负责）判断。
fn lost_link_reason(
    profile: DeviceProfile,
    link_ok: bool,
    since_last_report: Duration,
) -> Option<String> {
    if !link_ok {
        return Some("link is down".to_string());
    }
    if profile.is_agentcore() && since_last_report > REPORT_TIMEOUT {
        return Some(format!(
            "sent no report for {} s; treating the link as down",
            since_last_report.as_secs()
        ));
    }
    None
}

/// 读走设备主动上报（AgentCore 的 `STATUS:` / `CHG:` 心跳）。
///
/// 不读会把系统缓冲撑满，所以即使不展示也必须消费；电量与固件版本顺手写进
/// `DeviceInfo`，控制台就能看到硬件信息。
fn ingest_reports(
    writer: &mut Box<dyn Transport>,
    model: &mut crate::battery::BatteryModel,
    registry: &DeviceRegistry,
    id: &str,
    last_rx: &mut Instant,
) -> bool {
    let mut changed = false;
    let lines = writer.drain_lines();
    if !lines.is_empty() {
        // 任何一行上报都说明链路是活的（不只是 `CHG:`），心跳判活用它。
        *last_rx = Instant::now();
    }
    for line in lines {
        match crate::agentcore::parse_report(&line) {
            crate::agentcore::Report::Battery(battery) => {
                let now = agent_beacon_core::now_ms();
                // 厂商 AgentCoreHalo v2.5.2 的算法：CHG 去抖 + 电压证据 → 充电状态，
                // 中值滤波 + 电流模型 → 展示电量（见 `battery.rs`）。
                // `CHG` 不参与判定（实测两边都不可靠），只作为诊断信息带上。
                let reading = model.observe(now, battery.millivolts, battery.current_ma);
                let reading = crate::BatteryInfo {
                    percent: reading.percent,
                    percent_estimated: reading.percent_estimated,
                    gauge_percent: battery.percent,
                    chg_flag: battery.usb_online,
                    charging: reading.charge_active,
                    millivolts: reading.voltage_centivolts,
                    millivolts_raw: battery.millivolts,
                    current_ma: battery.current_ma,
                    updated_at_ms: now,
                };
                // 心跳每 5 秒一次，只有读数真的变了才广播，避免无谓地刷 UI。
                changed |=
                    !current_battery(registry, id).is_some_and(|old| old.same_reading(&reading));
                let log_percent = reading.percent;
                let log_estimated = reading.percent_estimated;
                let log_charging = reading.charging;
                update(registry, id, |info| {
                    info.battery = Some(reading);
                    // 设备能上报电量说明链路是活的。
                    info.last_seen_ms = Some(now);
                });
                tracing::debug!(
                    device = id,
                    chg = battery.usb_online,
                    charging = log_charging,
                    millivolts_raw = battery.millivolts,
                    percent_raw = battery.percent,
                    percent = log_percent,
                    estimated = log_estimated,
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

    #[test]
    fn link_liveness_uses_transport_state_and_agentcore_heartbeat() {
        let fresh = Duration::from_secs(1);
        let stale = REPORT_TIMEOUT + Duration::from_secs(1);

        // 传输层说断了：任何型号都应判掉线。
        for profile in [
            DeviceProfile::AgentCore,
            DeviceProfile::Mlight3Led,
            DeviceProfile::Yh50,
        ] {
            assert!(lost_link_reason(profile, false, fresh).is_some());
        }
        // 心跳只有 AgentCore 系列有；MLight/YH50 不能因为“没有上报”被误判。
        assert!(lost_link_reason(DeviceProfile::AgentCore, true, fresh).is_none());
        assert!(lost_link_reason(DeviceProfile::AgentCoreHalo, true, stale).is_some());
        assert!(lost_link_reason(DeviceProfile::Mlight3Led, true, stale).is_none());
        assert!(lost_link_reason(DeviceProfile::Yh50, true, stale).is_none());
    }

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

    #[test]
    fn spawn_does_not_wait_for_a_blocking_factory() {
        let (gate_tx, gate_rx) = std::sync::mpsc::channel::<()>();
        let gate_rx = std::sync::Mutex::new(gate_rx);
        let (_desired_tx, desired_rx) = watch::channel(desired(Effect::Off, 50));
        let started = std::time::Instant::now();
        let mut actor = spawn(ActorConfig {
            id: "slow:0".into(),
            port: "slow:0".into(),
            profile: DeviceProfile::AgentCore,
            factory: Arc::new(move || {
                // 模拟 BLE 连接/握手卡住：spawn 必须立即返回。
                let _ = gate_rx.lock().unwrap().recv();
                Err("released".into())
            }),
            desired: desired_rx,
            registry: new_registry(),
            max_backoff_secs: 30,
            on_online: None,
            on_change: None,
        });

        assert!(started.elapsed() < Duration::from_millis(100));
        actor.abort();
        gate_tx.send(()).unwrap();

        for _ in 0..100 {
            if actor.is_finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(actor.is_finished(), "abort 后 actor 线程应退出");
    }

    #[tokio::test]
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

    #[tokio::test]
    async fn halo_writes_custom_light_config_before_brightness() {
        let log = RecordingLog::default();
        let recorder = log.clone();
        let registry = new_registry();
        let (_desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 60));

        let _actor = spawn(ActorConfig {
            id: "halo:0".into(),
            port: "halo:0".into(),
            profile: DeviceProfile::AgentCoreHalo,
            factory: Arc::new(move || Ok(Box::new(RecordingTransport::new(recorder.clone())))),
            desired: desired_rx,
            registry,
            max_backoff_secs: 30,
            on_online: None,
            on_change: None,
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        let entries = log.entries();
        // 灯效表拆成 6 条（`;` 在固件里是命令分隔符），逐条跟在亮度命令之前。
        let expected: Vec<String> = crate::agentcore::default_custom_light_rows()
            .iter()
            .map(|row| crate::hex(format!("LC:{row}\n").as_bytes()))
            .collect();
        assert_eq!(
            entries.get(..expected.len()),
            Some(expected.as_slice()),
            "连接后先逐条下发灯效表（BUSY 橙色）：{entries:?}"
        );
        assert!(
            entries.len() >= expected.len() + 2,
            "亮度命令与状态命令应跟在灯效表之后：{entries:?}"
        );
    }

    #[tokio::test]
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
        // 三次采样 387/387/390 → 中值 387 → 88%；`CHG` 只出现两轮候选，
        // 没达到 3 条 + 20 秒证据，所以不算充电。
        assert_eq!(battery.percent, Some(88));
        assert!(!battery.charging, "静置电压不能判成充电");
        // 只有两次真正的变化会通知控制台刷新。
        assert_eq!(*hits.lock().unwrap(), 2);
        drop(desired_tx);
    }

    #[tokio::test]
    async fn latest_wins_aborts_stale_plan() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("ml:0", "/dev/fake", DeviceProfile::Mlight3Led),
        );
        let log = RecordingLog::default();
        let factory_log = log.clone();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 50));

        let mut actor = spawn(ActorConfig {
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

    #[tokio::test]
    async fn agentcore_sends_state_names_and_scaled_brightness() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("ac:0", "/dev/fake", DeviceProfile::AgentCore),
        );
        let log = RecordingLog::default();
        let factory_log = log.clone();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 50));

        let mut actor = spawn(ActorConfig {
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

    #[tokio::test]
    async fn brightness_change_replays_current_state_without_off() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("ml:0", "/dev/fake", DeviceProfile::Mlight3Led),
        );
        let log = RecordingLog::default();
        let factory_log = log.clone();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 100));

        let mut actor = spawn(ActorConfig {
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
