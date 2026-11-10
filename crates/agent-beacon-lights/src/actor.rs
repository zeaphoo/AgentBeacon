//! 每设备一个 actor：latest-wins 执行命令计划，失败后退避重连并重放最后灯效。

use std::sync::Arc;
use std::time::Duration;

use agent_beacon_core::{DeviceProfile, Effect};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::device::{update, DeviceRegistry};
use crate::transport::Transport;
use crate::{hex, plan_for, DesiredState, PlanOptions};

pub type TransportFactory = Arc<dyn Fn() -> Result<Box<dyn Transport>, String> + Send + Sync>;

/// 空闲时多久醒一次去读设备上报。
const REPORT_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub struct ActorConfig {
    pub id: String,
    pub port: String,
    pub profile: DeviceProfile,
    pub factory: TransportFactory,
    pub desired: watch::Receiver<DesiredState>,
    pub registry: DeviceRegistry,
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
                    update(&config.registry, &config.id, |info| {
                        info.connected = true;
                        info.last_error = None;
                        info.last_seen_ms = Some(agent_beacon_core::now_ms());
                    });
                    transport = Some(opened);
                }
                Err(error) => {
                    mark_error(&config.registry, &config.id, error);
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(30);
                    continue;
                }
            }
        }

        // 设备主动上报（`STATUS:`/`CHG:`）是异步到达的，每轮先读一次再决定要不要写。
        if let Some(writer) = transport.as_mut() {
            log_reports(writer, &config.id);
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
                    log_reports(writer, &config.id);
                    if config.profile == DeviceProfile::Mock {
                        update(&config.registry, &config.id, |info| {
                            info.mock_commands.push(hex(&command.bytes));
                            if info.mock_commands.len() > 50 {
                                info.mock_commands.remove(0);
                            }
                        });
                    }
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

/// 读走设备主动上报（AgentCore-Light 的 `STATUS:` / `CHG:` 心跳），只写诊断日志。
///
/// 不读会把系统缓冲撑满，所以即使不展示也必须消费。
fn log_reports(writer: &mut Box<dyn Transport>, id: &str) {
    for line in writer.drain_lines() {
        match crate::agentcore::parse_report(&line) {
            crate::agentcore::Report::Battery(battery) => tracing::debug!(
                device = id,
                charging = battery.charging,
                millivolts = battery.millivolts,
                percent = battery.percent,
                "agentcore battery report"
            ),
            crate::agentcore::Report::Status(status) => tracing::debug!(
                device = id,
                state = status.state.as_deref(),
                firmware = status.firmware.as_deref(),
                ble_state = status.ble_state,
                "agentcore status report"
            ),
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
}

fn mark_error(registry: &DeviceRegistry, id: &str, error: String) {
    tracing::warn!(device = id, error = %error, "device unavailable");
    update(registry, id, |info| {
        // 只在 online -> offline 的瞬间记录断开时间；重试期间不要不断刷新宽限期。
        if info.connected {
            info.last_seen_ms = Some(agent_beacon_core::now_ms());
        }
        info.connected = false;
        info.last_error = Some(error);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{new_registry, snapshot, upsert};
    use crate::mock::{MockLog, MockTransport};
    use crate::DeviceInfo;
    use std::time::Duration;

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
    async fn latest_wins_aborts_stale_plan() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("ml:0", "/dev/fake", DeviceProfile::Mlight3Led),
        );
        let log = MockLog::default();
        let factory_log = log.clone();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 50));

        let actor = spawn(ActorConfig {
            id: "ml:0".into(),
            port: "/dev/fake".into(),
            profile: DeviceProfile::Mlight3Led,
            factory: Arc::new(move || Ok(Box::new(MockTransport::new(factory_log.clone())))),
            desired: desired_rx,
            registry,
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
    async fn mock_actor_applies_effect_and_records_commands() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("mock:0", "mock", DeviceProfile::Mock),
        );

        let log = MockLog::default();
        let factory_log = log.clone();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 50));

        let actor = spawn(ActorConfig {
            id: "mock:0".into(),
            port: "mock".into(),
            profile: DeviceProfile::Mock,
            factory: Arc::new(move || Ok(Box::new(MockTransport::new(factory_log.clone())))),
            desired: desired_rx,
            registry: registry.clone(),
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        let devices = snapshot(&registry);
        assert_eq!(devices.len(), 1);
        assert!(devices[0].connected);
        assert_eq!(devices[0].effect, Effect::Busy);
        assert!(devices[0]
            .mock_commands
            .iter()
            .any(|command| command.starts_with("4D 4F 43 4B")));

        desired_tx.send(desired(Effect::Alert, 50)).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(snapshot(&registry)[0].effect, Effect::Alert);

        actor.abort();
    }
    #[tokio::test(start_paused = true)]
    async fn agentcore_sends_state_names_and_scaled_brightness() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("ac:0", "/dev/fake", DeviceProfile::AgentCore),
        );
        let log = MockLog::default();
        let factory_log = log.clone();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 50));

        let actor = spawn(ActorConfig {
            id: "ac:0".into(),
            port: "/dev/fake".into(),
            profile: DeviceProfile::AgentCore,
            factory: Arc::new(move || Ok(Box::new(MockTransport::new(factory_log.clone())))),
            desired: desired_rx,
            registry: registry.clone(),
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
        let log = MockLog::default();
        let factory_log = log.clone();
        let (desired_tx, desired_rx) = watch::channel(desired(Effect::Busy, 100));

        let actor = spawn(ActorConfig {
            id: "ml:0".into(),
            port: "/dev/fake".into(),
            profile: DeviceProfile::Mlight3Led,
            factory: Arc::new(move || Ok(Box::new(MockTransport::new(factory_log.clone())))),
            desired: desired_rx,
            registry,
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
