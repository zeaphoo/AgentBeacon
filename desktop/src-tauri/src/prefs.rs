//! 桌面壳偏好：开机自启动。

use std::fs;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub autostart: bool,
}

impl Prefs {
    pub fn load(app: &AppHandle) -> Self {
        let Ok(dir) = app.path().app_config_dir() else {
            return Self::default();
        };
        let path = dir.join("desktop-prefs.json");
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, app: &AppHandle) -> Result<(), String> {
        let dir = app
            .path()
            .app_config_dir()
            .map_err(|error| format!("无法解析应用配置目录：{error}"))?;
        fs::create_dir_all(&dir)
            .map_err(|error| format!("创建配置目录 {} 失败：{error}", dir.display()))?;
        let path = dir.join("desktop-prefs.json");
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("序列化偏好失败：{error}"))?;
        fs::write(&path, text).map_err(|error| format!("写入偏好 {} 失败：{error}", path.display()))
    }
}
