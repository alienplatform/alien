use super::*;

mod operations;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Elbv2Api: Send + Sync + std::fmt::Debug {
    // Load Balancer Operations
    async fn create_load_balancer(
        &self,
        request: CreateLoadBalancerRequest,
    ) -> Result<CreateLoadBalancerResponse>;
    async fn describe_load_balancers(
        &self,
        request: DescribeLoadBalancersRequest,
    ) -> Result<DescribeLoadBalancersResponse>;
    async fn modify_load_balancer_attributes(
        &self,
        request: ModifyLoadBalancerAttributesRequest,
    ) -> Result<ModifyLoadBalancerAttributesResponse>;
    async fn delete_load_balancer(&self, load_balancer_arn: &str) -> Result<()>;

    // Target Group Operations
    async fn create_target_group(
        &self,
        request: CreateTargetGroupRequest,
    ) -> Result<CreateTargetGroupResponse>;
    async fn describe_target_groups(
        &self,
        request: DescribeTargetGroupsRequest,
    ) -> Result<DescribeTargetGroupsResponse>;
    async fn modify_target_group(
        &self,
        request: ModifyTargetGroupRequest,
    ) -> Result<ModifyTargetGroupResponse>;
    async fn modify_target_group_attributes(
        &self,
        request: ModifyTargetGroupAttributesRequest,
    ) -> Result<ModifyTargetGroupAttributesResponse>;
    async fn delete_target_group(&self, target_group_arn: &str) -> Result<()>;

    // Target Operations
    async fn register_targets(&self, request: RegisterTargetsRequest) -> Result<()>;
    async fn deregister_targets(&self, request: DeregisterTargetsRequest) -> Result<()>;
    async fn describe_target_health(
        &self,
        request: DescribeTargetHealthRequest,
    ) -> Result<DescribeTargetHealthResponse>;

    // Listener Operations
    async fn create_listener(
        &self,
        request: CreateListenerRequest,
    ) -> Result<CreateListenerResponse>;
    async fn describe_listeners(
        &self,
        request: DescribeListenersRequest,
    ) -> Result<DescribeListenersResponse>;
    async fn modify_listener(
        &self,
        request: ModifyListenerRequest,
    ) -> Result<ModifyListenerResponse>;
    async fn delete_listener(&self, listener_arn: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// ELBv2 Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Elbv2Client {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl Elbv2Client {
    /// Create a new ELBv2 client.
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "elasticloadbalancing".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self
            .credentials
            .get_service_endpoint_option("elasticloadbalancing")
        {
            override_url.to_string()
        } else {
            format!(
                "https://elasticloadbalancing.{}.amazonaws.com",
                self.credentials.region()
            )
        }
    }

    fn get_host(&self) -> String {
        format!(
            "elasticloadbalancing.{}.amazonaws.com",
            self.credentials.region()
        )
    }

    // ------------------------- Internal Helpers -------------------------

    async fn send_form<T: DeserializeOwned + Send + 'static>(
        &self,
        form_data: HashMap<String, String>,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let url = self.get_base_url();

        let form_body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form_data.iter())
            .finish();

        let builder = self
            .client
            .request(Method::POST, &url)
            .host(&self.get_host())
            .content_type_form()
            .content_sha256(&form_body)
            .body(form_body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_xml(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, Some(&form_body))
    }

    async fn send_form_no_body(
        &self,
        form_data: HashMap<String, String>,
        operation: &str,
        resource: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let url = self.get_base_url();

        let form_body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form_data.iter())
            .finish();

        let builder = self
            .client
            .request(Method::POST, &url)
            .host(&self.get_host())
            .content_type_form()
            .content_sha256(&form_body)
            .body(form_body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(result, operation, resource, Some(&form_body))
    }

    fn map_result<T>(
        result: Result<T>,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Result<T> {
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(ref text),
                    ..
                }) = &e.error
                {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(mapped) =
                        Self::map_elbv2_error(status, text, operation, resource, request_body)
                    {
                        Err(e.context(mapped))
                    } else {
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_elbv2_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        if body.trim().is_empty() {
            return match status {
                StatusCode::NOT_FOUND => Some(ErrorData::RemoteResourceNotFound {
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                }),
                StatusCode::CONFLICT => Some(ErrorData::RemoteResourceConflict {
                    message: "Resource conflict".into(),
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                }),
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                    Some(ErrorData::RemoteAccessDenied {
                        resource_type: "LoadBalancer".into(),
                        resource_name: resource.into(),
                    })
                }
                StatusCode::TOO_MANY_REQUESTS => Some(ErrorData::RateLimitExceeded {
                    message: "Too many requests".into(),
                }),
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => Some(ErrorData::RemoteServiceUnavailable {
                    message: "Service unavailable".into(),
                }),
                _ => None,
            };
        }

        let parsed: std::result::Result<Elbv2ErrorResponse, _> = quick_xml::de::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => (e.error.code, e.error.message),
            Err(_) => {
                return None;
            }
        };

        // Map ELBv2 error codes
        Some(match code.as_str() {
            // Access / Auth errors
            "AccessDenied" | "UnauthorizedAccess" => ErrorData::RemoteAccessDenied {
                resource_type: "LoadBalancer".into(),
                resource_name: resource.into(),
            },
            // Throttling
            "Throttling" | "RequestLimitExceeded" => ErrorData::RateLimitExceeded { message },
            // Service unavailable
            "ServiceUnavailable" | "InternalFailure" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Load balancer not found
            "LoadBalancerNotFound" | "LoadBalancerNotFoundException" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                }
            }
            // Target group not found
            "TargetGroupNotFound" | "TargetGroupNotFoundException" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "TargetGroup".into(),
                    resource_name: resource.into(),
                }
            }
            // Listener not found
            "ListenerNotFound" | "ListenerNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "Listener".into(),
                resource_name: resource.into(),
            },
            // Already exists
            "DuplicateLoadBalancerName" | "DuplicateLoadBalancerNameException" => {
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                }
            }
            "DuplicateTargetGroupName" | "DuplicateTargetGroupNameException" => {
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "TargetGroup".into(),
                    resource_name: resource.into(),
                }
            }
            "DuplicateListener" | "DuplicateListenerException" => {
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "Listener".into(),
                    resource_name: resource.into(),
                }
            }
            // Resource in use
            "ResourceInUse" | "ResourceInUseException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "LoadBalancer".into(),
                resource_name: resource.into(),
            },
            // Limit exceeded
            "TooManyLoadBalancers"
            | "TooManyTargetGroups"
            | "TooManyListeners"
            | "TooManyTargets"
            | "TooManyRegistrationsForTargetId"
            | "TooManyTags" => ErrorData::QuotaExceeded { message },
            // Invalid input
            "InvalidConfigurationRequest" | "ValidationError" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
            "InvalidTarget" | "InvalidTargetException" => ErrorData::InvalidInput {
                message,
                field_name: Some("target".into()),
            },
            "InvalidSubnet" | "SubnetNotFound" => ErrorData::InvalidInput {
                message,
                field_name: Some("subnet".into()),
            },
            "InvalidSecurityGroup" | "SecurityGroupNotFound" => ErrorData::InvalidInput {
                message,
                field_name: Some("security_group".into()),
            },
            // Default fallback
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("ELBv2 operation failed: {}", message),
                    url: "elasticloadbalancing.amazonaws.com".into(),
                    http_status: status.as_u16(),
                    http_response_text: Some(body.into()),
                    http_request_text: request_body.map(|s| s.to_string()),
                },
            },
        })
    }

    fn add_tags(form_data: &mut HashMap<String, String>, tags: &[ElbTag]) {
        for (i, tag) in tags.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(format!("Tags.member.{}.Key", idx), tag.key.clone());
            form_data.insert(format!("Tags.member.{}.Value", idx), tag.value.clone());
        }
    }
}
