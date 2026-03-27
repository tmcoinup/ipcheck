use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub api_token: String,
    pub token_apply_url: String,
    pub note: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_token: String::new(),
            token_apply_url: "在这里填写 Token 申请地址，例如你的平台控制台地址".to_string(),
            note: "配置文件说明：api_token 为可选，留空表示不携带 Authorization".to_string(),
        }
    }
}

pub fn load_or_init_config() -> Result<AppConfig, String> {
    let path = config_path()?;
    if !path.exists() {
        let default = AppConfig::default();
        save_config_to_path(&default, &path)?;
        return Ok(default);
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str::<AppConfig>(&content).map_err(|e| e.to_string())
}

fn save_config_to_path(config: &AppConfig, path: &PathBuf) -> Result<(), String> {
    let serialized = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, serialized).map_err(|e| e.to_string())
}

fn config_path() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    Ok(cwd.join("app_config.json"))
}
