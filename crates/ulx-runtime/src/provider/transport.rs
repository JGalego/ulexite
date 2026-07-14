//! Sync HTTP transport shared by every real provider adapter (§12.4): one
//! place for timeouts, exponential backoff with jitter (honoring a
//! vendor's `Retry-After` on 429), a per-provider circuit breaker, and
//! status/error mapping to `ProviderError` — vendor adapters only build
//! and parse their own vendor-specific request/response shapes.
//! `ScriptedTransport` (test-only) keeps adapter unit tests in-process and
//! network-free, matching the rest of this crate's fully-offline test
//! philosophy.

use std::io::Read;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::ProviderError;

/// A request body: either a plain JSON document, or a `multipart/form-data`
/// upload (used by `transcribe`'s audio-file upload). Adapters never touch
/// the wire encoding directly — that's `UreqTransport`'s job.
#[derive(Clone)]
pub enum RequestBody {
    Json(serde_json::Value),
    Multipart {
        /// Plain text fields, e.g. `("model", "whisper-1")`.
        fields: Vec<(String, String)>,
        /// `(field name, filename, raw bytes)` for the uploaded file.
        file: (String, String, Vec<u8>),
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResponseKind {
    Json,
    /// Raw bytes on success (e.g. `speak`'s synthesized audio); a non-2xx
    /// response is still read as JSON, since vendors report errors as JSON
    /// even on binary-response endpoints.
    Bytes,
}

pub enum BodyOrBytes {
    Json(serde_json::Value),
    Bytes(Vec<u8>),
}

pub struct HttpResponse {
    pub status: u16,
    pub body: BodyOrBytes,
    /// Seconds from a `Retry-After` header, when the vendor sent one.
    pub retry_after: Option<u64>,
}

pub trait Transport: Send + Sync {
    fn send(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: RequestBody,
        accept: ResponseKind,
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

const MULTIPART_BOUNDARY: &str = "ulexite-boundary-7d1f2a9c";

fn build_multipart_body(fields: &[(String, String)], file: &(String, String, Vec<u8>)) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    let (field_name, filename, bytes) = file;
    body.extend_from_slice(
        format!(
            "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field_name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}--\r\n").as_bytes());
    body
}

impl Transport for UreqTransport {
    fn send(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: RequestBody,
        accept: ResponseKind,
    ) -> Result<HttpResponse, ProviderError> {
        let mut req = ureq::post(url).timeout(self.timeout);
        for (k, v) in headers {
            req = req.set(k, v);
        }

        let sent = match &body {
            RequestBody::Json(json) => req.send_json(json.clone()),
            RequestBody::Multipart { fields, file } => {
                let payload = build_multipart_body(fields, file);
                req.set(
                    "Content-Type",
                    &format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
                )
                .send_bytes(&payload)
            }
        };

        match sent {
            Ok(resp) => {
                let status = resp.status();
                let retry_after = parse_retry_after(&resp);
                let body = match accept {
                    ResponseKind::Json => {
                        BodyOrBytes::Json(resp.into_json::<serde_json::Value>().map_err(|e| {
                            ProviderError::Failed(format!("invalid JSON response: {e}"))
                        })?)
                    }
                    ResponseKind::Bytes => {
                        let mut buf = Vec::new();
                        resp.into_reader().read_to_end(&mut buf).map_err(|e| {
                            ProviderError::Failed(format!("could not read response body: {e}"))
                        })?;
                        BodyOrBytes::Bytes(buf)
                    }
                };
                Ok(HttpResponse {
                    status,
                    body,
                    retry_after,
                })
            }
            Err(ureq::Error::Status(status, resp)) => {
                let retry_after = parse_retry_after(&resp);
                let body = resp
                    .into_json::<serde_json::Value>()
                    .unwrap_or(serde_json::Value::Null);
                Ok(HttpResponse {
                    status,
                    body: BodyOrBytes::Json(body),
                    retry_after,
                })
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

fn parse_retry_after(resp: &ureq::Response) -> Option<u64> {
    resp.header("retry-after").and_then(|v| v.parse().ok())
}

/// The real transport used by every non-test provider: raw HTTP calls
/// through a per-provider circuit breaker (see `CircuitBreakerTransport`).
pub fn real_transport() -> Box<dyn Transport> {
    Box::new(CircuitBreakerTransport::new(Box::new(
        UreqTransport::default(),
    )))
}

const MAX_ATTEMPTS: u32 = 4;
const BASE_BACKOFF_MS: u64 = 250;
const MAX_BACKOFF_MS: u64 = 8_000;

/// Exponential backoff with full jitter (capped), honoring a vendor's
/// `Retry-After` on 429 instead of the computed delay when present.
fn backoff_delay(attempt: u32, retry_after: Option<u64>) -> Duration {
    if let Some(secs) = retry_after {
        return Duration::from_secs(secs.min(30));
    }
    let capped_ms = (BASE_BACKOFF_MS.saturating_mul(1u64 << attempt.min(20))).min(MAX_BACKOFF_MS);
    Duration::from_millis(jitter(capped_ms))
}

/// A cheap, non-cryptographic jitter source (0..=max_ms) — this is pacing
/// retries, not security-sensitive, so `SystemTime` subsecond nanos are
/// good enough without pulling in a `rand` dependency.
fn jitter(max_ms: u64) -> u64 {
    if max_ms == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    nanos % (max_ms + 1)
}

fn retry_with_backoff(
    transport: &dyn Transport,
    url: &str,
    headers: &[(String, String)],
    body: RequestBody,
    accept: ResponseKind,
) -> Result<HttpResponse, ProviderError> {
    let mut attempt = 1;
    loop {
        match transport.send(url, headers, body.clone(), accept) {
            Ok(resp) if resp.status < 400 => return Ok(resp),
            Ok(resp) => {
                let retryable = resp.status == 429 || resp.status >= 500;
                if !retryable || attempt >= MAX_ATTEMPTS {
                    return Err(status_to_error(resp.status, &resp.body));
                }
                std::thread::sleep(backoff_delay(attempt, resp.retry_after));
            }
            Err(e @ (ProviderError::Timeout | ProviderError::Failed(_))) => {
                if attempt >= MAX_ATTEMPTS {
                    return Err(e);
                }
                std::thread::sleep(backoff_delay(attempt, None));
            }
            Err(e) => return Err(e),
        }
        attempt += 1;
    }
}

fn status_to_error(status: u16, body: &BodyOrBytes) -> ProviderError {
    if status == 429 {
        return ProviderError::RateLimited;
    }
    let detail = match body {
        BodyOrBytes::Json(v) => v
            .get("error")
            .and_then(|e| e.get("message").or(Some(e)))
            .map(|v| v.to_string())
            .unwrap_or_else(|| v.to_string()),
        BodyOrBytes::Bytes(b) => format!("<{} byte response>", b.len()),
    };
    ProviderError::Failed(format!("HTTP {status}: {detail}"))
}

pub fn send_json_with_retry(
    transport: &dyn Transport,
    url: &str,
    headers: &[(String, String)],
    body: &serde_json::Value,
) -> Result<serde_json::Value, ProviderError> {
    match retry_with_backoff(
        transport,
        url,
        headers,
        RequestBody::Json(body.clone()),
        ResponseKind::Json,
    )?
    .body
    {
        BodyOrBytes::Json(v) => Ok(v),
        BodyOrBytes::Bytes(_) => Err(ProviderError::Failed(
            "expected a JSON response, got bytes".to_string(),
        )),
    }
}

pub fn send_multipart_with_retry(
    transport: &dyn Transport,
    url: &str,
    headers: &[(String, String)],
    fields: Vec<(String, String)>,
    file: (String, String, Vec<u8>),
) -> Result<serde_json::Value, ProviderError> {
    match retry_with_backoff(
        transport,
        url,
        headers,
        RequestBody::Multipart { fields, file },
        ResponseKind::Json,
    )?
    .body
    {
        BodyOrBytes::Json(v) => Ok(v),
        BodyOrBytes::Bytes(_) => Err(ProviderError::Failed(
            "expected a JSON response, got bytes".to_string(),
        )),
    }
}

pub fn send_json_expect_bytes_with_retry(
    transport: &dyn Transport,
    url: &str,
    headers: &[(String, String)],
    body: &serde_json::Value,
) -> Result<Vec<u8>, ProviderError> {
    match retry_with_backoff(
        transport,
        url,
        headers,
        RequestBody::Json(body.clone()),
        ResponseKind::Bytes,
    )?
    .body
    {
        BodyOrBytes::Bytes(b) => Ok(b),
        BodyOrBytes::Json(v) => Err(ProviderError::Failed(format!(
            "expected an audio byte response, got JSON: {v}"
        ))),
    }
}

/// A `Transport` decorator (§12.6's "cross-cutting middleware ... attaches
/// as a filter" pattern, applied to the transport layer): after
/// `threshold` consecutive 5xx/transport-level failures it trips open and
/// fails fast (no network call) for `cooldown`, then allows one half-open
/// trial call through. A 4xx (including 429, handled by
/// `retry_with_backoff` instead) never counts against the breaker — it
/// means the server is reachable and responding, just declining this call.
pub struct CircuitBreakerTransport {
    inner: Box<dyn Transport>,
    threshold: u32,
    cooldown: Duration,
    failures: AtomicU32,
    opened_at: Mutex<Option<Instant>>,
}

impl CircuitBreakerTransport {
    pub fn new(inner: Box<dyn Transport>) -> Self {
        Self::with_policy(inner, 5, Duration::from_secs(30))
    }

    pub fn with_policy(inner: Box<dyn Transport>, threshold: u32, cooldown: Duration) -> Self {
        CircuitBreakerTransport {
            inner,
            threshold,
            cooldown,
            failures: AtomicU32::new(0),
            opened_at: Mutex::new(None),
        }
    }

    fn is_open(&self) -> bool {
        let mut opened_at = self.opened_at.lock().unwrap();
        match *opened_at {
            Some(at) if at.elapsed() < self.cooldown => true,
            Some(_) => {
                // Cooldown elapsed: half-open, let the next call through as a trial.
                *opened_at = None;
                false
            }
            None => false,
        }
    }

    fn record_result(&self, healthy: bool) {
        if healthy {
            self.failures.store(0, Ordering::SeqCst);
            *self.opened_at.lock().unwrap() = None;
        } else {
            let failures = self.failures.fetch_add(1, Ordering::SeqCst) + 1;
            if failures >= self.threshold {
                *self.opened_at.lock().unwrap() = Some(Instant::now());
            }
        }
    }
}

impl Transport for CircuitBreakerTransport {
    fn send(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: RequestBody,
        accept: ResponseKind,
    ) -> Result<HttpResponse, ProviderError> {
        if self.is_open() {
            return Err(ProviderError::Failed(
                "circuit breaker open: too many recent failures".to_string(),
            ));
        }
        let result = self.inner.send(url, headers, body, accept);
        let healthy = match &result {
            Ok(resp) => resp.status < 500,
            Err(_) => false,
        };
        self.record_result(healthy);
        result
    }
}

#[cfg(test)]
pub struct ScriptedTransport {
    responses: Mutex<std::collections::VecDeque<Result<HttpResponse, ProviderError>>>,
}

#[cfg(test)]
impl ScriptedTransport {
    pub fn new(responses: Vec<Result<HttpResponse, ProviderError>>) -> Self {
        ScriptedTransport {
            responses: Mutex::new(responses.into_iter().collect()),
        }
    }

    pub fn ok(status: u16, body: serde_json::Value) -> Result<HttpResponse, ProviderError> {
        Ok(HttpResponse {
            status,
            body: BodyOrBytes::Json(body),
            retry_after: None,
        })
    }

    pub fn ok_bytes(status: u16, bytes: Vec<u8>) -> Result<HttpResponse, ProviderError> {
        Ok(HttpResponse {
            status,
            body: BodyOrBytes::Bytes(bytes),
            retry_after: None,
        })
    }
}

#[cfg(test)]
impl Transport for ScriptedTransport {
    fn send(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        _body: RequestBody,
        _accept: ResponseKind,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_breaker_opens_after_threshold_failures_then_fails_fast() {
        let scripted = ScriptedTransport::new(vec![
            Err(ProviderError::Failed("boom 1".to_string())),
            Err(ProviderError::Failed("boom 2".to_string())),
        ]);
        let breaker =
            CircuitBreakerTransport::with_policy(Box::new(scripted), 2, Duration::from_secs(30));

        assert!(breaker
            .send(
                "http://x",
                &[],
                RequestBody::Json(serde_json::json!({})),
                ResponseKind::Json
            )
            .is_err());
        assert!(breaker
            .send(
                "http://x",
                &[],
                RequestBody::Json(serde_json::json!({})),
                ResponseKind::Json
            )
            .is_err());

        // Third call: no scripted response left, but the breaker should
        // trip before even asking the inner transport.
        let err = breaker
            .send(
                "http://x",
                &[],
                RequestBody::Json(serde_json::json!({})),
                ResponseKind::Json,
            )
            .err()
            .unwrap();
        assert_eq!(
            err,
            ProviderError::Failed("circuit breaker open: too many recent failures".to_string())
        );
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let scripted = ScriptedTransport::new(vec![
            Err(ProviderError::Failed("boom".to_string())),
            ScriptedTransport::ok(200, serde_json::json!({"ok": true})),
            Err(ProviderError::Failed("boom".to_string())),
            Err(ProviderError::Failed("boom".to_string())),
        ]);
        let breaker =
            CircuitBreakerTransport::with_policy(Box::new(scripted), 2, Duration::from_secs(30));

        assert!(breaker
            .send(
                "http://x",
                &[],
                RequestBody::Json(serde_json::json!({})),
                ResponseKind::Json
            )
            .is_err());
        assert!(breaker
            .send(
                "http://x",
                &[],
                RequestBody::Json(serde_json::json!({})),
                ResponseKind::Json
            )
            .is_ok());
        // Failure count reset by the success above, so two more failures
        // shouldn't trip the breaker yet (threshold is 2, this is only 2
        // consecutive failures again but not a 3rd — exactly at threshold).
        assert!(breaker
            .send(
                "http://x",
                &[],
                RequestBody::Json(serde_json::json!({})),
                ResponseKind::Json
            )
            .is_err());
        let err = breaker
            .send(
                "http://x",
                &[],
                RequestBody::Json(serde_json::json!({})),
                ResponseKind::Json,
            )
            .err()
            .unwrap();
        assert_eq!(err, ProviderError::Failed("boom".to_string()));
    }

    #[test]
    fn rate_limit_status_never_trips_the_breaker() {
        let scripted = ScriptedTransport::new(vec![
            ScriptedTransport::ok(429, serde_json::json!({})),
            ScriptedTransport::ok(429, serde_json::json!({})),
            ScriptedTransport::ok(429, serde_json::json!({})),
            ScriptedTransport::ok(200, serde_json::json!({"ok": true})),
        ]);
        let breaker =
            CircuitBreakerTransport::with_policy(Box::new(scripted), 2, Duration::from_secs(30));
        for _ in 0..3 {
            let resp = breaker
                .send(
                    "http://x",
                    &[],
                    RequestBody::Json(serde_json::json!({})),
                    ResponseKind::Json,
                )
                .unwrap();
            assert_eq!(resp.status, 429);
        }
        let resp = breaker
            .send(
                "http://x",
                &[],
                RequestBody::Json(serde_json::json!({})),
                ResponseKind::Json,
            )
            .unwrap();
        assert_eq!(resp.status, 200);
    }
}
