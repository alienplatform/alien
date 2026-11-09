//! Log queries against a manager's data service.
//!
//! The platform signs a short-lived grant for one project's `logs` table
//! (`createDataGrant`). The grant is sent as a Bearer credential straight to the
//! data service URL the platform returns, which answers queries without calling
//! the platform again.

use std::time::Duration as StdDuration;

use alien_error::{AlienError, Context, ContextError as _, IntoAlienError};
use alien_platform_api::{types, Error as SdkError};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{ErrorData, Result};

/// A grant is replaced when it has less than this long left, so a request never
/// leaves with a grant that expires in flight.
const GRANT_REFRESH_MARGIN_SECONDS: i64 = 30;

const QUERY_TIMEOUT: StdDuration = StdDuration::from_secs(60);

/// The longest piece of a response body quoted in an error message.
const ERROR_BODY_LIMIT: usize = 300;

/// Sort order for a query. The data service orders by `timestampNanos`; a leading
/// `-` means newest first.
pub(crate) const NEWEST_FIRST: &str = "-timestampNanos";
pub(crate) const OLDEST_FIRST: &str = "timestampNanos";

#[derive(Debug, Clone)]
pub(crate) struct DataGrant {
    token: String,
    expires_at: DateTime<Utc>,
    data_url: String,
    pub(crate) database_id: String,
    table_id: String,
}

impl DataGrant {
    fn expires_within(&self, now: DateTime<Utc>, margin: Duration) -> bool {
        self.expires_at - now < margin
    }

    fn query_url(&self) -> String {
        format!(
            "{}/v2/databases/{}/tables/{}/query",
            self.data_url.trim_end_matches('/'),
            urlencoding::encode(&self.database_id),
            urlencoding::encode(&self.table_id),
        )
    }
}

/// Fetches grants for one project's logs on one manager and keeps the current one
/// until it is about to expire.
pub(crate) struct GrantSource {
    client: alien_platform_api::Client,
    workspace: String,
    manager_id: String,
    project_id: String,
    cached: Option<DataGrant>,
}

impl GrantSource {
    pub(crate) fn new(
        client: alien_platform_api::Client,
        workspace: String,
        manager_id: String,
        project_id: String,
    ) -> Self {
        Self {
            client,
            workspace,
            manager_id,
            project_id,
            cached: None,
        }
    }

    pub(crate) async fn current(&mut self) -> Result<&DataGrant> {
        self.current_at(Utc::now()).await
    }

    async fn current_at(&mut self, now: DateTime<Utc>) -> Result<&DataGrant> {
        let usable = self.cached.as_ref().is_some_and(|grant| {
            !grant.expires_within(now, Duration::seconds(GRANT_REFRESH_MARGIN_SECONDS))
        });
        if !usable {
            self.cached = Some(self.request().await?);
        }
        Ok(self.cached.as_ref().expect("grant was just stored"))
    }

    /// Drop the cached grant, for example after the data service refused it.
    fn invalidate(&mut self) {
        self.cached = None;
    }

    async fn request(&self) -> Result<DataGrant> {
        let body = types::CreateDataGrantRequest::builder()
            .action(types::CreateDataGrantRequestAction::Query)
            .table(types::CreateDataGrantRequestTable::Logs)
            .project_id(self.project_id.as_str());
        let result = self
            .client
            .create_data_grant()
            .id(self.manager_id.as_str())
            .workspace(self.workspace.as_str())
            .body(body)
            .send()
            .await;
        match result {
            Ok(response) => {
                let grant = response.into_inner();
                Ok(DataGrant {
                    token: grant.grant,
                    expires_at: grant.expires_at,
                    data_url: grant.data_url,
                    database_id: grant.database_id,
                    table_id: grant.table_id,
                })
            }
            Err(error) => Err(grant_error(error, &self.project_id, &self.manager_id)),
        }
    }
}

/// Turn a failed `createDataGrant` call into an error that says what to do.
fn grant_error(
    error: SdkError<types::ApiError>,
    project_id: &str,
    manager_id: &str,
) -> AlienError<ErrorData> {
    let status = match &error {
        SdkError::ErrorResponse(response) => Some(response.status().as_u16()),
        SdkError::UnexpectedResponse(response) => Some(response.status().as_u16()),
        _ => None,
    };
    match (status, &error) {
        (Some(403), _) => AlienError::new(ErrorData::LogDataAccessDenied {
            project: project_id.to_string(),
            manager_id: manager_id.to_string(),
        }),
        (Some(409), _) => AlienError::new(ErrorData::LogDataServiceNotConfigured),
        (Some(503), _) => AlienError::new(ErrorData::LogManagerOffline {
            manager_id: manager_id.to_string(),
        }),
        // A server that knows the route answers 404 with an API error about the
        // manager or the project. Any other 404, including an empty or plain-text
        // body, comes from a server that has no such route.
        (Some(404), SdkError::ErrorResponse(response))
            if response.code.contains("MANAGER") || response.code.contains("PROJECT") =>
        {
            api_failure(error, "Failed to request a log data grant")
        }
        (Some(404), _) => AlienError::new(ErrorData::LogDataGrantsUnsupported),
        // The declared 404 body is parsed as an API error before the status is
        // visible, so a plain-text 404 surfaces as a payload that is not a grant.
        (None, SdkError::InvalidResponsePayload(body, _)) if !looks_like_grant(body) => {
            AlienError::new(ErrorData::LogDataGrantsUnsupported)
        }
        _ => api_failure(error, "Failed to request a log data grant"),
    }
}

fn looks_like_grant(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .is_some_and(|value| value.get("grant").is_some())
}

fn api_failure(error: SdkError<types::ApiError>, message: &str) -> AlienError<ErrorData> {
    alien_platform_api::convert_sdk_error(error).context(ErrorData::ApiRequestFailed {
        message: message.to_string(),
        url: None,
    })
}

/// The body of a `query` request. Field names are the data service's.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryBody {
    pub search: String,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub filter: Map<String, Value>,
    pub sort_by: &'static str,
    pub start: String,
    pub end: String,
    pub limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryPage {
    #[serde(default)]
    rows: Vec<Value>,
    #[serde(default)]
    num_hits: u64,
    next_cursor: Option<String>,
}

#[derive(Debug)]
pub(crate) struct QueryResult {
    pub rows: Vec<Value>,
    /// Matches in the window, reported by the first page.
    pub num_hits: u64,
}

pub(crate) struct DataClient {
    http: reqwest::Client,
    grants: GrantSource,
}

impl DataClient {
    pub(crate) fn new(grants: GrantSource) -> Result<Self> {
        let http = crate::http_client::builder()
            .timeout(QUERY_TIMEOUT)
            .build()
            .into_alien_error()
            .context(ErrorData::NetworkError {
                message: "Failed to create HTTP client".to_string(),
            })?;
        Ok(Self { http, grants })
    }

    /// The grant in use, fetching one if there is none yet.
    pub(crate) async fn grant(&mut self) -> Result<&DataGrant> {
        self.grants.current().await
    }

    /// Run `body`, following cursors until `max_rows` rows are collected or the
    /// matches run out. Every page repeats the same body (including `limit`), as
    /// the data service requires for a cursor to apply.
    pub(crate) async fn query(&mut self, body: &QueryBody, max_rows: usize) -> Result<QueryResult> {
        let mut page_body = body.clone();
        let mut rows = Vec::new();
        let mut num_hits = None;
        loop {
            let page = self.query_page(&page_body).await?;
            num_hits.get_or_insert(page.num_hits);
            let page_was_empty = page.rows.is_empty();
            rows.extend(page.rows);
            match page.next_cursor {
                Some(cursor) if !page_was_empty && rows.len() < max_rows => {
                    page_body.cursor = Some(cursor);
                }
                _ => break,
            }
        }
        rows.truncate(max_rows);
        Ok(QueryResult {
            rows,
            num_hits: num_hits.unwrap_or(0),
        })
    }

    async fn query_page(&mut self, body: &QueryBody) -> Result<QueryPage> {
        // A 401 means the data service no longer accepts the grant, for example
        // because it expired against the service's clock. Fetch a new one once.
        for attempt in 0..2 {
            let grant = self.grants.current().await?.clone();
            let url = grant.query_url();
            let response = self
                .http
                .post(&url)
                .bearer_auth(&grant.token)
                .json(body)
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::NetworkError {
                    message: format!("Failed to reach the log data service at {}", grant.data_url),
                })?;
            let status = response.status();
            let bytes =
                response
                    .bytes()
                    .await
                    .into_alien_error()
                    .context(ErrorData::NetworkError {
                        message: "Failed to read the log data service response".to_string(),
                    })?;

            if status.is_success() {
                return serde_json::from_slice(&bytes).into_alien_error().context(
                    ErrorData::ApiRequestFailed {
                        message: "The log data service returned a response the CLI cannot read"
                            .to_string(),
                        url: Some(url),
                    },
                );
            }
            if status.as_u16() == 401 && attempt == 0 {
                self.grants.invalidate();
                continue;
            }
            return Err(data_service_error(status.as_u16(), &bytes, url));
        }
        unreachable!("the second attempt always returns")
    }
}

fn data_service_error(status: u16, body: &[u8], url: String) -> AlienError<ErrorData> {
    let text = String::from_utf8_lossy(body);
    let reason = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            ["error", "message"]
                .into_iter()
                .find_map(|key| value.get(key).and_then(Value::as_str).map(str::to_owned))
        })
        .unwrap_or_else(|| truncate(text.trim(), ERROR_BODY_LIMIT));
    match status {
        400 => AlienError::new(ErrorData::LogSearchRejected { message: reason }),
        401 | 403 => AlienError::new(ErrorData::ApiRequestFailed {
            message: format!(
                "The log data service refused the data grant (HTTP {status}): {reason}"
            ),
            url: Some(url),
        }),
        _ => AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("The log data service failed (HTTP {status}): {reason}"),
            url: Some(url),
        }),
    }
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut shortened: String = value.chars().take(limit).collect();
    shortened.push_str("...");
    shortened
}

/// A stand-in for the platform's `createDataGrant` and the data service's
/// `query`, which records every request it receives.
#[cfg(test)]
pub(crate) mod test_server {
    use std::sync::{Arc, Mutex};

    use axum::{body::Body, extract::State, http::Response, routing::any, Router};
    use serde_json::{json, Value};

    use super::{DataClient, GrantSource};

    #[derive(Debug, Clone)]
    pub struct Recorded {
        pub path: String,
        pub query: String,
        pub authorization: Option<String>,
        pub body: Value,
    }

    /// Decides the data service's answer: `(request number from 0, body)` to
    /// `(status, response body)`.
    type Responder = Box<dyn Fn(usize, &Value) -> (u16, Value) + Send + Sync>;

    struct Inner {
        grants: Vec<Recorded>,
        queries: Vec<Recorded>,
        ttl_seconds: i64,
        grant_failure: Option<(u16, String)>,
        responder: Responder,
        data_url: String,
    }

    pub struct TestServer {
        pub base_url: String,
        inner: Arc<Mutex<Inner>>,
    }

    impl TestServer {
        pub async fn start(
            responder: impl Fn(usize, &Value) -> (u16, Value) + Send + Sync + 'static,
        ) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base_url = format!("http://{}", listener.local_addr().unwrap());
            let inner = Arc::new(Mutex::new(Inner {
                grants: Vec::new(),
                queries: Vec::new(),
                ttl_seconds: 3600,
                grant_failure: None,
                responder: Box::new(responder),
                data_url: base_url.clone(),
            }));
            let app = Router::new()
                .fallback(any(handle))
                .with_state(Arc::clone(&inner));
            tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            Self { base_url, inner }
        }

        /// Seconds a granted token stays valid, as the platform reports it.
        pub fn set_grant_ttl(&self, seconds: i64) {
            self.inner.lock().unwrap().ttl_seconds = seconds;
        }

        /// Make `createDataGrant` answer `status` with `body` (raw text).
        pub fn fail_grants(&self, status: u16, body: &str) {
            self.inner.lock().unwrap().grant_failure = Some((status, body.to_string()));
        }

        pub fn grant_requests(&self) -> Vec<Recorded> {
            self.inner.lock().unwrap().grants.clone()
        }

        pub fn query_requests(&self) -> Vec<Recorded> {
            self.inner.lock().unwrap().queries.clone()
        }

        pub fn data_client(&self) -> DataClient {
            let client = alien_platform_api::Client::new(&self.base_url);
            DataClient::new(GrantSource::new(
                client,
                "demo".to_string(),
                "mgr_0000000000000000000000000001".to_string(),
                PROJECT.to_string(),
            ))
            .unwrap()
        }
    }

    pub const PROJECT: &str = "prj_0000000000000000000000000001";

    async fn handle(
        State(inner): State<Arc<Mutex<Inner>>>,
        request: axum::extract::Request,
    ) -> Response<Body> {
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let recorded = Recorded {
            path: parts.uri.path().to_string(),
            query: parts.uri.query().unwrap_or_default().to_string(),
            authorization: parts
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        };
        let mut inner = inner.lock().unwrap();
        let (status, body) = if recorded.path.ends_with("/data-grants") {
            inner.grants.push(recorded);
            let number = inner.grants.len();
            match &inner.grant_failure {
                Some((status, body)) => (*status, body.clone()),
                None => {
                    let expires_at =
                        chrono::Utc::now() + chrono::Duration::seconds(inner.ttl_seconds);
                    let body = json!({
                        "grant": format!("grant-{number}"),
                        "expiresAt": expires_at.to_rfc3339(),
                        "dataUrl": inner.data_url,
                        "tenantId": "ws_000000000000000000000000",
                        "databaseId": PROJECT,
                        "tableId": "logs",
                    });
                    (200, body.to_string())
                }
            }
        } else {
            let number = inner.queries.len();
            let (status, body) = (inner.responder)(number, &recorded.body);
            inner.queries.push(recorded);
            (status, body.to_string())
        };
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::test_server::{TestServer, PROJECT};
    use super::*;

    fn query_body(limit: usize) -> QueryBody {
        QueryBody {
            search: "(timeout)".to_string(),
            filter: serde_json::from_value(json!({"resource.alien.deployment_id": "dep_1"}))
                .unwrap(),
            sort_by: NEWEST_FIRST,
            start: "2026-11-09T10:00:00.000000000Z".to_string(),
            end: "2026-11-09T11:00:00.000000000Z".to_string(),
            limit,
            cursor: None,
        }
    }

    fn row(nanos: i64, message: &str) -> Value {
        json!({
            "timestampNanos": nanos.to_string(),
            "severityText": "INFO",
            "body": message,
        })
    }

    fn page(rows: Vec<Value>, cursor: Option<&str>, num_hits: u64) -> (u16, Value) {
        (
            200,
            json!({
                "rows": rows,
                "rowMetadata": [],
                "coverage": {"complete": true},
                "numHits": num_hits,
                "nextCursor": cursor,
            }),
        )
    }

    #[tokio::test]
    async fn query_sends_the_grant_to_the_data_service_in_the_native_shape() {
        let server = TestServer::start(|_, _| page(vec![row(1, "a")], None, 7)).await;
        let mut client = server.data_client();

        let result = client.query(&query_body(50), 50).await.unwrap();

        assert_eq!(result.rows, vec![row(1, "a")]);
        assert_eq!(result.num_hits, 7);

        let grants = server.grant_requests();
        assert_eq!(grants.len(), 1);
        assert_eq!(
            grants[0].path,
            "/v1/managers/mgr_0000000000000000000000000001/data-grants"
        );
        assert_eq!(grants[0].query, "workspace=demo");
        assert_eq!(
            grants[0].body,
            json!({"projectId": PROJECT, "table": "logs", "action": "query"})
        );

        let queries = server.query_requests();
        assert_eq!(queries.len(), 1);
        assert_eq!(
            queries[0].path,
            format!("/v2/databases/{PROJECT}/tables/logs/query")
        );
        assert_eq!(queries[0].authorization.as_deref(), Some("Bearer grant-1"));
        assert_eq!(
            queries[0].body,
            json!({
                "search": "(timeout)",
                "filter": {"resource.alien.deployment_id": "dep_1"},
                "sortBy": "-timestampNanos",
                "start": "2026-11-09T10:00:00.000000000Z",
                "end": "2026-11-09T11:00:00.000000000Z",
                "limit": 50,
            })
        );
    }

    #[tokio::test]
    async fn empty_filter_is_not_sent() {
        let server = TestServer::start(|_, _| page(vec![], None, 0)).await;
        let mut client = server.data_client();
        let mut body = query_body(10);
        body.filter.clear();

        client.query(&body, 10).await.unwrap();

        assert!(server.query_requests()[0].body.get("filter").is_none());
    }

    #[tokio::test]
    async fn cursor_pages_repeat_the_body_until_the_limit_is_reached() {
        // Every page holds two rows and a cursor, but only five are wanted.
        let server = TestServer::start(|number, _| {
            let base = number as i64 * 2;
            page(
                vec![row(100 - base, "x"), row(99 - base, "y")],
                Some(&format!("cursor-{}", number + 1)),
                100,
            )
        })
        .await;
        let mut client = server.data_client();

        let result = client.query(&query_body(2), 5).await.unwrap();

        assert_eq!(result.rows.len(), 5);
        assert_eq!(result.num_hits, 100);
        let queries = server.query_requests();
        assert_eq!(queries.len(), 3, "stops once five rows are collected");
        let mut expected = query_body(2);
        for (number, query) in queries.iter().enumerate() {
            let mut body = query.body.clone();
            let cursor = body.as_object_mut().unwrap().remove("cursor");
            assert_eq!(
                cursor,
                (number > 0).then(|| json!(format!("cursor-{number}")))
            );
            expected.cursor = None;
            assert_eq!(body, serde_json::to_value(&expected).unwrap());
        }
    }

    #[tokio::test]
    async fn paging_stops_when_the_matches_run_out_or_a_page_is_empty() {
        let server = TestServer::start(|number, _| match number {
            0 => page(vec![row(2, "a")], Some("c1"), 2),
            1 => page(vec![row(1, "b")], None, 2),
            _ => unreachable!("no cursor after the last page"),
        })
        .await;
        let mut client = server.data_client();
        let result = client.query(&query_body(1), 10).await.unwrap();
        assert_eq!(result.rows.len(), 2);

        let server = TestServer::start(|_, _| page(vec![], Some("again"), 0)).await;
        let mut client = server.data_client();
        let result = client.query(&query_body(10), 10).await.unwrap();
        assert!(result.rows.is_empty());
        assert_eq!(server.query_requests().len(), 1);
    }

    #[tokio::test]
    async fn a_grant_is_fetched_once_and_reused() {
        let server = TestServer::start(|_, _| page(vec![], None, 0)).await;
        let mut client = server.data_client();

        for _ in 0..3 {
            client.query(&query_body(10), 10).await.unwrap();
        }

        assert_eq!(server.grant_requests().len(), 1);
        assert!(server
            .query_requests()
            .iter()
            .all(|query| query.authorization.as_deref() == Some("Bearer grant-1")));
    }

    #[tokio::test]
    async fn a_grant_close_to_expiry_is_replaced_before_the_next_request() {
        let server = TestServer::start(|_, _| page(vec![], None, 0)).await;
        // Inside the 30 second refresh margin from the start.
        server.set_grant_ttl(20);
        let mut client = server.data_client();

        client.query(&query_body(10), 10).await.unwrap();
        client.query(&query_body(10), 10).await.unwrap();

        assert_eq!(server.grant_requests().len(), 2);
        let tokens: Vec<_> = server
            .query_requests()
            .into_iter()
            .map(|query| query.authorization.unwrap())
            .collect();
        assert_eq!(tokens, ["Bearer grant-1", "Bearer grant-2"]);
    }

    #[tokio::test]
    async fn a_grant_with_time_left_is_kept() {
        let server = TestServer::start(|_, _| page(vec![], None, 0)).await;
        server.set_grant_ttl(120);
        let mut client = server.data_client();

        client.query(&query_body(10), 10).await.unwrap();
        client.query(&query_body(10), 10).await.unwrap();

        assert_eq!(server.grant_requests().len(), 1);
    }

    #[tokio::test]
    async fn a_grant_the_data_service_refuses_is_replaced_once() {
        let server = TestServer::start(|number, _| {
            if number == 0 {
                (401, json!({"error": "grant expired"}))
            } else {
                page(vec![row(1, "ok")], None, 1)
            }
        })
        .await;
        let mut client = server.data_client();

        let result = client.query(&query_body(10), 10).await.unwrap();

        assert_eq!(result.rows.len(), 1);
        let tokens: Vec<_> = server
            .query_requests()
            .into_iter()
            .map(|query| query.authorization.unwrap())
            .collect();
        assert_eq!(tokens, ["Bearer grant-1", "Bearer grant-2"]);

        let server = TestServer::start(|_, _| (401, json!({"error": "bad grant"}))).await;
        let mut client = server.data_client();
        let error = client.query(&query_body(10), 10).await.unwrap_err();
        assert_eq!(error.code, "API_REQUEST_FAILED");
        assert!(
            error.message.contains("refused the data grant"),
            "{}",
            error.message
        );
        assert_eq!(server.query_requests().len(), 2, "no endless retries");
    }

    fn api_error(code: &str, status: u16) -> String {
        json!({
            "code": code, "message": format!("{code} from the server"),
            "retryable": false, "internal": false, "httpStatusCode": status
        })
        .to_string()
    }

    #[tokio::test]
    async fn grant_failures_say_what_to_do() {
        let cases = [
            // An old server has no such route: empty or plain-text 404s, or a
            // generic JSON 404.
            (404, String::new(), "LOG_DATA_GRANTS_UNSUPPORTED"),
            (
                404,
                "404 Not Found".to_string(),
                "LOG_DATA_GRANTS_UNSUPPORTED",
            ),
            (
                404,
                api_error("NOT_FOUND", 404),
                "LOG_DATA_GRANTS_UNSUPPORTED",
            ),
            (403, api_error("FORBIDDEN", 403), "LOG_DATA_ACCESS_DENIED"),
            (
                409,
                api_error("DATA_SERVICE_UNAVAILABLE", 409),
                "LOG_DATA_SERVICE_NOT_CONFIGURED",
            ),
            (
                503,
                api_error("MANAGER_OFFLINE", 503),
                "LOG_MANAGER_OFFLINE",
            ),
            // A server that has the route but not the project or manager.
            (
                404,
                api_error("PROJECT_NOT_FOUND", 404),
                "API_REQUEST_FAILED",
            ),
            (
                404,
                api_error("MANAGER_NOT_FOUND", 404),
                "API_REQUEST_FAILED",
            ),
            (500, api_error("INTERNAL_ERROR", 500), "API_REQUEST_FAILED"),
        ];
        for (status, body, code) in cases {
            let server = TestServer::start(|_, _| page(vec![], None, 0)).await;
            server.fail_grants(status, &body);
            let mut client = server.data_client();

            let error = client.grant().await.unwrap_err();

            assert_eq!(error.code, code, "{status} {body}: {}", error.message);
            assert!(server.query_requests().is_empty());
        }

        let server = TestServer::start(|_, _| page(vec![], None, 0)).await;
        server.fail_grants(404, "");
        let error = server.data_client().grant().await.unwrap_err();
        assert_eq!(
            error.message,
            "This Alien server doesn't support log queries with data grants yet; update the server or use an older CLI"
        );
        assert_eq!(error.http_status_code, Some(404));

        server.fail_grants(403, &api_error("FORBIDDEN", 403));
        let error = server.data_client().grant().await.unwrap_err();
        assert!(error.message.contains(PROJECT), "{}", error.message);
        assert!(error.message.contains("mgr_0000000000000000000000000001"));

        server.fail_grants(503, &api_error("MANAGER_OFFLINE", 503));
        let error = server.data_client().grant().await.unwrap_err();
        assert!(error.message.contains("offline"), "{}", error.message);
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn data_service_errors_keep_the_reason() {
        let server = TestServer::start(|_, _| {
            (
                400,
                json!({"code": "INVALID_REQUEST", "error": "invalid search: unbalanced parenthesis", "status": "error"}),
            )
        })
        .await;
        let error = server
            .data_client()
            .query(&query_body(10), 10)
            .await
            .unwrap_err();
        assert_eq!(error.code, "INVALID_REQUEST");
        assert_eq!(error.http_status_code, Some(400));
        assert_eq!(
            error.message,
            "Log search rejected: invalid search: unbalanced parenthesis"
        );

        let server = TestServer::start(|_, _| (503, json!({"error": "index unavailable"}))).await;
        let error = server
            .data_client()
            .query(&query_body(10), 10)
            .await
            .unwrap_err();
        assert_eq!(error.code, "API_REQUEST_FAILED");
        assert!(error.message.contains("503") && error.message.contains("index unavailable"));
    }
}
