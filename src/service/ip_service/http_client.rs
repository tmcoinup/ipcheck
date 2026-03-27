use reqwest::{Client, Proxy};
use tracing::error;

use crate::domain::models::ProxyEntry;

use super::error::ServiceError;

pub(super) fn build_direct_client(token: &str) -> Result<Client, ServiceError> {
    let mut headers = reqwest::header::HeaderMap::new();
    if !token.trim().is_empty() {
        let token_header = format!("Bearer {}", token.trim());
        let header_value = reqwest::header::HeaderValue::from_str(&token_header)
            .map_err(|e| ServiceError::Parse(format!("invalid token header: {e}")))?;
        headers.insert(reqwest::header::AUTHORIZATION, header_value);
    }
    Client::builder()
        .no_proxy()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| ServiceError::Network(format!("direct client build failed: {e}")))
}

pub(super) fn build_proxy_client(proxy: &ProxyEntry, token: &str) -> Result<Client, ServiceError> {
    let proxy_url = format!(
        "socks5h://{}:{}@{}:{}",
        proxy.username, proxy.password, proxy.host, proxy.port
    );
    tracing::info!(proxy_url = %proxy_url, "service build_proxy_client");
    let req_proxy =
        Proxy::all(&proxy_url).map_err(|e| ServiceError::Network(format!("proxy setup failed: {e}")))?;

    let mut headers = reqwest::header::HeaderMap::new();
    if !token.trim().is_empty() {
        let token_header = format!("Bearer {}", token.trim());
        let header_value = reqwest::header::HeaderValue::from_str(&token_header)
            .map_err(|e| ServiceError::Parse(format!("invalid token header: {e}")))?;
        headers.insert(reqwest::header::AUTHORIZATION, header_value);
    }

    Client::builder()
        .proxy(req_proxy)
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| {
            error!(proxy_id = proxy.id, error = %e, "service build_proxy_client failed");
            ServiceError::Network(format!("client build failed: {e}"))
        })
}
