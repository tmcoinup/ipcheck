use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("invalid socks5 line: {0}")]
    InvalidProxy(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("repository error: {0}")]
    Repo(String),
    /// 百度风控 `ret_data.code` 等为限速（如 601「查询次数过多」），勿重试、勿标绿。
    #[error("风控接口限速: {0}")]
    RateLimited(String),
}

impl ServiceError {
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimited(_))
    }
}
