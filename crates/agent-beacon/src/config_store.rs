//! 配置文件读写：服务是唯一写入者，落盘使用临时文件 + rename。

use std::fs;
use std::path::{Path, PathBuf};

use agent_beacon_core::{Config, ConfigError, APP_IDENTIFIER};

pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// 服务配置落在应用自己的数据根：`<data_dir>/<APP_IDENTIFIER>/config.json`。
    ///
    /// 桌面壳用的是同一个目录，因此 CLI、服务与桌面应用共用一份配置。
    pub fn default_path() -> Result<PathBuf, String> {
        Ok(data_dir()?.join("config.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读取配置；文件不存在时写入默认配置并返回。
    pub fn load_or_create(&self) -> Result<Config, String> {
        if !self.path.exists() {
            migrate_legacy_config(&self.path);
            let config = Config::default();
            config.validate().map_err(config_error)?;
            self.save(&config)?;
            return Ok(config);
        }

        let text = fs::read_to_string(&self.path)
            .map_err(|error| format!("failed to read config {}: {error}", self.path.display()))?;
        let config: Config = serde_json::from_str(&text).map_err(|error| {
            format!(
                "failed to parse config {} (delete it to restore defaults): {error}",
                self.path.display()
            )
        })?;
        config
            .validate()
            .map_err(|error| format!("invalid config {}: {error}", self.path.display()))?;
        Ok(config)
    }

    pub fn save(&self, config: &Config) -> Result<(), String> {
        config.validate().map_err(config_error)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create config directory {}: {error}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(config)
            .map_err(|error| format!("failed to serialize config: {error}"))?;
        let tmp = self
            .path
            .with_extension(format!("json.tmp-{}", std::process::id()));
        fs::write(&tmp, text.as_bytes())
            .map_err(|error| format!("failed to write temporary config {}: {error}", tmp.display()))?;
        fs::rename(&tmp, &self.path).map_err(|error| {
            let _ = fs::remove_file(&tmp);
            format!("failed to replace config {}: {error}", self.path.display())
        })
    }
}

fn config_error(error: ConfigError) -> String {
    format!("invalid config: {error}")
}

/// 应用数据根：Linux `$XDG_DATA_HOME`（默认 `~/.local/share`）、
/// macOS `~/Library/Application Support`、Windows `%APPDATA%`。
pub fn data_dir() -> Result<PathBuf, String> {
    let base = dirs::data_dir()
        .ok_or_else(|| "cannot resolve user data directory".to_string())?;
    Ok(base.join(APP_IDENTIFIER))
}

/// 旧版本把配置放在 config 目录；升级后复制到新的应用数据根。
fn migrate_legacy_config(target: &Path) {
    let Some(base) = dirs::config_dir() else {
        return;
    };
    let legacy = base.join("agent-beacon").join("config.json");
    if !legacy.exists() {
        return;
    }
    let Some(parent) = target.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let _ = fs::copy(&legacy, target);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_store(tag: &str) -> ConfigStore {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            + u128::from(COUNTER.fetch_add(1, Ordering::Relaxed));
        let dir = std::env::temp_dir().join(format!("agent-beacon-test-{tag}-{unique}"));
        ConfigStore::new(dir.join("config.json"))
    }

    #[test]
    fn creates_default_config_on_first_load() {
        let store = temp_store("create");
        let config = store.load_or_create().unwrap();
        assert_eq!(config.server.port, agent_beacon_core::DEFAULT_PORT);
        assert!(store.path().exists());

        let again = store.load_or_create().unwrap();
        assert_eq!(config, again);
        let _ = fs::remove_dir_all(store.path().parent().unwrap());
    }

    #[test]
    fn default_path_lives_under_the_application_data_root() {
        let path = ConfigStore::default_path().unwrap();
        assert_eq!(path.file_name().unwrap(), "config.json");
        let dir = path.parent().unwrap();
        assert_eq!(dir.file_name().unwrap(), APP_IDENTIFIER);
        assert_eq!(dir.parent().unwrap(), dirs::data_dir().unwrap());
    }

    #[test]
    fn rejects_invalid_config_without_overwriting() {
        let store = temp_store("invalid");
        let good = Config::default();
        store.save(&good).unwrap();

        let mut bad = good.clone();
        bad.manual_override_default_secs = 0;
        assert!(store.save(&bad).is_err());

        let text = fs::read_to_string(store.path()).unwrap();
        let on_disk: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(on_disk, good);
        let _ = fs::remove_dir_all(store.path().parent().unwrap());
    }
}
