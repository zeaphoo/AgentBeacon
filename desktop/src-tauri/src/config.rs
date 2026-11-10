//! 读取共享配置里的服务端口；桌面壳只读，不写配置文件。

use std::path::PathBuf;

use agent_beacon_core::{Config, DEFAULT_PORT};

/// 服务与桌面壳共用的应用数据根，与服务端 `ConfigStore::default_path()` 一致。
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|base| base.join(agent_beacon_core::APP_IDENTIFIER))
}

pub fn service_port() -> u16 {
    let Some(dir) = data_dir() else {
        return DEFAULT_PORT;
    };
    let path = dir.join("config.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return DEFAULT_PORT;
    };
    let Ok(config) = serde_json::from_str::<Config>(&text) else {
        return DEFAULT_PORT;
    };
    if config.validate().is_err() {
        return DEFAULT_PORT;
    }
    config.server.port
}
