use futures::stream::{FuturesUnordered, StreamExt};
use reqwest::Client;
use serde::Deserialize;
use tracing::{info, warn};

use crate::domain::models::ProxyEntry;

use super::error::ServiceError;
use super::http_headers::{apply_ip_probe_json_headers, apply_ip_probe_text_headers};

const IP_PROBE_LOG_BODY_MAX: usize = 2048;

fn log_body_prefix(body: &str, max: usize) -> String {
    let t: String = body.chars().take(max).collect();
    if body.len() > max {
        format!("{t}...")
    } else {
        t
    }
}

#[derive(Debug, Deserialize)]
struct IpifyResponse {
    ip: String,
}

pub(super) async fn query_real_ip(client: &Client) -> Result<String, ServiceError> {
    info!("service query_real_ip start");
    let mut tasks = FuturesUnordered::new();
    tasks.push(query_provider(client, "https://api.ipify.org?format=json", true));
    tasks.push(query_provider(client, "https://api64.ipify.org?format=json", true));
    tasks.push(query_provider(client, "https://api.ip.sb/ip", false));
    tasks.push(query_provider(client, "https://ifconfig.me/ip", false));
    tasks.push(query_provider(client, "https://ipinfo.io/ip", false));
    tasks.push(query_provider(client, "https://ifconfig.co/ip", false));

    while let Some(result) = tasks.next().await {
        match result {
            Ok(ip) if !ip.trim().is_empty() => {
                info!(real_ip = %ip, "service query_real_ip provider returned first valid result");
                drop(tasks);
                return Ok(ip.trim().to_string());
            }
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    warn!("service query_real_ip all providers failed");
    Err(ServiceError::Network(
        "all real ip providers failed".to_string(),
    ))
}

/// 先探测出口公网 IP；全部失败或结果为空时，使用代理 `host` 作为伪 IP，供百度风控等接口继续查询。
pub(super) async fn query_real_ip_or_pseudo(client: &Client, proxy: &ProxyEntry) -> String {
    let fallback = proxy.host.trim().to_string();
    match query_real_ip(client).await {
        Ok(s) => {
            let t = s.trim();
            if t.is_empty() {
                warn!("query_real_ip returned empty; use pseudo IP (proxy host)");
                fallback
            } else {
                t.to_string()
            }
        }
        Err(e) => {
            warn!(
                error = %e,
                pseudo = %fallback,
                "egress IP probe failed; use pseudo IP (proxy host) for risk API"
            );
            fallback
        }
    }
}

async fn query_provider(client: &Client, url: &str, json_mode: bool) -> Result<String, ServiceError> {
    if json_mode {
        query_ip_json(client, url).await
    } else {
        query_ip_text(client, url).await
    }
}

async fn query_ip_json(client: &Client, url: &str) -> Result<String, ServiceError> {
    let resp = apply_ip_probe_json_headers(client.get(url))
        .send()
        .await
        .map_err(|e| ServiceError::Network(format!("ip query failed ({url}): {e}")))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| ServiceError::Network(format!("ip query read body ({url}): {e}")))?;
    info!(
        target: "ipcheck_http",
        url = %url,
        status = %status,
        body_len = text.len(),
        body = %log_body_prefix(&text, IP_PROBE_LOG_BODY_MAX),
        "出口 IP 探测 HTTP 响应"
    );
    let body: IpifyResponse = serde_json::from_str(&text).map_err(|e| {
        ServiceError::Parse(format!("ip json parse failed ({url}): {e}"))
    })?;
    Ok(body.ip)
}

async fn query_ip_text(client: &Client, url: &str) -> Result<String, ServiceError> {
    let resp = apply_ip_probe_text_headers(client.get(url))
        .send()
        .await
        .map_err(|e| ServiceError::Network(format!("ip query failed ({url}): {e}")))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| ServiceError::Parse(format!("ip text parse failed ({url}): {e}")))?;
    info!(
        target: "ipcheck_http",
        url = %url,
        status = %status,
        body_len = text.len(),
        body = %log_body_prefix(&text, IP_PROBE_LOG_BODY_MAX),
        "出口 IP 探测 HTTP 响应"
    );
    Ok(text.trim().to_string())
}
