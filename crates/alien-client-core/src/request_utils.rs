use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use backon::{ExponentialBuilder, Retryable};
use serde::de::DeserializeOwned;
use std::time::Duration;

/// Extract request body as string from a reqwest::Request if available
fn extract_request_body_string(request: &reqwest::Request) -> Option<String> {
    request
        .body()
        .and_then(|body| body.as_bytes())
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

/// Helper to build request and extract body before sending
fn build_and_extract_body(
    builder: reqwest::RequestBuilder,
) -> Result<(reqwest::Client, reqwest::Request, Option<String>)> {
    let (client, req_result) = builder.build_split();
    let request = req_result
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to build request".to_string(),
        })?;

    let body_string = extract_request_body_string(&request);
    Ok((client, request, body_string))
}

/// Handle an HTTP response by checking status and parsing JSON on success
pub async fn handle_json_response<T: DeserializeOwned>(
    response: reqwest::Response,
    request_body: Option<String>,
) -> Result<T> {
    let status = response.status();
    let url = response.url().to_string();
    let response_text =
        response
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to read response body".to_string(),
            })?;

    if !status.is_success() {
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!(
                "Request failed with HTTP {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown error")
            ),
            url,
            http_status: status.as_u16(),
            http_request_text: request_body,
            http_response_text: Some(response_text),
        }));
    }

    // Parse the JSON response using serde_path_to_error for better error messages
    let jd = &mut serde_json::Deserializer::from_str(&response_text);
    let parsed_response: T = serde_path_to_error::deserialize(jd).map_err(|err| {
        AlienError::new(ErrorData::HttpResponseError {
            message: format!(
                "Invalid JSON response at field '{}': {}",
                err.path(),
                err.inner()
            ),
            url,
            http_status: status.as_u16(),
            http_request_text: request_body,
            http_response_text: Some(response_text),
        })
    })?;

    Ok(parsed_response)
}

/// Handle an HTTP response by checking status and parsing XML on success
pub async fn handle_xml_response<T: DeserializeOwned>(
    response: reqwest::Response,
    request_body: Option<String>,
) -> Result<T> {
    let status = response.status();
    let url = response.url().to_string();
    let response_text =
        response
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to read response body".to_string(),
            })?;

    if !status.is_success() {
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!(
                "Request failed with HTTP {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown error")
            ),
            url,
            http_status: status.as_u16(),
            http_request_text: request_body,
            http_response_text: Some(response_text),
        }));
    }

    // Parse the XML response using serde_path_to_error for better error messages
    let mut xml_deserializer = quick_xml::de::Deserializer::from_str(&response_text);
    let parsed_response: T =
        serde_path_to_error::deserialize(&mut xml_deserializer).map_err(|err| {
            AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Invalid XML response at field '{}': {}",
                    err.path(),
                    err.inner()
                ),
                url,
                http_status: status.as_u16(),
                http_request_text: request_body,
                http_response_text: Some(response_text),
            })
        })?;

    Ok(parsed_response)
}

/// Handle an HTTP response by checking status without parsing the body
pub async fn handle_no_response(
    response: reqwest::Response,
    request_body: Option<String>,
) -> Result<()> {
    let status = response.status();
    let url = response.url().to_string();

    if !status.is_success() {
        let response_text =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Failed to read error response body".to_string(),
                })?;
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!(
                "Request failed with HTTP {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown error")
            ),
            url,
            http_status: status.as_u16(),
            http_request_text: request_body,
            http_response_text: Some(response_text),
        }));
    }

    Ok(())
}

/// Extension trait for `reqwest::RequestBuilder` to add JSON and XML response handling
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait RequestBuilderExt {
    /// Enable retries with an exponential back-off strategy.
    ///
    /// Example:
    /// ```ignore
    /// let obj: MyType = client
    ///     .get("https://api.example.com/obj")
    ///     .with_retry()                // <- enable retries
    ///     .send_json()                // <- deserialize JSON body
    ///     .await?;
    /// ```
    fn with_retry(self) -> RetriableRequestBuilder;

    /// Send the request and parse the response as JSON
    async fn send_json<T: DeserializeOwned + 'static>(self) -> Result<T>;

    /// Send the request and parse the response as XML
    async fn send_xml<T: DeserializeOwned + 'static>(self) -> Result<T>;

    /// Send the request without parsing the response body
    async fn send_no_response(self) -> Result<()>;

    /// Send the request and return the raw response for custom handling
    async fn send_raw(self) -> Result<reqwest::Response>;
}

/// A `reqwest::RequestBuilder` wrapper that automatically retries failed
/// requests using an exponential back-off strategy powered by the `backon`
/// crate. Use [`RequestBuilderExt::with_retry`] to construct one.
pub struct RetriableRequestBuilder {
    inner: reqwest::RequestBuilder,
    backoff: ExponentialBuilder,
}

impl RetriableRequestBuilder {
    /// Overrides the default back-off settings.
    pub fn backoff(mut self, backoff: ExponentialBuilder) -> Self {
        self.backoff = backoff;
        self
    }

    /// Determine if a given error is retry-able using the retryable field.
    fn is_retryable_error(e: &AlienError<ErrorData>) -> bool {
        e.retryable
    }

    /// Creates a default exponential back-off (max 3 attempts, up to 20s).
    fn default_backoff() -> ExponentialBuilder {
        ExponentialBuilder::default()
            .with_max_times(3)
            .with_max_delay(Duration::from_secs(20))
            .with_jitter()
    }

    /// Execute the request, applying retries, and parse the body as JSON.
    pub async fn send_json<T: DeserializeOwned + Send + 'static>(self) -> Result<T> {
        let backoff = self.backoff;
        let builder = self.inner;

        let retryable = move || {
            let attempt_builder = builder.try_clone();
            async move {
                let attempt_builder = attempt_builder.ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: "Request retry preparation failed".into(),
                    })
                })?;

                let (client, request, body_string) = build_and_extract_body(attempt_builder)?;
                let new_builder = reqwest::RequestBuilder::from_parts(client, request);

                #[cfg(target_arch = "wasm32")]
                {
                    let resp = new_builder.send().await.into_alien_error().context(
                        ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        },
                    )?;
                    handle_json_response(resp, body_string).await
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let resp = new_builder.send().await.into_alien_error().context(
                        ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        },
                    )?;
                    handle_json_response(resp, body_string).await
                }
            }
        };

        retryable
            .retry(backoff)
            .when(Self::is_retryable_error)
            .await
    }

    /// Execute the request, applying retries, and parse the body as XML.
    pub async fn send_xml<T: DeserializeOwned + Send + 'static>(self) -> Result<T> {
        let backoff = self.backoff;
        let builder = self.inner;

        let retryable = move || {
            let attempt_builder = builder.try_clone();
            async move {
                let attempt_builder = attempt_builder.ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: "Request retry preparation failed".into(),
                    })
                })?;

                let (client, request, body_string) = build_and_extract_body(attempt_builder)?;
                let new_builder = reqwest::RequestBuilder::from_parts(client, request);

                #[cfg(target_arch = "wasm32")]
                {
                    let resp = new_builder.send().await.into_alien_error().context(
                        ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        },
                    )?;
                    handle_xml_response(resp, body_string).await
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let resp = new_builder.send().await.into_alien_error().context(
                        ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        },
                    )?;
                    handle_xml_response(resp, body_string).await
                }
            }
        };

        retryable
            .retry(backoff)
            .when(Self::is_retryable_error)
            .await
    }

    /// Execute the request, applying retries, without parsing the response body.
    pub async fn send_no_response(self) -> Result<()> {
        let backoff = self.backoff;
        let builder = self.inner;

        let retryable = move || {
            let attempt_builder = builder.try_clone();
            async move {
                let attempt_builder = attempt_builder.ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: "Request retry preparation failed".into(),
                    })
                })?;

                let (client, request, body_string) = build_and_extract_body(attempt_builder)?;
                let new_builder = reqwest::RequestBuilder::from_parts(client, request);

                #[cfg(target_arch = "wasm32")]
                {
                    let resp = new_builder.send().await.into_alien_error().context(
                        ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        },
                    )?;
                    handle_no_response(resp, body_string).await
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let resp = new_builder.send().await.into_alien_error().context(
                        ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        },
                    )?;
                    handle_no_response(resp, body_string).await
                }
            }
        };

        retryable
            .retry(backoff)
            .when(Self::is_retryable_error)
            .await
    }

    /// Execute the request, applying retries, and return the raw response
    pub async fn send_raw(self) -> Result<reqwest::Response> {
        let backoff = self.backoff;
        let builder = self.inner;

        let retryable = move || {
            let attempt_builder = builder.try_clone();
            async move {
                let attempt_builder = attempt_builder.ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: "Request retry preparation failed".into(),
                    })
                })?;

                let (client, request, _body_string) = build_and_extract_body(attempt_builder)?;
                let new_builder = reqwest::RequestBuilder::from_parts(client, request);

                #[cfg(target_arch = "wasm32")]
                {
                    new_builder.send().await.into_alien_error().context(
                        ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        },
                    )
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    new_builder.send().await.into_alien_error().context(
                        ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        },
                    )
                }
            }
        };

        retryable
            .retry(backoff)
            .when(Self::is_retryable_error)
            .await
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RequestBuilderExt for reqwest::RequestBuilder {
    fn with_retry(self) -> RetriableRequestBuilder {
        RetriableRequestBuilder {
            inner: self,
            backoff: RetriableRequestBuilder::default_backoff(),
        }
    }

    async fn send_json<T: DeserializeOwned + 'static>(self) -> Result<T> {
        let (client, request, body_string) = build_and_extract_body(self)?;
        let builder = reqwest::RequestBuilder::from_parts(client, request);

        #[cfg(target_arch = "wasm32")]
        {
            let resp =
                builder
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Network error during HTTP request".to_string(),
                    })?;
            handle_json_response(resp, body_string).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let resp =
                builder
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Network error during HTTP request".to_string(),
                    })?;
            handle_json_response(resp, body_string).await
        }
    }

    async fn send_xml<T: DeserializeOwned + 'static>(self) -> Result<T> {
        let (client, request, body_string) = build_and_extract_body(self)?;
        let builder = reqwest::RequestBuilder::from_parts(client, request);

        #[cfg(target_arch = "wasm32")]
        {
            let resp =
                builder
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Network error during HTTP request".to_string(),
                    })?;
            handle_xml_response(resp, body_string).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let resp =
                builder
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Network error during HTTP request".to_string(),
                    })?;
            handle_xml_response(resp, body_string).await
        }
    }

    async fn send_no_response(self) -> Result<()> {
        let (client, request, body_string) = build_and_extract_body(self)?;
        let builder = reqwest::RequestBuilder::from_parts(client, request);

        #[cfg(target_arch = "wasm32")]
        {
            let resp =
                builder
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Network error during HTTP request".to_string(),
                    })?;
            handle_no_response(resp, body_string).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let resp =
                builder
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Network error during HTTP request".to_string(),
                    })?;
            handle_no_response(resp, body_string).await
        }
    }

    async fn send_raw(self) -> Result<reqwest::Response> {
        let (client, request, _body_string) = build_and_extract_body(self)?;
        let builder = reqwest::RequestBuilder::from_parts(client, request);

        #[cfg(target_arch = "wasm32")]
        {
            builder
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Network error during HTTP request".to_string(),
                })
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            builder
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Network error during HTTP request".to_string(),
                })
        }
    }
}
