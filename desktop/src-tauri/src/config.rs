//! 读取共享配置里的服务端口；桌面壳只读，不写配置文件。

use agent_beacon_core::{Config, DEFAULT_PORT};

pub fn service_port() -> u16 {
    let Some(base) = dirs::config_dir() else {
        return DEFAULT_PORT;
    };
    let path = base.join("agent-beacon").join("config.json");
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
