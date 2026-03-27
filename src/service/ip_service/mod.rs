//! IP 检测与代理导入：编排仓储、HTTP 客户端与外部 API。

mod baidu_api;
mod error;
mod http_client;
mod http_headers;
mod ip_probe;
mod proxy_parse;

use anyhow::Context;
use chrono::Utc;
use tracing::{info, warn};

pub use error::ServiceError;

/// 批量风控：顺序执行；单条遇百度 `ret_data` 限速时**跳过该条并继续**（不同代理可走不同出口，不应全局停批）。
#[derive(Debug, Clone)]
pub struct CheckProxyBatchOutcome {
    pub results: Vec<CheckResult>,
    /// 本会话内因限速跳过的代理条数（未写入结果）。
    pub skipped_rate_limit: u32,
}

use crate::domain::models::{CheckResult, ProxyEntry, ProxySpec};
use crate::repository::sqlite_repo::AppRepository;

use baidu_api::{query_base, query_overall};
use http_client::{build_direct_client, build_proxy_client};
use ip_probe::query_real_ip_or_pseudo;
use proxy_parse::parse_proxy_line_compatible;

#[derive(Clone)]
pub struct IpCheckService<R: AppRepository + Clone> {
    repo: R,
}

impl<R: AppRepository + Clone + 'static> IpCheckService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub fn parse_import_text(&self, text: &str) -> Result<Vec<ProxySpec>, ServiceError> {
        info!("service parse_import_text start");
        let mut parsed = Vec::new();
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            if line.starts_with('#') || line.starts_with('•') {
                continue;
            }
            if let Ok(spec) = parse_proxy_line_compatible(line) {
                parsed.push(spec);
            }
        }
        if parsed.is_empty() {
            return Err(ServiceError::InvalidProxy(
                "未解析到有效代理行（可与说明混贴；仅识别 socks5 / 管道 / --- 格式）".to_string(),
            ));
        }
        info!(count = parsed.len(), "service parse_import_text done");
        Ok(parsed)
    }

    pub fn save_imported_proxies(&self, proxies: &[ProxySpec]) -> Result<(), ServiceError> {
        info!(count = proxies.len(), "service save_imported_proxies");
        self.repo
            .insert_proxies(proxies)
            .map_err(|e| ServiceError::Repo(e.to_string()))
    }

    pub fn save_token(&self, token: &str) -> Result<(), ServiceError> {
        info!(token_len = token.len(), "service save_token");
        self.repo
            .save_token(token)
            .map_err(|e| ServiceError::Repo(e.to_string()))
    }

    pub fn load_snapshot(&self) -> Result<crate::domain::models::AppStateSnapshot, ServiceError> {
        info!("service load_snapshot");
        self.repo
            .load_snapshot()
            .map_err(|e| ServiceError::Repo(e.to_string()))
    }

    pub async fn resolve_real_ip(
        &self,
        proxy: ProxyEntry,
        token: String,
    ) -> Result<(i64, String, String), ServiceError> {
        info!(proxy_id = proxy.id, host = %proxy.host, port = proxy.port, "service resolve_real_ip start");
        let client = build_proxy_client(&proxy, token.as_str())?;
        let real_ip = query_real_ip_or_pseudo(&client, &proxy).await;
        let now = now_string();
        self.repo
            .update_real_ip(proxy.id, &real_ip, &now)
            .map_err(|e| ServiceError::Repo(e.to_string()))?;
        info!(proxy_id = proxy.id, real_ip = %real_ip, "service resolve_real_ip success");
        Ok((proxy.id, real_ip, now))
    }

    pub async fn check_proxy_ip(
        &self,
        proxy: ProxyEntry,
        token: String,
    ) -> Result<CheckResult, ServiceError> {
        info!(proxy_id = proxy.id, host = %proxy.host, port = proxy.port, "service check_proxy_ip start");
        match self.check_proxy_ip_inner(&proxy, token.as_str(), false).await {
            Ok(r) => Ok(r),
            Err(e) => {
                if e.is_rate_limited() {
                    return Err(e);
                }
                warn!(proxy_id = proxy.id, error = %e, "check_proxy_ip failed, retry without proxy");
                self.check_proxy_ip_inner(&proxy, token.as_str(), true).await
            }
        }
    }

    /// `force_direct`: 走直连访问百度接口（不走 SOCKS）；**必须使用本记录已保存的真实出口 IP**，
    /// 禁止对直连客户端做 `query_real_ip`（否则会拿到本机公网 IP，风控会查错对象）。
    async fn check_proxy_ip_inner(
        &self,
        proxy: &ProxyEntry,
        token: &str,
        force_direct: bool,
    ) -> Result<CheckResult, ServiceError> {
        let client_for_baidu = if force_direct {
            build_direct_client(token)?
        } else {
            build_proxy_client(proxy, token)?
        };

        let real_ip = if force_direct {
            match record_real_ip_for_direct_retry(proxy) {
                Ok(ip) => ip,
                Err(_) => {
                    let pc = build_proxy_client(proxy, token)?;
                    let ip = query_real_ip_or_pseudo(&pc, proxy).await;
                    let now = now_string();
                    self.repo
                        .update_real_ip(proxy.id, &ip, &now)
                        .map_err(|e| ServiceError::Repo(e.to_string()))?;
                    ip
                }
            }
        } else if should_skip_real_ip_query(proxy) {
            proxy
                .last_real_ip
                .as_ref()
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        } else {
            let ip = query_real_ip_or_pseudo(&client_for_baidu, proxy).await;
            let now = now_string();
            self.repo
                .update_real_ip(proxy.id, &ip, &now)
                .map_err(|e| ServiceError::Repo(e.to_string()))?;
            ip
        };

        let base = query_base(&client_for_baidu, &real_ip).await?;
        let overall = query_overall(&client_for_baidu, &real_ip).await?;
        let checked_at = now_string();

        self.repo
            .update_real_ip(proxy.id, &real_ip, &checked_at)
            .map_err(|e| ServiceError::Repo(e.to_string()))?;

        let result = CheckResult {
            proxy_id: proxy.id,
            source_proxy: proxy.raw.clone(),
            real_ip,
            base,
            overall,
            checked_at,
        };

        info!(proxy_id = proxy.id, real_ip = %result.real_ip, "service check_proxy_ip_inner success");
        Ok(result)
    }

    pub fn clear_real_ip_row(&self, proxy_id: i64) -> Result<(), ServiceError> {
        self.repo
            .clear_real_ip(proxy_id)
            .map_err(|e| ServiceError::Repo(e.to_string()))
    }

    pub fn clear_all_real_ip_rows(&self) -> Result<(), ServiceError> {
        self.repo
            .clear_all_real_ips()
            .map_err(|e| ServiceError::Repo(e.to_string()))
    }

    pub fn delete_results_for_proxy(&self, proxy_id: i64) -> Result<(), ServiceError> {
        self.repo
            .delete_results_for_proxy(proxy_id)
            .map_err(|e| ServiceError::Repo(e.to_string()))
    }

    pub async fn check_proxy_spec(
        &self,
        spec: ProxySpec,
        token: String,
    ) -> Result<CheckResult, ServiceError> {
        let proxy = ProxyEntry {
            id: 0,
            raw: spec.raw.clone(),
            username: spec.username,
            password: spec.password,
            host: spec.host,
            port: spec.port,
            created_at: None,
            last_real_ip: None,
            updated_at: None,
        };
        let client = build_proxy_client(&proxy, token.as_str())?;
        let real_ip = query_real_ip_or_pseudo(&client, &proxy).await;
        let base = query_base(&client, &real_ip).await?;
        let overall = query_overall(&client, &real_ip).await?;
        let checked_at = now_string();
        Ok(CheckResult {
            proxy_id: 0,
            source_proxy: spec.raw,
            real_ip,
            base,
            overall,
            checked_at,
        })
    }

    pub fn save_result(&self, result: &CheckResult) -> Result<(), ServiceError> {
        info!(proxy_id = result.proxy_id, real_ip = %result.real_ip, "service save_result");
        self.repo
            .insert_result(result)
            .map_err(|e| ServiceError::Repo(e.to_string()))
    }

    pub fn clear_proxy_list(&self) -> Result<(), ServiceError> {
        info!("service clear_proxy_list");
        self.repo
            .clear_proxies()
            .map_err(|e| ServiceError::Repo(e.to_string()))
    }

    pub fn delete_proxy(&self, proxy_id: i64) -> Result<(), ServiceError> {
        info!(proxy_id, "service delete_proxy");
        self.repo
            .delete_proxy(proxy_id)
            .map_err(|e| ServiceError::Repo(e.to_string()))
    }

    /// 顺序风控检测；单条 `RateLimited` 仅跳过该代理并继续（不同 SOCKS 出口可能不受同一次限速影响）。
    pub async fn check_proxies_sequential(
        &self,
        proxies: Vec<ProxyEntry>,
        token: String,
    ) -> Result<CheckProxyBatchOutcome, ServiceError> {
        info!(count = proxies.len(), "service check_proxies_sequential start");
        let mut results = Vec::new();
        let mut skipped_rate_limit = 0u32;
        for proxy in proxies {
            let pid = proxy.id;
            match self.check_proxy_ip(proxy, token.clone()).await {
                Ok(r) => {
                    self.save_result(&r)?;
                    results.push(r);
                }
                Err(e) if e.is_rate_limited() => {
                    skipped_rate_limit += 1;
                    warn!(
                        proxy_id = pid,
                        error = %e,
                        "check_proxies_sequential skip one proxy due to rate limit, continue"
                    );
                }
                Err(e) => return Err(e),
            }
        }
        info!(
            ok = results.len(),
            skipped = skipped_rate_limit,
            "service check_proxies_sequential done"
        );
        Ok(CheckProxyBatchOutcome {
            results,
            skipped_rate_limit,
        })
    }
}

fn now_string() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 直连重试时：使用库中已保存的 IP（含出口真 IP 或伪 IP=代理 host）。不在此直连客户端上做 `query_real_ip`（否则会拿到本机公网 IP）。
fn record_real_ip_for_direct_retry(proxy: &ProxyEntry) -> Result<String, ServiceError> {
    let Some(ref raw) = proxy.last_real_ip else {
        return Err(ServiceError::Network(
            "直连重试需要已保存的 IP，请先经代理完成一次风控或查询IP".to_string(),
        ));
    };
    let ip = raw.trim();
    if ip.is_empty() {
        return Err(ServiceError::Network(
            "无有效 IP 记录".to_string(),
        ));
    }
    Ok(ip.to_string())
}

/// 若已有「真实出口 IP」（且不是查询失败时的回退 IP=代理 host），风控查询可跳过出口 IP 探测。
fn should_skip_real_ip_query(proxy: &ProxyEntry) -> bool {
    let Some(ref ip) = proxy.last_real_ip else {
        return false;
    };
    let ip = ip.trim();
    if ip.is_empty() {
        return false;
    }
    if ip == proxy.host.trim() {
        return false;
    }
    true
}

pub fn resolve_db_path() -> anyhow::Result<std::path::PathBuf> {
    let current = std::env::current_dir().context("failed to get current directory")?;
    let data_dir = current.join("data");
    std::fs::create_dir_all(&data_dir).context("failed to create data directory")?;
    Ok(data_dir.join("ipcheck.db"))
}
