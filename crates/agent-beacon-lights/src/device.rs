//! 设备注册表：设备 actor 写入，API 读取。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::DeviceInfo;

pub type DeviceRegistry = Arc<Mutex<HashMap<String, DeviceInfo>>>;

pub fn new_registry() -> DeviceRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn upsert(registry: &DeviceRegistry, info: DeviceInfo) {
    if let Ok(mut devices) = registry.lock() {
        devices.insert(info.id.clone(), info);
    }
}

pub fn update<F>(registry: &DeviceRegistry, id: &str, apply: F)
where
    F: FnOnce(&mut DeviceInfo),
{
    if let Ok(mut devices) = registry.lock() {
        if let Some(info) = devices.get_mut(id) {
            apply(info);
        }
    }
}

/// 标记链路已连上：清掉掉线时间，刷新最后联系时间。
pub fn mark_online(registry: &DeviceRegistry, id: &str, now_ms: u64) {
    update(registry, id, |info| {
        info.connected = true;
        info.offline_since_ms = None;
        info.last_seen_ms = Some(now_ms);
        info.last_error = None;
    });
}

/// 标记链路掉线：只在第一次掉线时记录时间，重试期间不要不断刷新保留期。
pub fn mark_offline(registry: &DeviceRegistry, id: &str, now_ms: u64, error: Option<String>) {
    update(registry, id, |info| {
        info.connected = false;
        info.offline_since_ms.get_or_insert(now_ms);
        if error.is_some() {
            info.last_error = error;
        }
    });
}

/// 设备已经离线多久（从未连上过则从登记时间算起）。
pub fn offline_for_ms(info: &DeviceInfo, now_ms: u64) -> Option<u64> {
    if info.connected {
        return None;
    }
    info.offline_since_ms
        .map(|since| now_ms.saturating_sub(since))
}

pub fn remove(registry: &DeviceRegistry, id: &str) {
    if let Ok(mut devices) = registry.lock() {
        devices.remove(id);
    }
}

pub fn snapshot(registry: &DeviceRegistry) -> Vec<DeviceInfo> {
    let mut devices: Vec<DeviceInfo> = registry
        .lock()
        .map(|devices| devices.values().cloned().collect())
        .unwrap_or_default();
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    devices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceInfo;
    use agent_beacon_core::{DeviceProfile, Effect};

    #[test]
    fn offline_timer_starts_on_registration_and_stops_on_connect() {
        let registry = new_registry();
        let mut info = DeviceInfo::new("ble:aa", "ble:aa", DeviceProfile::AgentCore);
        info.offline_since_ms = Some(1_000);
        upsert(&registry, info);

        let stored = snapshot(&registry).remove(0);
        assert_eq!(offline_for_ms(&stored, 61_000), Some(60_000));
        assert_eq!(stored.effect, Effect::Off);

        mark_online(&registry, "ble:aa", 61_000);
        let stored = snapshot(&registry).remove(0);
        assert!(stored.connected);
        assert_eq!(stored.offline_since_ms, None);
        assert_eq!(stored.last_seen_ms, Some(61_000));
        assert_eq!(offline_for_ms(&stored, 90_000), None);
    }

    #[test]
    fn repeated_failures_keep_the_first_offline_timestamp() {
        let registry = new_registry();
        upsert(
            &registry,
            DeviceInfo::new("ble:aa", "ble:aa", DeviceProfile::AgentCore),
        );
        mark_offline(&registry, "ble:aa", 5_000, Some("timeout".into()));
        mark_offline(&registry, "ble:aa", 50_000, Some("timeout again".into()));

        let stored = snapshot(&registry).remove(0);
        assert_eq!(stored.offline_since_ms, Some(5_000));
        assert_eq!(stored.last_error.as_deref(), Some("timeout again"));
        assert_eq!(offline_for_ms(&stored, 185_000), Some(180_000));
    }
}
