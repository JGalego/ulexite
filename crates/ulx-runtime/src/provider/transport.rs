//! Sync HTTP transport shared by every real provider adapter (§12.4): one
//! place for timeouts, one retry on 429/5xx, and status/error mapping to
//! `ProviderError`, so vendor adapters only deal with building/parsing
//! vendor-specific JSON bodies. `ScriptedTransport` (test-only) keeps
//! adapter unit tests in-process and network-free, matching the rest of
//! this crate's fully-offline test philosophy.

use std::time::Duration;

use super::ProviderError;

pub struct HttpResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

pub trait Transport: Send + Sync {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &serde_json::Value,
    ) -> Result<HttpResponse, ProviderError>;
}

pub struct UreqTransport {
    timeout: Duration,
}

impl UreqTransport {
    pub fn new(timeout: Duration) -> Self {
        UreqTransport { timeout }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        UreqTransport::new(Duration::from_secs(30))
    }
}

impl Transport for UreqTransport {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &serde_json::Value,
    ) -> Result<HttpResponse, ProviderError> {
        let mut req = ureq::post(url).timeout(self.timeout);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        match req.send_json(body.clone()) {
            Ok(resp) => {
                let status = resp.status();
                let body = resp
                    .into_json::<serde_json::Value>()
                    .map_err(|e| ProviderError::Failed(format!("invalid JSON response: {e}")))?;
                Ok(HttpResponse { status, body })
            }
            Err(ureq::Error::Status(status, resp)) => {
                let body = resp
                    .into_json::<serde_json::Value>()
                    .unwrap_or(serde_json::Value::Null);
                Ok(HttpResponse { status, body })
            }
            Err(ureq::Error::Transport(t)) => {
                let is_timeout = std::error::Error::source(&t)
                    .and_then(|s| s.downcast_ref::<std::io::Error>())
                    .is_some_and(|io_err| io_err.kind() == std::io::ErrorKind::TimedOut);
                if is_timeout {
                    Err(ProviderError::Timeout)
                } else {
                    Err(ProviderError::Failed(format!("transport error: {t}")))
                }
            }
        }
    }
}

/// One retry on HTTP 429/5xx with a short fixed backoff, then a final
/// status→`ProviderError` mapping. This is the "sizable work" the runtime's
/// module docs used to call out as scoped away — deliberately kept simple
/// (no exponential backoff/circuit breaker) rather than unbounded.
pub fn send_json_with_retry(
    transport: &dyn Transport,
    url: &str,
    headers: &[(String, String)],
    body: &serde_json::Value,
) -> Result<serde_json::Value, ProviderError> {
    const MAX_ATTEMPTS: u32 = 2;
    const BACKOFF: Duration = Duration::from_millis(250);

    let mut last = None;
    for attempt in 1..=MAX_ATTEMPTS {
        let resp = transport.post_json(url, headers, body)?;
        if resp.status < 400 {
            return Ok(resp.body);
        }
        let retryable = resp.status == 429 || resp.status >= 500;
        if !retryable || attempt == MAX_ATTEMPTS {
            return Err(status_to_error(resp.status, &resp.body));
        }
        last = Some(resp);
        std::thread::sleep(BACKOFF);
    }
    // Unreachable in practice (loop always returns above), but keeps the
    // function total without an `unwrap`.
    Err(last
        .map(|r| status_to_error(r.status, &r.body))
        .unwrap_or_else(|| ProviderError::Failed("no response".to_string())))
}

fn status_to_error(status: u16, body: &serde_json::Value) -> ProviderError {
    if status == 429 {
        return ProviderError::RateLimited;
    }
    let detail = body
        .get("error")
        .and_then(|e| e.get("message").or(Some(e)))
        .map(|v| v.to_string())
        .unwrap_or_else(|| body.to_string());
    ProviderError::Failed(format!("HTTP {status}: {detail}"))
}

#[cfg(test)]
pub struct ScriptedTransport {
    responses: std::sync::Mutex<std::collections::VecDeque<Result<HttpResponse, ProviderError>>>,
}

#[cfg(test)]
impl ScriptedTransport {
    pub fn new(responses: Vec<Result<HttpResponse, ProviderError>>) -> Self {
        ScriptedTransport {
            responses: std::sync::Mutex::new(responses.into_iter().collect()),
        }
    }

    pub fn ok(status: u16, body: serde_json::Value) -> Result<HttpResponse, ProviderError> {
        Ok(HttpResponse { status, body })
    }
}

#[cfg(test)]
impl Transport for ScriptedTransport {
    fn post_json(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        _body: &serde_json::Value,
    ) -> Result<HttpResponse, ProviderError> {
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Err(ProviderError::Failed(
                    "no scripted response left".to_string(),
                ))
            })
    }
}
