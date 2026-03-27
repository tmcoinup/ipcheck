use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use tracing::info;

use crate::domain::models::{BaseData, OverallData};

use super::error::ServiceError;
use super::http_headers::apply_baidu_risk_api_headers;

fn body_prefix(body: &str, max: usize) -> String {
    let t: String = body.chars().take(max).collect();
    if body.len() > max {
        format!("{t}...")
    } else {
        t
    }
}

fn json_value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        _ => v.to_string(),
    }
}

/// 百度接口部分字段可能为数字或字符串；解析失败时按 0 处理。
fn de_f64_loose<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let v = serde_json::Value::deserialize(deserializer)?;
    match v {
        serde_json::Value::Null => Ok(0.0),
        serde_json::Value::Number(n) => Ok(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => s
            .parse::<f64>()
            .map_err(|e| Error::custom(format!("lng/lat string parse: {e}"))),
        _ => Ok(0.0),
    }
}

/// 根级 `ret_code==0` 时，`ret_data` 内仍可能带业务码 601 等表示限速。
fn check_ret_data_rate_limit(v: &serde_json::Value) -> Result<(), ServiceError> {
    for key in ["ret_data", "retData"] {
        let Some(rd) = v.get(key) else {
            continue;
        };
        if !rd.is_object() {
            continue;
        }
        let code = rd.get("code").and_then(|c| {
            c.as_i64()
                .or_else(|| c.as_u64().map(|u| u as i64))
                .or_else(|| c.as_str().and_then(|s| s.parse().ok()))
        });
        if let Some(c) = code {
            if c == 601 || c == 429 {
                let msg = rd
                    .get("message")
                    .or_else(|| rd.get("msg"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("查询次数过多");
                let limit = rd
                    .get("limit")
                    .map(|l| format!("（每日上限约 {} 次）", json_value_to_string(l)))
                    .unwrap_or_default();
                return Err(ServiceError::RateLimited(format!("{msg}{limit}")));
            }
        }
        if let Some(msg) = rd
            .get("message")
            .or_else(|| rd.get("msg"))
            .and_then(|m| m.as_str())
        {
            if msg.contains("查询次数") || msg.contains("过多") || msg.contains("限速") {
                return Err(ServiceError::RateLimited(msg.to_string()));
            }
        }
    }
    Ok(())
}

fn check_baidu_business_error(v: &serde_json::Value) -> Result<(), ServiceError> {
    let code = v
        .get("ret_code")
        .or_else(|| v.get("retCode"))
        .and_then(|c| {
            c.as_i64()
                .or_else(|| c.as_u64().map(|u| u as i64))
                .or_else(|| c.as_str().and_then(|s| s.parse::<i64>().ok()))
        });
    let Some(c) = code else {
        return Ok(());
    };
    if c == 0 {
        return Ok(());
    }
    let msg = v
        .get("ret_msg")
        .or_else(|| v.get("retMsg"))
        .or_else(|| v.get("msg"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    Err(ServiceError::Parse(format!(
        "百度接口业务错误 ret_code={c} {msg}"
    )))
}

/// 响应体写入日志的最大字符数（避免控制台被撑爆）。
const HTTP_LOG_BODY_MAX: usize = 12_288;

async fn read_baidu_json(
    resp: reqwest::Response,
    api_name: &str,
    request_url: &str,
) -> Result<serde_json::Value, ServiceError> {
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| ServiceError::Network(format!("{api_name} 读取响应体失败: {e}")))?;

    info!(
        target: "ipcheck_http",
        url = %request_url,
        api = %api_name,
        status = %status,
        body_len = body.len(),
        body = %body_prefix(&body, HTTP_LOG_BODY_MAX),
        "百度风控 HTTP 响应"
    );

    if !status.is_success() {
        return Err(ServiceError::Network(format!(
            "{api_name} HTTP {status}: {}",
            body_prefix(&body, 500)
        )));
    }

    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        ServiceError::Parse(format!(
            "{api_name} 非 JSON 或格式异常: {e}; 前缀={}",
            body_prefix(&body, 400)
        ))
    })?;

    check_baidu_business_error(&v)?;
    check_ret_data_rate_limit(&v)?;
    Ok(v)
}

#[derive(Debug, Deserialize)]
struct BaiduBaseResp {
    #[serde(alias = "retData")]
    ret_data: BaiduBaseRetPayload,
}

/// 百度 `ipage/base` 的 `ret_data` 有时为 `{ "data": { ... } }`，有时字段直接在 `ret_data` 下（无 `data` 包裹）。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BaiduBaseRetPayload {
    Nested {
        #[serde(alias = "baseData")]
        data: BaiduBaseData,
    },
    Flat(BaiduBaseData),
}

#[derive(Debug, Deserialize)]
struct BaiduBaseData {
    #[serde(default)]
    ip: String,
    #[serde(default)]
    country: String,
    #[serde(default)]
    province: String,
    #[serde(default)]
    city: String,
    #[serde(default, deserialize_with = "de_f64_loose")]
    lng: f64,
    #[serde(default, deserialize_with = "de_f64_loose")]
    lat: f64,
    #[serde(default)]
    idc: String,
    #[serde(default)]
    scene: String,
    #[serde(default)]
    isp: String,
}

/// `ipage/overall` 的 `ret_data` 形态较多：可能有 `data` / `overallData` 包裹，或 `overall` 与 `security_risks` 并列；不用 untagged 结构体，改为按键取值。
fn parse_overall_from_root(v: &serde_json::Value) -> Result<OverallData, ServiceError> {
    let rd = v
        .get("ret_data")
        .or_else(|| v.get("retData"))
        .ok_or_else(|| ServiceError::Parse("overall: 缺少 ret_data".to_string()))?;

    if !rd.is_object() {
        return Err(ServiceError::Parse(format!(
            "overall: ret_data 非对象: {}",
            body_prefix(&rd.to_string(), 240)
        )));
    }

    let payload = rd
        .get("data")
        .filter(|x| x.is_object())
        .or_else(|| {
            rd.get("overallData")
                .filter(|x| x.is_object())
        })
        .or_else(|| rd.get("result").filter(|x| x.is_object()))
        .unwrap_or(rd);

    let risk_score = extract_overall_risk_score(payload);
    let update_day = extract_overall_update_day(payload);
    let security_risks = payload
        .get("security_risks")
        .or_else(|| payload.get("securityRisks"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    Ok(OverallData {
        risk_score,
        update_day,
        behavior_risks: parse_risks(&security_risks, "行为风险"),
        device_risks: parse_risks(&security_risks, "关联设备风险"),
        malware_risks: parse_risks(&security_risks, "恶意事件风险"),
        other_tags: parse_risks(&security_risks, "其他标签"),
    })
}

fn extract_overall_risk_score(p: &serde_json::Value) -> String {
    if let Some(ov) = p.get("overall") {
        if let Some(v) = ov
            .get("risk_score_new")
            .or_else(|| ov.get("riskScoreNew"))
        {
            return json_value_to_string(v);
        }
        if let Some(s) = ov.as_str() {
            return s.to_string();
        }
    }
    if let Some(v) = p
        .get("risk_score_new")
        .or_else(|| p.get("riskScoreNew"))
    {
        return json_value_to_string(v);
    }
    String::new()
}

fn extract_overall_update_day(p: &serde_json::Value) -> String {
    p.get("update_day")
        .or_else(|| p.get("updateDay"))
        .map(json_value_to_string)
        .unwrap_or_default()
}

pub(super) async fn query_base(client: &Client, ip: &str) -> Result<BaseData, ServiceError> {
    let t = Utc::now().timestamp_millis();
    let referer = format!(
        "https://cloud.baidu.com/product-s/afd_s/ip-threat.html?s={ip}&t={t}"
    );
    let url = format!("https://cloud.baidu.com/api/afd-ip-threat/act/v1/ipage/base/{ip}");

    let resp = apply_baidu_risk_api_headers(client.get(url.as_str()), referer.as_str())?
        .send()
        .await
        .map_err(|e| ServiceError::Network(format!("base request failed: {e}")))?;

    let v = read_baidu_json(resp, "base", url.as_str()).await?;
    let data: BaiduBaseResp = serde_json::from_value(v).map_err(|e| {
        ServiceError::Parse(format!(
            "base 字段结构与预期不符: {e}（若百度改版接口，需同步调整 baidu_api.rs）"
        ))
    })?;

    let base = match data.ret_data {
        BaiduBaseRetPayload::Nested { data } => data,
        BaiduBaseRetPayload::Flat(d) => d,
    };

    Ok(BaseData {
        ip: base.ip,
        country: base.country,
        province: base.province,
        city: base.city,
        lng: base.lng,
        lat: base.lat,
        idc: base.idc,
        scene: base.scene,
        isp: base.isp,
    })
}

pub(super) async fn query_overall(client: &Client, ip: &str) -> Result<OverallData, ServiceError> {
    let t = Utc::now().timestamp_millis();
    let referer = format!(
        "https://cloud.baidu.com/product-s/afd_s/ip-threat.html?s={ip}&t={t}"
    );
    let url = format!("https://cloud.baidu.com/api/afd-ip-threat/act/v1/ipage/overall/{ip}");

    let resp = apply_baidu_risk_api_headers(client.get(url.as_str()), referer.as_str())?
        .send()
        .await
        .map_err(|e| ServiceError::Network(format!("overall request failed: {e}")))?;

    let v = read_baidu_json(resp, "overall", url.as_str()).await?;
    parse_overall_from_root(&v)
}

fn parse_risks(root: &serde_json::Value, key: &str) -> Vec<String> {
    let mut labels = Vec::new();
    let Some(items) = root.get(key).and_then(|v| v.as_array()) else {
        return labels;
    };

    for item in items {
        if let Some(sub_items) = item
            .get("subItems")
            .or_else(|| item.get("sub_items"))
            .and_then(|s| s.as_array())
        {
            for sub in sub_items {
                let name = match sub.get("name").and_then(|v| v.as_str()) {
                    Some(v) => v,
                    None => "",
                };
                let level = match sub
                    .get("risk_level")
                    .or_else(|| sub.get("riskLevel"))
                    .and_then(|v| v.as_str())
                {
                    Some(v) => v,
                    None => "",
                };
                if !name.is_empty() {
                    labels.push(format!("{name}({level})"));
                }
            }
        } else if let Some(label) = item.get("label").and_then(|v| v.as_str()) {
            labels.push(label.to_string());
        }
    }
    labels
}
