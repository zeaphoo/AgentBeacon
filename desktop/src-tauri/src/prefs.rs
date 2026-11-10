//! 桌面壳偏好：开机自启动。

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub autostart: bool,
}

impl Prefs {
    /// 偏好与服务配置放在同一个数据目录；旧版本写在应用标识目录里，读到后迁移过来。
    pub fn load(app: &AppHandle) -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let prefs = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok());
        if prefs.is_some() {
            return prefs.unwrap_or_default();
        }

        let legacy = app
            .path()
            .app_config_dir()
            .ok()
            .map(|dir| dir.join("desktop-prefs.json"));
        let migrated = legacy
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Self>(&text).ok());
        match migrated {
            Some(prefs) => {
                let _ = prefs.save();
                prefs
            }
            None => Self::default(),
        }
    }

    fn path() -> Option<PathBuf> {
        Some(crate::config::data_dir()?.join("desktop-prefs.json"))
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = crate::config::data_dir().ok_or_else(|| "无法解析应用数据目录".to_string())?;
        fs::create_dir_all(&dir)
            .map_err(|error| format!("创建配置目录 {} 失败：{error}", dir.display()))?;
        let path = dir.join("desktop-prefs.json");
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("序列化偏好失败：{error}"))?;
        fs::write(&path, text).map_err(|error| format!("写入偏好 {} 失败：{error}", path.display()))
    }
}
