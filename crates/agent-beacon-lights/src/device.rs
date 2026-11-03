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

pub fn snapshot(registry: &DeviceRegistry) -> Vec<DeviceInfo> {
    let mut devices: Vec<DeviceInfo> = registry
        .lock()
        .map(|devices| devices.values().cloned().collect())
        .unwrap_or_default();
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    devices
}
