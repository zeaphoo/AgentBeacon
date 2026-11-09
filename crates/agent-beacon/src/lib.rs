//! 服务层库：配置、API、SSE、CLI 与设备编排。

pub mod api;
pub mod cli;
pub mod codex;
pub mod config_store;
pub mod dsh;
pub mod hookcmd;
pub mod integrations;
pub mod journal;
pub mod settings_hooks;
pub mod state;
pub mod worker;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
