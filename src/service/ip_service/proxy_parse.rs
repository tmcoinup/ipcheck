use url::Url;

use crate::domain::models::ProxySpec;

use super::error::ServiceError;

pub(super) fn parse_proxy_line_compatible(line: &str) -> Result<ProxySpec, ServiceError> {
    if line.starts_with("socks5://") && line.contains("---") {
        return parse_socks5_dash_style(line);
    }
    if line.contains('|') {
        return parse_pipe_style(line);
    }
    parse_standard_socks5_url(line)
}

fn parse_standard_socks5_url(line: &str) -> Result<ProxySpec, ServiceError> {
    let url = Url::parse(line).map_err(|_| ServiceError::InvalidProxy(line.to_string()))?;
    if url.scheme() != "socks5" {
        return Err(ServiceError::InvalidProxy(line.to_string()));
    }
    let username = url.username().to_string();
    let password = url
        .password()
        .ok_or_else(|| ServiceError::InvalidProxy(line.to_string()))?
        .to_string();
    let host = url
        .host_str()
        .ok_or_else(|| ServiceError::InvalidProxy(line.to_string()))?
        .to_string();
    let port = url
        .port()
        .ok_or_else(|| ServiceError::InvalidProxy(line.to_string()))?;

    Ok(ProxySpec {
        raw: line.to_string(),
        username,
        password,
        host,
        port,
    })
}

fn parse_socks5_dash_style(line: &str) -> Result<ProxySpec, ServiceError> {
    let body = line
        .strip_prefix("socks5://")
        .ok_or_else(|| ServiceError::InvalidProxy(line.to_string()))?;
    let parts: Vec<&str> = body.split("---").collect();
    if parts.len() < 3 {
        return Err(ServiceError::InvalidProxy(line.to_string()));
    }
    let host_port = parts[0].trim();
    let username = parts[1].trim();
    let password = parts[2].trim();

    let (host, port) = split_host_port(host_port)
        .ok_or_else(|| ServiceError::InvalidProxy(line.to_string()))?;
    Ok(ProxySpec {
        raw: format!("socks5://{}:{}@{}:{}", username, password, host, port),
        username: username.to_string(),
        password: password.to_string(),
        host: host.to_string(),
        port,
    })
}

fn parse_pipe_style(line: &str) -> Result<ProxySpec, ServiceError> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 4 {
        return Err(ServiceError::InvalidProxy(line.to_string()));
    }
    let host = parts[0].trim();
    let port = parts[1]
        .trim()
        .parse::<u16>()
        .map_err(|_| ServiceError::InvalidProxy(line.to_string()))?;
    let username = parts[2].trim();
    let password = parts[3].trim();

    if host.is_empty() || username.is_empty() || password.is_empty() {
        return Err(ServiceError::InvalidProxy(line.to_string()));
    }
    Ok(ProxySpec {
        raw: format!("socks5://{}:{}@{}:{}", username, password, host, port),
        username: username.to_string(),
        password: password.to_string(),
        host: host.to_string(),
        port,
    })
}

fn split_host_port(host_port: &str) -> Option<(&str, u16)> {
    let mut seg = host_port.rsplitn(2, ':');
    let port_str = seg.next()?;
    let host = seg.next()?;
    let port = port_str.parse::<u16>().ok()?;
    if host.trim().is_empty() {
        return None;
    }
    Some((host.trim(), port))
}
