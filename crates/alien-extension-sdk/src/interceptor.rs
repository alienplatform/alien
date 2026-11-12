//! Interceptors: an extension's code inside a call Alien's controllers make, which changes the
//! request before Alien signs it and the response before Alien reads it.
//!
//! Alien hands the module every call in one HTTP shape, [`InterceptedRequest`]. The views here
//! read it the way each API carries its operation and parameters: AWS form keys or a JSON body,
//! Google Cloud and Azure paths and JSON bodies, and Kubernetes objects.

use alien_core::hooks::{
    InterceptedCloud, InterceptedRequest, InterceptedResponse, InterceptorInput, InterceptorOutput,
};
use alien_core::{ErrorData, Result};
use alien_error::AlienError;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde_json::Value;
use std::ops::Range;
use url::Url;

/// Changes calls Alien's controllers make to AWS.
pub trait AwsInterceptor {
    /// Whether this interceptor sees the call. Only a call it matches reaches the other methods.
    fn matches(&self, call: &AwsCall) -> bool;

    /// Changes the request before Alien signs it.
    fn modify_request(&self, request: &mut AwsRequest) -> Result<()>;

    /// Changes the response, error responses included, before Alien reads it.
    fn modify_response(&self, _response: &mut AwsResponse) -> Result<()> {
        Ok(())
    }
}

/// Changes calls Alien's controllers make to Google Cloud.
pub trait GcpInterceptor {
    /// Whether this interceptor sees the call. Only a call it matches reaches the other methods.
    fn matches(&self, call: &GcpCall) -> bool;

    /// Changes the request before Alien signs it.
    fn modify_request(&self, request: &mut GcpRequest) -> Result<()>;

    /// Changes the response, error responses included, before Alien reads it.
    fn modify_response(&self, _response: &mut GcpResponse) -> Result<()> {
        Ok(())
    }
}

/// Changes calls Alien's controllers make to Azure Resource Manager.
pub trait AzureInterceptor {
    /// Whether this interceptor sees the call. Only a call it matches reaches the other methods.
    fn matches(&self, call: &AzureCall) -> bool;

    /// Changes the request before Alien signs it.
    fn modify_request(&self, request: &mut AzureRequest) -> Result<()>;

    /// Changes the response, error responses included, before Alien reads it.
    fn modify_response(&self, _response: &mut AzureResponse) -> Result<()> {
        Ok(())
    }
}

/// Changes calls Alien makes to the Kubernetes API server.
pub trait KubernetesInterceptor {
    /// Whether this interceptor sees the call. Only a call it matches reaches the other methods.
    fn matches(&self, call: &KubernetesCall) -> bool;

    /// Changes the object Alien sends, such as a pod.
    fn modify_request(&self, object: &mut KubernetesObject) -> Result<()>;

    /// Changes the object the API server returns, before Alien reads it.
    fn modify_response(&self, _object: &mut KubernetesObject) -> Result<()> {
        Ok(())
    }
}

fn refused(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::GenericError {
        message: message.into(),
    })
}

/// The views a cloud's interceptor reads a call through.
trait CallView: Sized {
    fn read(request: &InterceptedRequest) -> Result<Self>;
}

trait RequestView: Sized {
    fn read(request: InterceptedRequest) -> Result<Self>;
    fn write(self) -> Result<InterceptedRequest>;
}

trait ResponseView: Sized {
    fn read(request: &InterceptedRequest, response: InterceptedResponse) -> Result<Self>;
    fn write(self) -> Result<InterceptedResponse>;
}

fn run<C: CallView, Q: RequestView, S: ResponseView>(
    cloud: InterceptedCloud,
    input: &[u8],
    matches: impl Fn(&C) -> bool,
    modify_request: impl Fn(&mut Q) -> Result<()>,
    modify_response: impl Fn(&mut S) -> Result<()>,
) -> std::result::Result<Vec<u8>, alien_core::hooks::HookError> {
    let input: InterceptorInput = crate::read_input(input, "intercepted call")?;
    let request = match &input {
        InterceptorInput::Matches { request }
        | InterceptorInput::ModifyRequest { request }
        | InterceptorInput::ModifyResponse { request, .. } => request,
    };
    if request.cloud != cloud {
        return Err(format!(
            "an interceptor for {cloud:?} got a call to {:?}",
            request.cloud
        )
        .into());
    }
    let output = (|| -> Result<InterceptorOutput> {
        Ok(match input {
            InterceptorInput::Matches { request } => {
                InterceptorOutput::Matches(matches(&C::read(&request)?))
            }
            InterceptorInput::ModifyRequest { request } => {
                let mut view = Q::read(request)?;
                modify_request(&mut view)?;
                InterceptorOutput::Request(view.write()?)
            }
            InterceptorInput::ModifyResponse { request, response } => {
                let mut view = S::read(&request, response)?;
                modify_response(&mut view)?;
                InterceptorOutput::Response(view.write()?)
            }
        })
    })()
    .map_err(alien_core::hooks::HookError::from)?;
    crate::write_output(&output)
}

pub(crate) fn run_aws(
    interceptor: &dyn AwsInterceptor,
    input: &[u8],
) -> std::result::Result<Vec<u8>, alien_core::hooks::HookError> {
    run::<AwsCall, AwsRequest, AwsResponse>(
        InterceptedCloud::Aws,
        input,
        |call| interceptor.matches(call),
        |request| interceptor.modify_request(request),
        |response| interceptor.modify_response(response),
    )
}

pub(crate) fn run_gcp(
    interceptor: &dyn GcpInterceptor,
    input: &[u8],
) -> std::result::Result<Vec<u8>, alien_core::hooks::HookError> {
    run::<GcpCall, GcpRequest, GcpResponse>(
        InterceptedCloud::Gcp,
        input,
        |call| interceptor.matches(call),
        |request| interceptor.modify_request(request),
        |response| interceptor.modify_response(response),
    )
}

pub(crate) fn run_azure(
    interceptor: &dyn AzureInterceptor,
    input: &[u8],
) -> std::result::Result<Vec<u8>, alien_core::hooks::HookError> {
    run::<AzureCall, AzureRequest, AzureResponse>(
        InterceptedCloud::Azure,
        input,
        |call| interceptor.matches(call),
        |request| interceptor.modify_request(request),
        |response| interceptor.modify_response(response),
    )
}

pub(crate) fn run_kubernetes(
    interceptor: &dyn KubernetesInterceptor,
    input: &[u8],
) -> std::result::Result<Vec<u8>, alien_core::hooks::HookError> {
    run::<KubernetesCall, KubernetesObject, KubernetesObject>(
        InterceptedCloud::Kubernetes,
        input,
        |call| interceptor.matches(call),
        |object| interceptor.modify_request(object),
        |object| interceptor.modify_response(object),
    )
}

/// The parts of a request every cloud's view shares: where it goes and its headers.
struct Http {
    method: String,
    url: Url,
    /// The URL as Alien wrote it, sent back unchanged unless the path or query changed.
    original_url: String,
    headers: Vec<(String, String)>,
}

impl Http {
    fn read(request: &InterceptedRequest) -> Result<Self> {
        let url = Url::parse(&request.url).map_err(|error| {
            refused(format!(
                "the call's URL {} did not parse: {error}",
                request.url
            ))
        })?;
        Ok(Self {
            method: request.method.clone(),
            url,
            original_url: request.url.clone(),
            headers: request.headers.clone(),
        })
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn url_text(&self) -> String {
        match Url::parse(&self.original_url) {
            Ok(original) if original == self.url => self.original_url.clone(),
            _ => self.url.to_string(),
        }
    }

    fn write(self, request: InterceptedRequest, body: Option<String>) -> InterceptedRequest {
        InterceptedRequest {
            method: self.method.clone(),
            url: self.url_text(),
            headers: self.headers,
            body,
            ..request
        }
    }
}

/// Writes the accessors every request view shares.
macro_rules! http_accessors {
    ($view:ty) => {
        impl $view {
            /// The HTTP method, such as `POST`.
            pub fn method(&self) -> &str {
                &self.http.method
            }

            /// The URL's path, such as `/v1/projects/p/instances`.
            pub fn path(&self) -> &str {
                self.http.url.path()
            }

            /// The URL's query, without the `?`.
            pub fn query(&self) -> Option<&str> {
                self.http.url.query()
            }

            /// Replaces the URL's path. The scheme, host and port never change.
            pub fn set_path(&mut self, path: &str) {
                self.http.url.set_path(path);
            }

            /// Replaces the URL's query.
            pub fn set_query(&mut self, query: Option<&str>) {
                self.http.url.set_query(query);
            }

            /// The headers, in order. Alien refuses a change to `Host` or to a signing header.
            pub fn headers_mut(&mut self) -> &mut Vec<(String, String)> {
                &mut self.http.headers
            }
        }
    };
}

/// A text body read as JSON: none is `null`, and text that isn't JSON is a JSON string.
fn json_body(text: Option<&str>) -> Value {
    match text {
        None | Some("") => Value::Null,
        Some(text) => {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
        }
    }
}

/// A JSON body as text: the original bytes when it is unchanged, and none when it is `null`.
fn json_text(value: &Value, original: Option<String>) -> Result<Option<String>> {
    if json_body(original.as_deref()) == *value {
        return Ok(original);
    }
    if value.is_null() {
        return Ok(None);
    }
    serde_json::to_string(value)
        .map(Some)
        .map_err(|error| refused(format!("the changed body did not serialize: {error}")))
}

// ---- AWS ----

/// How an AWS request carries its parameters.
#[derive(Debug, Clone, PartialEq)]
enum AwsBody {
    /// A query API's form: RDS, EC2, IAM, SQS and others.
    Query(Vec<(String, String)>),
    /// A JSON body: a JSON API (DynamoDB, KMS and others) or a REST API with JSON.
    Json(Value),
    /// No body, or text that is neither, such as S3's XML.
    Text(Option<String>),
}

impl AwsBody {
    fn read(http: &Http, body: Option<&str>) -> Self {
        let form = http
            .header("content-type")
            .is_some_and(|value| value.starts_with("application/x-www-form-urlencoded"));
        match body {
            Some(text) if form => Self::Query(
                form_urlencoded::parse(text.as_bytes())
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect(),
            ),
            Some(text) => match serde_json::from_str(text) {
                Ok(json @ Value::Object(_)) => Self::Json(json),
                _ => Self::Text(Some(text.to_string())),
            },
            None => Self::Text(None),
        }
    }

    fn write(&self) -> Result<Option<String>> {
        Ok(match self {
            Self::Query(form) => Some(
                form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(form)
                    .finish(),
            ),
            Self::Json(json) => Some(serde_json::to_string(json).map_err(|error| {
                refused(format!("the changed body did not serialize: {error}"))
            })?),
            Self::Text(text) => text.clone(),
        })
    }
}

/// An AWS call, as [`AwsInterceptor::matches`] sees it.
pub struct AwsCall {
    service: String,
    operation: String,
    method: String,
    path: String,
}

impl AwsCall {
    /// The service the call is signed for, such as `rds` or `dynamodb`.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The operation, such as `CreateDBCluster`: a query API's `Action`, or the one a JSON API's
    /// `X-Amz-Target` header names. A REST API such as S3 or Lambda names none on the wire, so
    /// this is empty; match on [`method`](Self::method) and [`path`](Self::path) instead.
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// The HTTP method.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The URL's path.
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl CallView for AwsCall {
    fn read(request: &InterceptedRequest) -> Result<Self> {
        let http = Http::read(request)?;
        let action = |pairs: Vec<(String, String)>| {
            pairs
                .into_iter()
                .find(|(key, _)| key == "Action")
                .map(|(_, value)| value)
        };
        let operation = match (
            http.header("x-amz-target"),
            AwsBody::read(&http, request.body.as_deref()),
        ) {
            (Some(target), _) => target.rsplit('.').next().map(str::to_string),
            (None, AwsBody::Query(form)) => action(form),
            (None, _) => action(http.url.query_pairs().into_owned().collect()),
        };
        Ok(Self {
            service: request.service.clone(),
            operation: operation.unwrap_or_default(),
            method: http.method.clone(),
            path: http.url.path().to_string(),
        })
    }
}

/// An AWS request, in the wire format its API uses.
pub struct AwsRequest {
    request: InterceptedRequest,
    http: Http,
    body: AwsBody,
    original_body: AwsBody,
    /// A change the request's format can't carry, reported when the interceptor returns.
    misuse: Option<String>,
}

http_accessors!(AwsRequest);

impl AwsRequest {
    /// A query API request for tests: `Action` is `operation`, and each field of `params`
    /// becomes a form value.
    pub fn new(service: &str, operation: &str, params: Value) -> Self {
        let mut form = vec![("Action".to_string(), operation.to_string())];
        if let Value::Object(params) = params {
            form.extend(
                params
                    .into_iter()
                    .map(|(key, value)| (key, form_value(value))),
            );
        }
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&form)
            .finish();
        let request = InterceptedRequest {
            cloud: InterceptedCloud::Aws,
            service: service.to_string(),
            method: "POST".to_string(),
            url: format!("https://{service}.us-east-1.amazonaws.com/"),
            headers: vec![(
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            )],
            body: Some(body),
            tenant_id: None,
        };
        RequestView::read(request).expect("a request built here reads back")
    }

    /// A query API's form value, such as `param("Tags.Tag.1.Key")`.
    pub fn param(&self, key: &str) -> Option<&str> {
        match &self.body {
            AwsBody::Query(form) => form
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str()),
            _ => None,
        }
    }

    /// Sets a form value on a query API, or a top-level field on a JSON API, replacing it when
    /// present. `set("EnableIAMDatabaseAuthentication", true)` sets the form value `true`.
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        let value = value.into();
        match &mut self.body {
            AwsBody::Query(form) => {
                let value = form_value(value);
                match form.iter_mut().find(|(name, _)| name == key) {
                    Some((_, existing)) => *existing = value,
                    None => form.push((key.to_string(), value)),
                }
            }
            AwsBody::Json(Value::Object(fields)) => {
                fields.insert(key.to_string(), value);
            }
            _ => self.misuse(format!("set(\"{key}\")")),
        }
    }

    /// Removes a form value on a query API, or a top-level field on a JSON API.
    pub fn remove(&mut self, key: &str) {
        match &mut self.body {
            AwsBody::Query(form) => form.retain(|(name, _)| name != key),
            AwsBody::Json(Value::Object(fields)) => {
                fields.shift_remove(key);
            }
            _ => self.misuse(format!("remove(\"{key}\")")),
        }
    }

    /// Adds a member to a query API's indexed list at the next free index, such as
    /// `push_member("Tags.Tag", [("Key", "team"), ("Value", "payments")])`, which writes
    /// `Tags.Tag.<n>.Key` and `Tags.Tag.<n>.Value` after the members already there.
    pub fn push_member<K: AsRef<str>, V: AsRef<str>>(
        &mut self,
        prefix: &str,
        fields: impl IntoIterator<Item = (K, V)>,
    ) {
        let AwsBody::Query(form) = &mut self.body else {
            return self.misuse(format!("push_member(\"{prefix}\")"));
        };
        let member_prefix = format!("{prefix}.");
        let next = form
            .iter()
            .filter_map(|(name, _)| {
                name.strip_prefix(&member_prefix)?
                    .split('.')
                    .next()?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .unwrap_or(0)
            + 1;
        for (field, value) in fields {
            form.push((
                format!("{prefix}.{next}.{}", field.as_ref()),
                value.as_ref().to_string(),
            ));
        }
    }

    /// The body as JSON, on a JSON API or a REST API that sends JSON. On a query API the body is
    /// a form, so this returns an error.
    pub fn body_mut(&mut self) -> Result<&mut Value> {
        match &mut self.body {
            AwsBody::Json(json) => Ok(json),
            AwsBody::Query(_) => Err(refused(
                "the body of a query API call is a form: read and change it with param, set, remove and push_member",
            )),
            AwsBody::Text(_) => Err(refused("the call has no JSON body")),
        }
    }

    /// The body as text, on a call whose body is neither a form nor JSON, such as S3's XML.
    pub fn text_mut(&mut self) -> Result<&mut Option<String>> {
        match &mut self.body {
            AwsBody::Text(text) => Ok(text),
            _ => Err(refused(
                "the call's body is a form or JSON: use param, set, remove, push_member or body_mut",
            )),
        }
    }

    fn misuse(&mut self, what: String) {
        self.misuse.get_or_insert(format!(
            "{what} needs a query API's form or a JSON body, and the {} call {} {} has neither",
            self.request.service,
            self.http.method,
            self.http.url.path()
        ));
    }
}

/// A JSON value as a query API form value: strings as they are, anything else as JSON text.
fn form_value(value: Value) -> String {
    match value {
        Value::String(text) => text,
        other => other.to_string(),
    }
}

impl RequestView for AwsRequest {
    fn read(request: InterceptedRequest) -> Result<Self> {
        let http = Http::read(&request)?;
        let body = AwsBody::read(&http, request.body.as_deref());
        Ok(Self {
            original_body: body.clone(),
            request,
            http,
            body,
            misuse: None,
        })
    }

    fn write(self) -> Result<InterceptedRequest> {
        if let Some(misuse) = self.misuse {
            return Err(refused(misuse));
        }
        let body = if self.body == self.original_body {
            self.request.body.clone()
        } else {
            self.body.write()?
        };
        Ok(self.http.write(self.request, body))
    }
}

/// An AWS response: XML on query APIs and S3, JSON elsewhere.
pub struct AwsResponse {
    response: InterceptedResponse,
    json: Option<Value>,
}

impl AwsResponse {
    /// The HTTP status.
    pub fn status(&self) -> u16 {
        self.response.status
    }

    /// Replaces the HTTP status, such as turning an error into a success.
    pub fn set_status(&mut self, status: u16) {
        self.response.status = status;
    }

    /// The headers, in order.
    pub fn headers_mut(&mut self) -> &mut Vec<(String, String)> {
        &mut self.response.headers
    }

    /// The body as text: the XML of a query API or S3.
    pub fn xml(&self) -> &str {
        &self.response.body
    }

    /// The text of the first element at `path`, the element names from the root down, such as
    /// `CreateDBClusterResponse/CreateDBClusterResult/DBCluster/Status`.
    pub fn get(&self, path: &str) -> Option<String> {
        match find_element(&self.response.body, path).ok()?? {
            Element::Text(range) => quick_xml::escape::unescape(&self.response.body[range])
                .ok()
                .map(|text| text.into_owned()),
            Element::Empty { .. } => Some(String::new()),
        }
    }

    /// Replaces the text of the first element at `path`. Fails when there is no such element.
    pub fn set(&mut self, path: &str, value: &str) -> Result<()> {
        let body = &mut self.response.body;
        let element = find_element(body, path)
            .map_err(|error| refused(format!("the response is not XML: {error}")))?
            .ok_or_else(|| refused(format!("the response has no element at {path}")))?;
        let escaped = quick_xml::escape::escape(value);
        match element {
            Element::Text(range) => body.replace_range(range, &escaped),
            Element::Empty { range, name } => {
                body.replace_range(range, &format!("<{name}>{escaped}</{name}>"))
            }
        }
        Ok(())
    }

    /// The body as JSON, on an API that answers in JSON.
    pub fn body_mut(&mut self) -> Result<&mut Value> {
        self.json
            .as_mut()
            .ok_or_else(|| refused("the response body is not JSON"))
    }
}

impl ResponseView for AwsResponse {
    fn read(_request: &InterceptedRequest, response: InterceptedResponse) -> Result<Self> {
        let json = serde_json::from_str(&response.body)
            .ok()
            .filter(Value::is_object);
        Ok(Self { response, json })
    }

    fn write(self) -> Result<InterceptedResponse> {
        let mut response = self.response;
        if let Some(json) = self.json {
            if json_body(Some(&response.body)) != json {
                response.body = serde_json::to_string(&json).map_err(|error| {
                    refused(format!("the changed body did not serialize: {error}"))
                })?;
            }
        }
        Ok(response)
    }
}

enum Element {
    /// The text between an element's start and end tags.
    Text(Range<usize>),
    /// An element with no content, such as `<Status/>`.
    Empty { range: Range<usize>, name: String },
}

/// The first element at `path` in `xml`, or `None` when there is none.
fn find_element(xml: &str, path: &str) -> std::result::Result<Option<Element>, quick_xml::Error> {
    let wanted: Vec<&str> = path.split('/').collect();
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut content_start = None;
    loop {
        let before = reader.buffer_position() as usize;
        let is_wanted = |stack: &[Vec<u8>]| {
            stack.len() == wanted.len()
                && stack
                    .iter()
                    .zip(&wanted)
                    .all(|(name, want)| name == want.as_bytes())
        };
        match reader.read_event()? {
            Event::Start(start) => {
                stack.push(start.name().as_ref().to_vec());
                if content_start.is_none() && is_wanted(&stack) {
                    content_start = Some((stack.len(), reader.buffer_position() as usize));
                }
            }
            Event::End(_) => {
                if let Some((depth, start)) = content_start {
                    if depth == stack.len() {
                        return Ok(Some(Element::Text(start..before)));
                    }
                }
                stack.pop();
            }
            Event::Empty(empty) => {
                stack.push(empty.name().as_ref().to_vec());
                if content_start.is_none() && is_wanted(&stack) {
                    let name = String::from_utf8_lossy(empty.name().as_ref()).into_owned();
                    return Ok(Some(Element::Empty {
                        range: before..reader.buffer_position() as usize,
                        name,
                    }));
                }
                stack.pop();
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
    }
}

// ---- Google Cloud ----

/// A Google Cloud call, as [`GcpInterceptor::matches`] sees it.
pub struct GcpCall {
    service: String,
    method: String,
    path: String,
}

impl GcpCall {
    /// Alien's key for the service, such as `cloudsql` or `compute`.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The HTTP method.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The URL's path, such as `/v1/projects/p/instances`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Whether the call has this method and its path ends with `route`, where `*` stands for one
    /// segment: `matches_route("POST", "projects/*/instances")` matches a Cloud SQL insert.
    pub fn matches_route(&self, method: &str, route: &str) -> bool {
        if !self.method.eq_ignore_ascii_case(method) {
            return false;
        }
        let segments: Vec<&str> = self.path.trim_matches('/').split('/').collect();
        let route: Vec<&str> = route.trim_matches('/').split('/').collect();
        segments.len() >= route.len()
            && segments[segments.len() - route.len()..]
                .iter()
                .zip(&route)
                .all(|(segment, want)| *want == "*" || segment == want)
    }
}

impl CallView for GcpCall {
    fn read(request: &InterceptedRequest) -> Result<Self> {
        let http = Http::read(request)?;
        Ok(Self {
            service: request.service.clone(),
            path: http.url.path().to_string(),
            method: http.method,
        })
    }
}

/// A Google Cloud request.
pub struct GcpRequest {
    request: InterceptedRequest,
    http: Http,
    body: Value,
}

http_accessors!(GcpRequest);

impl GcpRequest {
    /// The body as JSON. A request with no body reads as `null`.
    pub fn body_mut(&mut self) -> &mut Value {
        &mut self.body
    }
}

impl RequestView for GcpRequest {
    fn read(request: InterceptedRequest) -> Result<Self> {
        Ok(Self {
            http: Http::read(&request)?,
            body: json_body(request.body.as_deref()),
            request,
        })
    }

    fn write(self) -> Result<InterceptedRequest> {
        let body = json_text(&self.body, self.request.body.clone())?;
        Ok(self.http.write(self.request, body))
    }
}

/// A Google Cloud or Azure response, its body as JSON.
pub struct JsonResponse {
    response: InterceptedResponse,
    body: Value,
}

/// A Google Cloud response.
pub type GcpResponse = JsonResponse;

/// An Azure Resource Manager response.
pub type AzureResponse = JsonResponse;

impl JsonResponse {
    /// The HTTP status.
    pub fn status(&self) -> u16 {
        self.response.status
    }

    /// Replaces the HTTP status, such as turning an error into a success.
    pub fn set_status(&mut self, status: u16) {
        self.response.status = status;
    }

    /// The headers, in order.
    pub fn headers_mut(&mut self) -> &mut Vec<(String, String)> {
        &mut self.response.headers
    }

    /// The body as JSON. Text that isn't JSON, such as a proxy's error page, reads as a string.
    pub fn body_mut(&mut self) -> &mut Value {
        &mut self.body
    }
}

impl ResponseView for JsonResponse {
    fn read(_request: &InterceptedRequest, response: InterceptedResponse) -> Result<Self> {
        Ok(Self {
            body: json_body(Some(&response.body)),
            response,
        })
    }

    fn write(self) -> Result<InterceptedResponse> {
        let mut response = self.response;
        response.body = json_text(&self.body, Some(response.body))?.unwrap_or_default();
        Ok(response)
    }
}

// ---- Azure ----

/// An Azure Resource Manager call, as [`AzureInterceptor::matches`] sees it.
pub struct AzureCall {
    provider: String,
    resource_type: String,
    method: String,
    tenant_id: String,
}

impl AzureCall {
    /// The resource provider, such as `Microsoft.DBforPostgreSQL`. Empty for a path outside any
    /// provider, such as a resource group's.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The resource type under the provider, such as `flexibleServers`, or
    /// `flexibleServers/databases` for a child resource.
    pub fn resource_type(&self) -> &str {
        &self.resource_type
    }

    /// The HTTP method.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The Azure tenant the call is made in.
    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }
}

impl CallView for AzureCall {
    fn read(request: &InterceptedRequest) -> Result<Self> {
        let http = Http::read(request)?;
        let tenant_id = request
            .tenant_id
            .clone()
            .ok_or_else(|| refused("Alien sent an Azure call without its tenant"))?;
        let segments: Vec<&str> = http
            .url
            .path_segments()
            .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
            .unwrap_or_default();
        let (provider, resource_type) = match segments
            .iter()
            .rposition(|segment| segment.eq_ignore_ascii_case("providers"))
        {
            Some(at) => (
                segments
                    .get(at + 1)
                    .copied()
                    .unwrap_or_default()
                    .to_string(),
                segments
                    .iter()
                    .skip(at + 2)
                    .step_by(2)
                    .copied()
                    .collect::<Vec<_>>()
                    .join("/"),
            ),
            None => (String::new(), String::new()),
        };
        Ok(Self {
            provider,
            resource_type,
            method: http.method,
            tenant_id,
        })
    }
}

/// An Azure Resource Manager request.
pub struct AzureRequest {
    request: InterceptedRequest,
    http: Http,
    call: AzureCall,
    body: Value,
}

http_accessors!(AzureRequest);

impl AzureRequest {
    /// The call this interceptor matched.
    pub fn call(&self) -> &AzureCall {
        &self.call
    }

    /// The `api-version` the call uses.
    pub fn api_version(&self) -> Option<String> {
        self.http
            .url
            .query_pairs()
            .find(|(key, _)| key == "api-version")
            .map(|(_, value)| value.into_owned())
    }

    /// The body as JSON. A request with no body reads as `null`.
    pub fn body_mut(&mut self) -> &mut Value {
        &mut self.body
    }
}

impl RequestView for AzureRequest {
    fn read(request: InterceptedRequest) -> Result<Self> {
        Ok(Self {
            http: Http::read(&request)?,
            call: AzureCall::read(&request)?,
            body: json_body(request.body.as_deref()),
            request,
        })
    }

    fn write(self) -> Result<InterceptedRequest> {
        let body = json_text(&self.body, self.request.body.clone())?;
        Ok(self.http.write(self.request, body))
    }
}

// ---- Kubernetes ----

/// A Kubernetes call, as [`KubernetesInterceptor::matches`] sees it.
#[derive(Clone)]
pub struct KubernetesCall {
    kind: String,
    verb: String,
    namespace: Option<String>,
}

impl KubernetesCall {
    /// The kind of the object the call sends, such as `Pod`. Empty for a call that sends none,
    /// such as a get or a delete.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The verb: `create`, `update`, `patch`, `delete`, `get` or `list`.
    pub fn verb(&self) -> &str {
        &self.verb
    }

    /// The namespace in the path, for a namespaced object.
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }
}

impl CallView for KubernetesCall {
    fn read(request: &InterceptedRequest) -> Result<Self> {
        let http = Http::read(request)?;
        let segments: Vec<&str> = http
            .url
            .path_segments()
            .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
            .unwrap_or_default();
        // A namespaced path reads `namespaces/<namespace>/<resource>`; `namespaces/<name>` alone
        // is the namespace object itself.
        let namespace = segments
            .iter()
            .position(|segment| *segment == "namespaces")
            .filter(|at| segments.len() > at + 2)
            .map(|at| segments[at + 1].to_string());
        // `/api/v1/...` or `/apis/<group>/<version>/...`, then the namespace pair, the resource
        // and, for one object, its name.
        let prefix = match segments.first() {
            Some(&"apis") => 3,
            _ => 2,
        };
        let rest = segments.len().saturating_sub(prefix);
        let names_one_object = match namespace {
            Some(_) => rest >= 4,
            None => rest >= 2,
        };
        let verb = match http.method.to_ascii_uppercase().as_str() {
            "POST" => "create",
            "PUT" => "update",
            "PATCH" => "patch",
            "DELETE" => "delete",
            _ if names_one_object => "get",
            _ => "list",
        };
        let kind = json_body(request.body.as_deref())
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok(Self {
            kind,
            verb: verb.to_string(),
            namespace,
        })
    }
}

/// A Kubernetes object: the one Alien sends, or the one the API server returns.
pub struct KubernetesObject {
    call: KubernetesCall,
    body: Value,
    from: ObjectSource,
}

/// Where a [`KubernetesObject`] came from, which is where it goes back.
enum ObjectSource {
    Request(Box<(InterceptedRequest, Http)>),
    Response(InterceptedResponse),
}

impl KubernetesObject {
    /// The object's kind, such as `Pod`.
    pub fn kind(&self) -> &str {
        self.body
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or(&self.call.kind)
    }

    /// The call's verb, such as `create`.
    pub fn verb(&self) -> &str {
        &self.call.verb
    }

    /// The call's namespace, for a namespaced object.
    pub fn namespace(&self) -> Option<&str> {
        self.call.namespace()
    }

    /// The object as JSON.
    pub fn body_mut(&mut self) -> &mut Value {
        &mut self.body
    }
}

impl RequestView for KubernetesObject {
    fn read(request: InterceptedRequest) -> Result<Self> {
        Ok(Self {
            call: KubernetesCall::read(&request)?,
            body: json_body(request.body.as_deref()),
            from: ObjectSource::Request(Box::new((request.clone(), Http::read(&request)?))),
        })
    }

    fn write(self) -> Result<InterceptedRequest> {
        let ObjectSource::Request(source) = self.from else {
            unreachable!("only a request reads as a request object");
        };
        let (request, http) = *source;
        let body = json_text(&self.body, request.body.clone())?;
        Ok(http.write(request, body))
    }
}

impl ResponseView for KubernetesObject {
    fn read(request: &InterceptedRequest, response: InterceptedResponse) -> Result<Self> {
        Ok(Self {
            call: KubernetesCall::read(request)?,
            body: json_body(Some(&response.body)),
            from: ObjectSource::Response(response),
        })
    }

    fn write(self) -> Result<InterceptedResponse> {
        let ObjectSource::Response(mut response) = self.from else {
            unreachable!("only a response reads as a response object");
        };
        response.body = json_text(&self.body, Some(response.body))?.unwrap_or_default();
        Ok(response)
    }
}
