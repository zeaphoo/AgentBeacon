//! AgentBeacon 的纯领域模型。
//!
//! 这里不碰串口、HTTP 或文件系统，所有时间都由调用方注入，方便用固定时钟测试。

pub mod config;
pub mod effect;
pub mod profile;
pub mod session;

pub use config::{
    Config, ConfigError, DeviceAssignment, DeviceBinding, MlightConfig, RoutingConfig,
    RoutingStrategy, ServerConfig, ShutdownConfig, Yh50Config,
};
pub use effect::{Effect, Mapping};
pub use profile::{DeviceCapabilities, DeviceProfile, DeviceProfileInfo};
pub use session::{
    aggregate_of, AgentState, Aggregate, Event, SessionRegistry, SessionView, ValidationError,
    SESSION_HISTORY_CAPACITY,
};

/// 应用标识：桌面 Bundle ID，也是平台数据目录下的应用数据根名。
///
/// 服务、CLI 与桌面壳都用它定位同一份配置与日志。
pub const APP_IDENTIFIER: &str = "net.quadhub.agent-beacon";

/// 服务默认端口。
pub const DEFAULT_PORT: u16 = 6980;

/// 默认会话存活时间（秒）。
pub const DEFAULT_TTL_SECS: u64 = 900;

/// 会话 TTL 允许范围（秒）。
pub const MIN_TTL_SECS: u64 = 30;
pub const MAX_TTL_SECS: u64 = 86_400;

/// 当前 Unix 毫秒时间戳。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}
