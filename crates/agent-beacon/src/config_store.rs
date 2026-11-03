//! 配置文件读写：服务是唯一写入者，落盘使用临时文件 + rename。

use std::fs;
use std::path::{Path, PathBuf};

use agent_beacon_core::{Config, ConfigError};

pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> Result<PathBuf, String> {
        let base = dirs::config_dir().ok_or_else(|| "无法解析用户配置目录".to_string())?;
        Ok(base.join("agent-beacon").join("config.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读取配置；文件不存在时写入默认配置并返回。
    pub fn load_or_create(&self) -> Result<Config, String> {
        if !self.path.exists() {
            let config = Config::default();
            config.validate().map_err(config_error)?;
            self.save(&config)?;
            return Ok(config);
        }

        let text = fs::read_to_string(&self.path)
            .map_err(|error| format!("读取配置 {} 失败：{error}", self.path.display()))?;
        let config: Config = serde_json::from_str(&text).map_err(|error| {
            format!(
                "解析配置 {} 失败（可删除该文件恢复默认）：{error}",
                self.path.display()
            )
        })?;
        config
            .validate()
            .map_err(|error| format!("配置 {} 非法：{error}", self.path.display()))?;
        Ok(config)
    }

    pub fn save(&self, config: &Config) -> Result<(), String> {
        config.validate().map_err(config_error)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建配置目录 {} 失败：{error}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(config)
            .map_err(|error| format!("序列化配置失败：{error}"))?;
        let tmp = self
            .path
            .with_extension(format!("json.tmp-{}", std::process::id()));
        fs::write(&tmp, text.as_bytes())
            .map_err(|error| format!("写入临时配置 {} 失败：{error}", tmp.display()))?;
        fs::rename(&tmp, &self.path).map_err(|error| {
            let _ = fs::remove_file(&tmp);
            format!("替换配置 {} 失败：{error}", self.path.display())
        })
    }
}

fn config_error(error: ConfigError) -> String {
    format!("配置非法：{error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store(tag: &str) -> ConfigStore {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
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
