use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyEntry {
    pub id: i64,
    pub raw: String,
    pub username: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    pub created_at: Option<String>,
    pub last_real_ip: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySpec {
    pub raw: String,
    pub username: String,
    pub password: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaseData {
    pub ip: String,
    pub country: String,
    pub province: String,
    pub city: String,
    pub lng: f64,
    pub lat: f64,
    pub idc: String,
    pub scene: String,
    pub isp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OverallData {
    pub risk_score: String,
    pub update_day: String,
    pub behavior_risks: Vec<String>,
    pub device_risks: Vec<String>,
    pub malware_risks: Vec<String>,
    pub other_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CheckResult {
    pub proxy_id: i64,
    pub source_proxy: String,
    pub real_ip: String,
    pub base: BaseData,
    pub overall: OverallData,
    pub checked_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct AppStateSnapshot {
    pub token: String,
    pub proxies: Vec<ProxyEntry>,
    pub results: Vec<CheckResult>,
}
