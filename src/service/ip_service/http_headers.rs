use reqwest::header::{self, HeaderValue};

use super::error::ServiceError;

/// 桌面 Chrome 常见 UA，降低出口 IP / 风控接口将请求识别为脚本客户端的概率。
const BROWSER_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 ",
    "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
);

const ACCEPT_LANGUAGE: &str = "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7";

/// 与浏览器访问 ipify / ifconfig 类站点时接近的请求头。
pub(super) fn apply_ip_probe_json_headers(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    req.header(header::USER_AGENT, BROWSER_USER_AGENT)
        .header(header::ACCEPT, "application/json, text/plain, */*")
        .header(header::ACCEPT_LANGUAGE, ACCEPT_LANGUAGE)
}

/// 纯文本出口 IP 接口（部分站点返回 text/plain 或 HTML）。
pub(super) fn apply_ip_probe_text_headers(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    req.header(header::USER_AGENT, BROWSER_USER_AGENT)
        .header(
            header::ACCEPT,
            "text/plain, text/html, application/xhtml+xml, */*",
        )
        .header(header::ACCEPT_LANGUAGE, ACCEPT_LANGUAGE)
}

/// 与页面内 XHR 访问 `cloud.baidu.com` 风控 API 时接近的请求头（Referer 与页面 URL 一致）。
pub(super) fn apply_baidu_risk_api_headers(
    req: reqwest::RequestBuilder,
    referer: &str,
) -> Result<reqwest::RequestBuilder, ServiceError> {
    let referer_val = HeaderValue::from_str(referer).map_err(|e| {
        ServiceError::Parse(format!("风控 Referer 无法构造为合法 HTTP 头: {e}"))
    })?;
    let sec_ch_ua = HeaderValue::from_static(
        r#""Google Chrome";v="131", "Chromium";v="131", "Not_A Brand";v="24""#,
    );
    Ok(req
        .header(header::USER_AGENT, BROWSER_USER_AGENT)
        .header(header::ACCEPT, "application/json, text/plain, */*")
        .header(header::ACCEPT_LANGUAGE, ACCEPT_LANGUAGE)
        .header(header::REFERER, referer_val)
        .header(header::ORIGIN, "https://cloud.baidu.com")
        .header("Sec-Fetch-Dest", "empty")
        .header("Sec-Fetch-Mode", "cors")
        .header("Sec-Fetch-Site", "same-origin")
        .header("sec-ch-ua", sec_ch_ua)
        .header("sec-ch-ua-mobile", "?0")
        .header("sec-ch-ua-platform", "\"Windows\""))
}
