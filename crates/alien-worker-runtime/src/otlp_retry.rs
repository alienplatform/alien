//! Bounded OTLP/HTTP retries on the SDK's dedicated blocking export thread.
use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use opentelemetry_http::{HttpClient, HttpError};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

pub(crate) const DELIVERY_BUDGET: Duration = Duration::from_secs(60);
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub(crate) struct RetryingLogClient {
    client: OnceLock<reqwest::blocking::Client>,
    budget: Duration,
}

impl RetryingLogClient {
    pub(crate) fn new() -> Self {
        Self {
            client: OnceLock::new(),
            budget: DELIVERY_BUDGET,
        }
    }

    fn send(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        // Exporters are often configured inside an async runtime. Construct the
        // blocking client lazily on the SDK export thread, never during setup.
        let client = self.client.get_or_init(reqwest::blocking::Client::new);
        let deadline = Instant::now() + self.budget;
        let mut backoff = Duration::from_millis(200);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "OTLP delivery deadline exceeded",
                )));
            }
            let result = client
                .request(request.method().clone(), request.uri().to_string())
                .headers(request.headers().clone())
                .body(request.body().clone())
                .timeout(ATTEMPT_TIMEOUT.min(remaining))
                .send();
            let retry_after = match result {
                Ok(mut response) => {
                    if !matches!(response.status().as_u16(), 429 | 502 | 503 | 504) {
                        let status = response.status();
                        let headers = std::mem::take(response.headers_mut());
                        match response.bytes() {
                            Ok(body) => {
                                let mut result = Response::builder().status(status).body(body)?;
                                *result.headers_mut() = headers;
                                return Ok(result);
                            }
                            // A truncated success response is an ambiguous delivery,
                            // just like losing the connection before its headers.
                            Err(_) if status.is_success() => None,
                            Err(error) => return Err(Box::new(error)),
                        }
                    } else {
                        response
                            .headers()
                            .get(http::header::RETRY_AFTER)
                            .and_then(|value| value.to_str().ok())
                            .and_then(retry_after)
                    }
                }
                Err(error) => {
                    if !(error.is_timeout()
                        || error.is_connect()
                        || error.is_request()
                        || error.is_body())
                    {
                        return Err(Box::new(error));
                    }
                    None
                }
            };
            // Jitter prevents synchronized exporters retrying together. Never retry
            // before Retry-After, and never sleep or start an attempt past the budget.
            let jitter = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64
                % 200;
            let delay = retry_after.unwrap_or(backoff + Duration::from_millis(jitter));
            let remaining = deadline.saturating_duration_since(Instant::now());
            if delay >= remaining {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "OTLP retry exceeds delivery deadline",
                )));
            }
            std::thread::sleep(delay);
            backoff = (backoff * 2).min(Duration::from_secs(5));
        }
    }
}

fn retry_after(value: &str) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    (at.with_timezone(&chrono::Utc) - chrono::Utc::now())
        .to_std()
        .ok()
}

#[async_trait]
impl HttpClient for RetryingLogClient {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        // The SDK uses a blocking thread and may poll without a Tokio runtime.
        self.send(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn serve(
        statuses: Vec<u16>,
        retry_after: Option<&'static str>,
    ) -> (String, std::thread::JoinHandle<Vec<Vec<u8>>>) {
        serve_wire(statuses, retry_after, false)
    }

    fn serve_wire(
        statuses: Vec<u16>,
        retry_after: Option<&'static str>,
        truncate_first: bool,
    ) -> (String, std::thread::JoinHandle<Vec<Vec<u8>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/logs", listener.local_addr().unwrap());
        let task = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for (index, status) in statuses.into_iter().enumerate() {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut size = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                        size = value.trim().parse::<usize>().unwrap();
                    }
                }
                let mut body = vec![0; size];
                reader.read_exact(&mut body).unwrap();
                bodies.push(body);
                if status == 0 {
                    // Drop the connection before response headers arrive.
                    continue;
                }
                if truncate_first && index == 0 {
                    write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: 10\r\nConnection: close\r\n\r\nx").unwrap();
                    continue;
                }
                let retry_header = retry_after
                    .map(|value| format!("Retry-After: {value}\r\n"))
                    .unwrap_or_default();
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n{retry_header}\r\n").unwrap();
            }
            bodies
        });
        (url, task)
    }

    #[tokio::test]
    async fn pinned_sdk_exporter_retries_and_reports_permanent_failure() {
        use opentelemetry::logs::{LogRecord, Logger, LoggerProvider};
        use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
        for (statuses, succeeds) in [(vec![503, 200], true), (vec![401], false)] {
            let expected_attempts = statuses.len();
            let (url, server) = serve(statuses, Some("0"));
            let mut client = RetryingLogClient::new();
            client.budget = Duration::from_secs(2);
            let exporter = opentelemetry_otlp::LogExporter::builder()
                .with_http()
                .with_endpoint(url)
                .with_http_client(client)
                .build()
                .unwrap();
            let provider = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
                .with_batch_exporter(exporter)
                .build();
            let logger = provider.logger("retry-test");
            let mut log = logger.create_log_record();
            log.set_body("acknowledged-or-error".into());
            logger.emit(log);
            let flush_provider = provider.clone();
            assert_eq!(
                tokio::task::spawn_blocking(move || flush_provider.force_flush())
                    .await
                    .unwrap()
                    .is_ok(),
                succeeds
            );
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), expected_attempts);
            assert!(!requests[0].is_empty());
            assert!(requests.iter().all(|body| *body == requests[0]));
            tokio::task::spawn_blocking(move || provider.shutdown())
                .await
                .unwrap()
                .unwrap();
        }
    }

    #[test]
    fn retries_identical_payload_until_success() {
        let (url, server) = serve(vec![503, 429, 200], Some("0"));
        let mut client = RetryingLogClient::new();
        client.budget = Duration::from_secs(2);
        let response = client
            .send(
                Request::post(url)
                    .body(Bytes::from_static(b"same-batch"))
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(server.join().unwrap(), vec![b"same-batch".to_vec(); 3]);
    }

    #[test]
    fn truncated_success_body_retries_but_permanent_status_does_not() {
        let (url, server) = serve_wire(vec![200, 200], None, true);
        let response = RetryingLogClient::new()
            .send(
                Request::post(url)
                    .body(Bytes::from_static(b"ambiguous-batch"))
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(server.join().unwrap(), vec![b"ambiguous-batch".to_vec(); 2]);

        let (url, server) = serve_wire(vec![400], None, true);
        assert!(RetryingLogClient::new()
            .send(Request::post(url).body(Bytes::new()).unwrap())
            .is_err());
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn connection_failure_retries_identical_bytes_but_invalid_scheme_fails_fast() {
        let (url, server) = serve(vec![0, 200], None);
        let mut client = RetryingLogClient::new();
        client.budget = Duration::from_secs(2);
        let response = client
            .send(
                Request::post(url)
                    .body(Bytes::from_static(b"transport-batch"))
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(server.join().unwrap(), vec![b"transport-batch".to_vec(); 2]);
        let started = Instant::now();
        assert!(client
            .send(
                Request::post("ftp://127.0.0.1/logs")
                    .body(Bytes::new())
                    .unwrap()
            )
            .is_err());
        assert!(started.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn permanent_failures_are_not_retried() {
        for status in [400, 401, 403, 413, 500] {
            let (url, server) = serve(vec![status], None);
            let response = RetryingLogClient::new()
                .send(Request::post(url).body(Bytes::new()).unwrap())
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }

    #[test]
    fn retry_after_cannot_extend_deadline() {
        let (url, server) = serve(vec![503], Some("120"));
        let mut client = RetryingLogClient::new();
        client.budget = Duration::from_millis(100);
        let start = Instant::now();
        assert!(client
            .send(Request::post(url).body(Bytes::new()).unwrap())
            .is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(server.join().unwrap().len(), 1);
    }
}
