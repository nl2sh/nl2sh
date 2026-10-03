use anyhow::{anyhow, Result};
use reqwest::{Response, StatusCode};
use std::time::Duration;

pub async fn checked(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let safe = if body.len() > 512 {
        &body[..512]
    } else {
        &body
    };
    Err(anyhow!("LLM HTTP {status}: {safe}"))
}
pub fn retryable_status(s: StatusCode, retry_method_not_allowed: bool) -> bool {
    s == StatusCode::TOO_MANY_REQUESTS
        || s.is_server_error()
        || (retry_method_not_allowed && s == StatusCode::METHOD_NOT_ALLOWED)
}
pub fn delay(base: u64, attempt: u32) -> Duration {
    Duration::from_millis(base.saturating_mul(1u64 << attempt.min(6)).min(30_000))
}
