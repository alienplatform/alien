use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use tracing::{debug, info, warn};

use crate::core::EnvironmentVariableBuilder;

use crate::core::split_certificate_chain;
use crate::core::ResourceController;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::worker::readiness_probe::{
    run_readiness_probe_with_dns_override, ReadinessProbeDnsOverride, READINESS_PROBE_MAX_ATTEMPTS,
};
use alien_core::{
    standard_resource_tags, AwsClientConfig, AwsLambdaWorkerHeartbeatData, CertificateStatus,
    DnsRecordStatus, HeartbeatBackend, Ingress, Network, NetworkSettings, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceDefinition, ResourceHeartbeat, ResourceHeartbeatData,
    ResourceOutputs, ResourceRef, ResourceStatus, Worker, WorkerHeartbeatData, WorkerOutputs,
    WorkloadHeartbeatStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_macros::controller;
use aws_sdk_acm::{
    error::{ProvideErrorMetadata as AcmProvideErrorMetadata, SdkError as AcmSdkError},
    operation::import_certificate::{ImportCertificateInput, ImportCertificateOutput},
    primitives::Blob,
    types::Tag,
    Client as AcmClient,
};
use aws_sdk_apigatewayv2::{
    error::{
        ProvideErrorMetadata as ApiGatewayV2ProvideErrorMetadata, SdkError as ApiGatewayV2SdkError,
    },
    operation::{
        create_api::{CreateApiInput, CreateApiOutput},
        create_api_mapping::{CreateApiMappingInput, CreateApiMappingOutput},
        create_domain_name::{CreateDomainNameInput, CreateDomainNameOutput},
        create_integration::{CreateIntegrationInput, CreateIntegrationOutput},
        create_route::{CreateRouteInput, CreateRouteOutput},
        create_stage::{CreateStageInput, CreateStageOutput},
    },
    types::{DomainNameConfiguration, EndpointType, IntegrationType, ProtocolType, SecurityPolicy},
    Client as ApiGatewayV2Client,
};
use aws_sdk_ec2::{
    error::{ProvideErrorMetadata as Ec2ProvideErrorMetadata, SdkError as Ec2SdkError},
    operation::describe_network_interfaces::{
        DescribeNetworkInterfacesInput, DescribeNetworkInterfacesOutput,
    },
    types::Filter,
    Client as Ec2Client,
};
use aws_sdk_eventbridge::{
    error::{
        ProvideErrorMetadata as EventBridgeProvideErrorMetadata, SdkError as EventBridgeSdkError,
    },
    operation::{
        put_rule::{PutRuleInput, PutRuleOutput},
        put_targets::PutTargetsInput,
    },
    types::{RuleState, Tag as EventBridgeTag, Target as EventBridgeTarget},
    Client as EventBridgeClient,
};
use aws_sdk_lambda::{
    error::{ProvideErrorMetadata as LambdaProvideErrorMetadata, SdkError as LambdaSdkError},
    operation::{
        add_permission::{AddPermissionInput, AddPermissionOutput},
        create_event_source_mapping::{
            CreateEventSourceMappingInput, CreateEventSourceMappingOutput,
        },
        create_function::{CreateFunctionInput, CreateFunctionOutput},
        delete_event_source_mapping::DeleteEventSourceMappingOutput,
        get_function_configuration::GetFunctionConfigurationOutput,
        list_event_source_mappings::{ListEventSourceMappingsInput, ListEventSourceMappingsOutput},
        update_function_code::{UpdateFunctionCodeInput, UpdateFunctionCodeOutput},
        update_function_configuration::{
            UpdateFunctionConfigurationInput, UpdateFunctionConfigurationOutput,
        },
    },
    types::{
        Architecture as LambdaArchitecture, Environment, FunctionCode,
        LastUpdateStatus as LambdaLastUpdateStatus, PackageType, State as LambdaState, VpcConfig,
    },
    Client as LambdaClient,
};
use aws_sdk_s3::{
    error::{ProvideErrorMetadata as S3ProvideErrorMetadata, SdkError as S3SdkError},
    operation::{
        get_bucket_notification_configuration::{
            GetBucketNotificationConfigurationError, GetBucketNotificationConfigurationOutput,
        },
        put_bucket_notification_configuration::PutBucketNotificationConfigurationOutput,
    },
    types::{Event as S3Event, LambdaFunctionConfiguration, NotificationConfiguration},
    Client as S3Client,
};
use chrono::Utc;

/// Generates the full, prefixed AWS resource name.
fn get_aws_worker_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

fn readiness_probe_dns_override(
    url: &str,
    fqdn: Option<&str>,
    load_balancer: Option<&LoadBalancerState>,
) -> Option<ReadinessProbeDnsOverride> {
    let fqdn = fqdn?;
    let endpoint = load_balancer?.endpoint.as_ref()?;
    let parsed = reqwest::Url::parse(url).ok()?;
    let url_host = parsed.host_str()?;

    if url_host != fqdn {
        return None;
    }

    Some(ReadinessProbeDnsOverride {
        host: fqdn.to_string(),
        target_dns_name: endpoint.dns_name.clone(),
        port: parsed.port_or_known_default().unwrap_or(443),
    })
}

async fn describe_ec2_network_interfaces(
    client: &Ec2Client,
    request: DescribeNetworkInterfacesInput,
) -> Result<DescribeNetworkInterfacesOutput> {
    match client
        .describe_network_interfaces()
        .set_next_token(request.next_token)
        .set_max_results(request.max_results)
        .set_dry_run(request.dry_run)
        .set_network_interface_ids(request.network_interface_ids)
        .set_filters(request.filters)
        .send()
        .await
    {
        Ok(output) => Ok(output),
        Err(error) => Err(map_ec2_error(
            error,
            "DescribeNetworkInterfaces",
            "NetworkInterface",
            "*",
        )),
    }
}

fn map_ec2_error<E>(
    error: Ec2SdkError<E>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> AlienError<ErrorData>
where
    E: Ec2ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    if let Some(service_error) = error.as_service_error() {
        match service_error.code() {
            Some("InvalidNetworkInterfaceID.NotFound") => {
                return AlienError::new(ErrorData::CloudResourceNotFound {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                });
            }
            Some(code @ ("DependencyViolation" | "ResourceInUse")) => {
                return AlienError::new(ErrorData::CloudResourceConflict {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                    message: format!(
                        "{operation} reported {code}: {}",
                        service_error.message().unwrap_or(code)
                    ),
                });
            }
            _ => {}
        }
    }

    error
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("EC2 {operation} API failed for {resource_type} '{resource_name}'"),
            resource_id: None,
        })
}

fn eventbridge_tags(prefix: &str, resource_id: &str) -> Result<Vec<EventBridgeTag>> {
    standard_resource_tags(prefix, resource_id)
        .into_iter()
        .map(|(key, value)| {
            EventBridgeTag::builder()
                .key(key)
                .value(value)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Invalid EventBridge tag".to_string(),
                    resource_id: Some(resource_id.to_string()),
                })
        })
        .collect()
}

async fn create_lambda_function(
    client: &LambdaClient,
    request: CreateFunctionInput,
) -> Result<CreateFunctionOutput> {
    let function_name = request.function_name.clone().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "CreateFunction request did not include functionName".to_string(),
            resource_id: None,
        })
    })?;

    lambda_result(
        client
            .create_function()
            .set_function_name(request.function_name)
            .set_runtime(request.runtime)
            .set_role(request.role)
            .set_handler(request.handler)
            .set_code(request.code)
            .set_description(request.description)
            .set_timeout(request.timeout)
            .set_memory_size(request.memory_size)
            .set_publish(request.publish)
            .set_vpc_config(request.vpc_config)
            .set_package_type(request.package_type)
            .set_dead_letter_config(request.dead_letter_config)
            .set_environment(request.environment)
            .set_kms_key_arn(request.kms_key_arn)
            .set_tracing_config(request.tracing_config)
            .set_tags(request.tags)
            .set_layers(request.layers)
            .set_file_system_configs(request.file_system_configs)
            .set_image_config(request.image_config)
            .set_code_signing_config_arn(request.code_signing_config_arn)
            .set_architectures(request.architectures)
            .set_ephemeral_storage(request.ephemeral_storage)
            .set_snap_start(request.snap_start)
            .set_logging_config(request.logging_config)
            .set_capacity_provider_config(request.capacity_provider_config)
            .set_publish_to(request.publish_to)
            .set_durable_config(request.durable_config)
            .set_tenancy_config(request.tenancy_config)
            .send()
            .await,
        "CreateFunction",
        "LambdaFunction",
        &function_name,
    )
}

async fn add_lambda_permission(
    client: &LambdaClient,
    request: AddPermissionInput,
) -> Result<AddPermissionOutput> {
    let function_name = request.function_name.clone().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "AddPermission request did not include functionName".to_string(),
            resource_id: None,
        })
    })?;

    lambda_result(
        client
            .add_permission()
            .set_function_name(request.function_name)
            .set_statement_id(request.statement_id)
            .set_action(request.action)
            .set_principal(request.principal)
            .set_source_arn(request.source_arn)
            .set_source_account(request.source_account)
            .set_event_source_token(request.event_source_token)
            .set_qualifier(request.qualifier)
            .set_revision_id(request.revision_id)
            .set_principal_org_id(request.principal_org_id)
            .set_function_url_auth_type(request.function_url_auth_type)
            .set_invoked_via_function_url(request.invoked_via_function_url)
            .send()
            .await,
        "AddPermission",
        "LambdaFunction",
        &function_name,
    )
}

async fn update_lambda_function_code(
    client: &LambdaClient,
    request: UpdateFunctionCodeInput,
) -> Result<UpdateFunctionCodeOutput> {
    let function_name = request.function_name.clone().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "UpdateFunctionCode request did not include functionName".to_string(),
            resource_id: None,
        })
    })?;

    lambda_result(
        client
            .update_function_code()
            .set_function_name(request.function_name)
            .set_zip_file(request.zip_file)
            .set_s3_bucket(request.s3_bucket)
            .set_s3_key(request.s3_key)
            .set_s3_object_version(request.s3_object_version)
            .set_image_uri(request.image_uri)
            .set_publish(request.publish)
            .set_dry_run(request.dry_run)
            .set_revision_id(request.revision_id)
            .set_architectures(request.architectures)
            .set_source_kms_key_arn(request.source_kms_key_arn)
            .set_publish_to(request.publish_to)
            .send()
            .await,
        "UpdateFunctionCode",
        "LambdaFunction",
        &function_name,
    )
}

async fn update_lambda_function_configuration(
    client: &LambdaClient,
    request: UpdateFunctionConfigurationInput,
) -> Result<UpdateFunctionConfigurationOutput> {
    let function_name = request.function_name.clone().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "UpdateFunctionConfiguration request did not include functionName".to_string(),
            resource_id: None,
        })
    })?;

    lambda_result(
        client
            .update_function_configuration()
            .set_function_name(request.function_name)
            .set_role(request.role)
            .set_handler(request.handler)
            .set_description(request.description)
            .set_timeout(request.timeout)
            .set_memory_size(request.memory_size)
            .set_vpc_config(request.vpc_config)
            .set_environment(request.environment)
            .set_runtime(request.runtime)
            .set_dead_letter_config(request.dead_letter_config)
            .set_kms_key_arn(request.kms_key_arn)
            .set_tracing_config(request.tracing_config)
            .set_revision_id(request.revision_id)
            .set_layers(request.layers)
            .set_file_system_configs(request.file_system_configs)
            .set_image_config(request.image_config)
            .set_ephemeral_storage(request.ephemeral_storage)
            .set_snap_start(request.snap_start)
            .set_logging_config(request.logging_config)
            .set_capacity_provider_config(request.capacity_provider_config)
            .set_durable_config(request.durable_config)
            .send()
            .await,
        "UpdateFunctionConfiguration",
        "LambdaFunction",
        &function_name,
    )
}

async fn get_lambda_function_configuration(
    client: &LambdaClient,
    function_name: &str,
    qualifier: Option<String>,
) -> Result<GetFunctionConfigurationOutput> {
    lambda_result(
        client
            .get_function_configuration()
            .function_name(function_name)
            .set_qualifier(qualifier)
            .send()
            .await,
        "GetFunctionConfiguration",
        "LambdaFunction",
        function_name,
    )
}

async fn delete_lambda_function(
    client: &LambdaClient,
    function_name: &str,
    qualifier: Option<String>,
) -> Result<()> {
    lambda_result(
        client
            .delete_function()
            .function_name(function_name)
            .set_qualifier(qualifier)
            .send()
            .await,
        "DeleteFunction",
        "LambdaFunction",
        function_name,
    )?;
    Ok(())
}

async fn create_lambda_event_source_mapping(
    client: &LambdaClient,
    request: CreateEventSourceMappingInput,
) -> Result<CreateEventSourceMappingOutput> {
    let resource_name = request
        .event_source_arn
        .as_deref()
        .or(request.function_name.as_deref())
        .unwrap_or("unknown")
        .to_string();

    lambda_result(
        client
            .create_event_source_mapping()
            .set_event_source_arn(request.event_source_arn)
            .set_function_name(request.function_name)
            .set_enabled(request.enabled)
            .set_batch_size(request.batch_size)
            .set_filter_criteria(request.filter_criteria)
            .set_maximum_batching_window_in_seconds(request.maximum_batching_window_in_seconds)
            .set_parallelization_factor(request.parallelization_factor)
            .set_starting_position(request.starting_position)
            .set_starting_position_timestamp(request.starting_position_timestamp)
            .set_destination_config(request.destination_config)
            .set_maximum_record_age_in_seconds(request.maximum_record_age_in_seconds)
            .set_bisect_batch_on_function_error(request.bisect_batch_on_function_error)
            .set_maximum_retry_attempts(request.maximum_retry_attempts)
            .set_tags(request.tags)
            .set_tumbling_window_in_seconds(request.tumbling_window_in_seconds)
            .set_topics(request.topics)
            .set_queues(request.queues)
            .set_source_access_configurations(request.source_access_configurations)
            .set_self_managed_event_source(request.self_managed_event_source)
            .set_function_response_types(request.function_response_types)
            .set_amazon_managed_kafka_event_source_config(
                request.amazon_managed_kafka_event_source_config,
            )
            .set_self_managed_kafka_event_source_config(
                request.self_managed_kafka_event_source_config,
            )
            .set_scaling_config(request.scaling_config)
            .set_document_db_event_source_config(request.document_db_event_source_config)
            .set_kms_key_arn(request.kms_key_arn)
            .set_metrics_config(request.metrics_config)
            .set_logging_config(request.logging_config)
            .set_provisioned_poller_config(request.provisioned_poller_config)
            .send()
            .await,
        "CreateEventSourceMapping",
        "EventSourceMapping",
        &resource_name,
    )
}

async fn delete_lambda_event_source_mapping(
    client: &LambdaClient,
    uuid: &str,
) -> Result<DeleteEventSourceMappingOutput> {
    lambda_result(
        client.delete_event_source_mapping().uuid(uuid).send().await,
        "DeleteEventSourceMapping",
        "EventSourceMapping",
        uuid,
    )
}

async fn list_lambda_event_source_mappings(
    client: &LambdaClient,
    request: ListEventSourceMappingsInput,
) -> Result<ListEventSourceMappingsOutput> {
    let resource_name = request
        .event_source_arn
        .as_deref()
        .or(request.function_name.as_deref())
        .unwrap_or("all")
        .to_string();

    lambda_result(
        client
            .list_event_source_mappings()
            .set_event_source_arn(request.event_source_arn)
            .set_function_name(request.function_name)
            .set_marker(request.marker)
            .set_max_items(request.max_items)
            .send()
            .await,
        "ListEventSourceMappings",
        "EventSourceMapping",
        &resource_name,
    )
}

async fn put_lambda_function_concurrency(
    client: &LambdaClient,
    function_name: &str,
    reserved_concurrent_executions: u32,
) -> Result<()> {
    let reserved_concurrent_executions =
        i32::try_from(reserved_concurrent_executions).map_err(|_| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Lambda reserved concurrency '{reserved_concurrent_executions}' exceeds i32 range"
                ),
                resource_id: Some(function_name.to_string()),
            })
        })?;

    lambda_result(
        client
            .put_function_concurrency()
            .function_name(function_name)
            .reserved_concurrent_executions(reserved_concurrent_executions)
            .send()
            .await,
        "PutFunctionConcurrency",
        "LambdaFunction",
        function_name,
    )?;
    Ok(())
}

async fn delete_lambda_function_concurrency(
    client: &LambdaClient,
    function_name: &str,
) -> Result<()> {
    lambda_result(
        client
            .delete_function_concurrency()
            .function_name(function_name)
            .send()
            .await,
        "DeleteFunctionConcurrency",
        "LambdaFunction",
        function_name,
    )?;
    Ok(())
}

fn lambda_result<T, E>(
    result: std::result::Result<T, LambdaSdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: LambdaProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                match service_error.code() {
                    Some("ResourceNotFoundException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                        }));
                    }
                    Some("ResourceConflictException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceConflict {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                            message: format!("{operation} reported ResourceConflictException"),
                        }));
                    }
                    _ => {}
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Lambda {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

async fn import_acm_certificate(
    client: &AcmClient,
    request: ImportCertificateInput,
    resource_name: &str,
) -> Result<ImportCertificateOutput> {
    let response = acm_result(
        client
            .import_certificate()
            .set_certificate_arn(request.certificate_arn)
            .set_certificate(request.certificate)
            .set_private_key(request.private_key)
            .set_certificate_chain(request.certificate_chain)
            .set_tags(request.tags)
            .send()
            .await,
        "ImportCertificate",
        "Certificate",
        resource_name,
    )?;

    if response.certificate_arn().is_none() {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: "ACM ImportCertificate response did not include certificateArn".to_string(),
            resource_id: None,
        }));
    }

    Ok(response)
}

async fn reimport_acm_certificate(
    client: &AcmClient,
    request: ImportCertificateInput,
) -> Result<()> {
    let certificate_arn = request.certificate_arn.clone().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "ACM reimport request did not include certificateArn".to_string(),
            resource_id: None,
        })
    })?;

    acm_result(
        client
            .import_certificate()
            .set_certificate_arn(request.certificate_arn)
            .set_certificate(request.certificate)
            .set_private_key(request.private_key)
            .set_certificate_chain(request.certificate_chain)
            .set_tags(request.tags)
            .send()
            .await,
        "ImportCertificate",
        "Certificate",
        &certificate_arn,
    )?;

    Ok(())
}

fn acm_result<T, E>(
    result: std::result::Result<T, AcmSdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: AcmProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                if service_error.code() == Some("ResourceNotFoundException") {
                    return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                        resource_type: resource_type.to_string(),
                        resource_name: resource_name.to_string(),
                    }));
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "ACM {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

async fn create_api_gateway_api(
    client: &ApiGatewayV2Client,
    request: CreateApiInput,
) -> Result<CreateApiOutput> {
    let resource_name = request
        .name
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    api_gateway_v2_result(
        client
            .create_api()
            .set_api_key_selection_expression(request.api_key_selection_expression)
            .set_cors_configuration(request.cors_configuration)
            .set_credentials_arn(request.credentials_arn)
            .set_description(request.description)
            .set_disable_schema_validation(request.disable_schema_validation)
            .set_disable_execute_api_endpoint(request.disable_execute_api_endpoint)
            .set_ip_address_type(request.ip_address_type)
            .set_name(request.name)
            .set_protocol_type(request.protocol_type)
            .set_route_key(request.route_key)
            .set_route_selection_expression(request.route_selection_expression)
            .set_tags(request.tags)
            .set_target(request.target)
            .set_version(request.version)
            .send()
            .await,
        "CreateApi",
        "ApiGatewayApi",
        &resource_name,
    )
}

async fn create_api_gateway_integration(
    client: &ApiGatewayV2Client,
    request: CreateIntegrationInput,
) -> Result<CreateIntegrationOutput> {
    let api_id = request
        .api_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    api_gateway_v2_result(
        client
            .create_integration()
            .set_api_id(request.api_id)
            .set_connection_id(request.connection_id)
            .set_connection_type(request.connection_type)
            .set_content_handling_strategy(request.content_handling_strategy)
            .set_credentials_arn(request.credentials_arn)
            .set_description(request.description)
            .set_integration_method(request.integration_method)
            .set_integration_subtype(request.integration_subtype)
            .set_integration_type(request.integration_type)
            .set_integration_uri(request.integration_uri)
            .set_passthrough_behavior(request.passthrough_behavior)
            .set_payload_format_version(request.payload_format_version)
            .set_request_parameters(request.request_parameters)
            .set_request_templates(request.request_templates)
            .set_response_parameters(request.response_parameters)
            .set_template_selection_expression(request.template_selection_expression)
            .set_timeout_in_millis(request.timeout_in_millis)
            .set_tls_config(request.tls_config)
            .send()
            .await,
        "CreateIntegration",
        "ApiGatewayIntegration",
        &api_id,
    )
}

async fn create_api_gateway_route(
    client: &ApiGatewayV2Client,
    request: CreateRouteInput,
) -> Result<CreateRouteOutput> {
    let api_id = request
        .api_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    api_gateway_v2_result(
        client
            .create_route()
            .set_api_id(request.api_id)
            .set_api_key_required(request.api_key_required)
            .set_authorization_scopes(request.authorization_scopes)
            .set_authorization_type(request.authorization_type)
            .set_authorizer_id(request.authorizer_id)
            .set_model_selection_expression(request.model_selection_expression)
            .set_operation_name(request.operation_name)
            .set_request_models(request.request_models)
            .set_request_parameters(request.request_parameters)
            .set_route_key(request.route_key)
            .set_route_response_selection_expression(request.route_response_selection_expression)
            .set_target(request.target)
            .send()
            .await,
        "CreateRoute",
        "ApiGatewayRoute",
        &api_id,
    )
}

async fn create_api_gateway_stage(
    client: &ApiGatewayV2Client,
    request: CreateStageInput,
) -> Result<CreateStageOutput> {
    let api_id = request
        .api_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    api_gateway_v2_result(
        client
            .create_stage()
            .set_access_log_settings(request.access_log_settings)
            .set_api_id(request.api_id)
            .set_auto_deploy(request.auto_deploy)
            .set_client_certificate_id(request.client_certificate_id)
            .set_default_route_settings(request.default_route_settings)
            .set_deployment_id(request.deployment_id)
            .set_description(request.description)
            .set_route_settings(request.route_settings)
            .set_stage_name(request.stage_name)
            .set_stage_variables(request.stage_variables)
            .set_tags(request.tags)
            .send()
            .await,
        "CreateStage",
        "ApiGatewayStage",
        &api_id,
    )
}

async fn create_api_gateway_domain_name(
    client: &ApiGatewayV2Client,
    request: CreateDomainNameInput,
) -> Result<CreateDomainNameOutput> {
    let domain_name = request
        .domain_name
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    api_gateway_v2_result(
        client
            .create_domain_name()
            .set_domain_name(request.domain_name)
            .set_domain_name_configurations(request.domain_name_configurations)
            .set_mutual_tls_authentication(request.mutual_tls_authentication)
            .set_routing_mode(request.routing_mode)
            .set_tags(request.tags)
            .send()
            .await,
        "CreateDomainName",
        "ApiGatewayDomainName",
        &domain_name,
    )
}

async fn create_api_gateway_mapping(
    client: &ApiGatewayV2Client,
    request: CreateApiMappingInput,
) -> Result<CreateApiMappingOutput> {
    let domain_name = request
        .domain_name
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    api_gateway_v2_result(
        client
            .create_api_mapping()
            .set_api_id(request.api_id)
            .set_api_mapping_key(request.api_mapping_key)
            .set_domain_name(request.domain_name)
            .set_stage(request.stage)
            .send()
            .await,
        "CreateApiMapping",
        "ApiGatewayApiMapping",
        &domain_name,
    )
}

async fn delete_api_gateway_mapping(
    client: &ApiGatewayV2Client,
    domain_name: &str,
    api_mapping_id: &str,
) -> Result<()> {
    api_gateway_v2_result(
        client
            .delete_api_mapping()
            .domain_name(domain_name)
            .api_mapping_id(api_mapping_id)
            .send()
            .await,
        "DeleteApiMapping",
        "ApiGatewayApiMapping",
        domain_name,
    )?;

    Ok(())
}

fn api_gateway_v2_result<T, E>(
    result: std::result::Result<T, ApiGatewayV2SdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: ApiGatewayV2ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                match service_error.code() {
                    Some("NotFoundException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                        }));
                    }
                    Some("ConflictException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceConflict {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                            message: service_error
                                .message()
                                .unwrap_or("API Gateway V2 conflict")
                                .to_string(),
                        }));
                    }
                    _ => {}
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                    "API Gateway V2 {operation} API failed for {resource_type} '{resource_name}'"
                ),
                    resource_id: None,
                }))
        }
    }
}

async fn put_eventbridge_rule(
    client: &EventBridgeClient,
    request: PutRuleInput,
) -> Result<PutRuleOutput> {
    let rule_name = request
        .name
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    eventbridge_result(
        client
            .put_rule()
            .set_name(request.name)
            .set_schedule_expression(request.schedule_expression)
            .set_event_pattern(request.event_pattern)
            .set_state(request.state)
            .set_description(request.description)
            .set_role_arn(request.role_arn)
            .set_tags(request.tags)
            .set_event_bus_name(request.event_bus_name)
            .send()
            .await,
        "PutRule",
        "EventBridgeRule",
        &rule_name,
    )
}

async fn put_eventbridge_targets(
    client: &EventBridgeClient,
    request: PutTargetsInput,
) -> Result<()> {
    let rule_name = request
        .rule
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let output = eventbridge_result(
        client
            .put_targets()
            .set_rule(request.rule)
            .set_event_bus_name(request.event_bus_name)
            .set_targets(request.targets)
            .send()
            .await,
        "PutTargets",
        "EventBridgeRule",
        &rule_name,
    )?;

    ensure_no_eventbridge_target_failures(
        output.failed_entry_count,
        format!("{:?}", output.failed_entries),
        "PutTargets",
        &rule_name,
    )
}

async fn remove_eventbridge_targets(
    client: &EventBridgeClient,
    rule_name: &str,
    target_ids: Vec<String>,
) -> Result<()> {
    let output = eventbridge_result(
        client
            .remove_targets()
            .rule(rule_name)
            .set_ids(Some(target_ids))
            .send()
            .await,
        "RemoveTargets",
        "EventBridgeRule",
        rule_name,
    )?;

    ensure_no_eventbridge_target_failures(
        output.failed_entry_count,
        format!("{:?}", output.failed_entries),
        "RemoveTargets",
        rule_name,
    )
}

fn ensure_no_eventbridge_target_failures(
    failed_entry_count: i32,
    failed_entries: String,
    operation: &str,
    rule_name: &str,
) -> Result<()> {
    if failed_entry_count == 0 {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::CloudPlatformError {
        message: format!(
            "EventBridge {operation} reported {failed_entry_count} failed target entries for rule '{rule_name}': {failed_entries}"
        ),
        resource_id: None,
    }))
}

fn eventbridge_result<T, E>(
    result: std::result::Result<T, EventBridgeSdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: EventBridgeProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                match service_error.code() {
                    Some("ResourceNotFoundException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                        }));
                    }
                    Some("ResourceAlreadyExistsException" | "ConcurrentModificationException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceConflict {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                            message: service_error
                                .message()
                                .unwrap_or("EventBridge conflict")
                                .to_string(),
                        }));
                    }
                    _ => {}
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "EventBridge {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

fn is_remote_resource_conflict(error: &AlienError<ErrorData>) -> bool {
    matches!(&error.error, Some(ErrorData::CloudResourceConflict { .. }))
}

fn replace_lambda_notification_config(
    notification_config: &mut NotificationConfiguration,
    replacement: LambdaFunctionConfiguration,
) {
    let mut lambda_function_configurations = notification_config
        .lambda_function_configurations
        .take()
        .unwrap_or_default();
    if let Some(replacement_id) = replacement.id() {
        lambda_function_configurations.retain(|config| config.id() != Some(replacement_id));
    }
    lambda_function_configurations.push(replacement);
    notification_config.lambda_function_configurations = Some(lambda_function_configurations);
}

fn s3_trigger_events(events: &[String]) -> Vec<S3Event> {
    events
        .iter()
        .map(|event| match event.as_str() {
            "created" => S3Event::from("s3:ObjectCreated:*"),
            "deleted" => S3Event::from("s3:ObjectRemoved:*"),
            other => S3Event::from(format!("s3:{other}").as_str()),
        })
        .collect()
}

fn s3_lambda_notification_config(
    statement_id: &str,
    function_arn: &str,
    events: &[String],
    resource_id: &str,
) -> Result<LambdaFunctionConfiguration> {
    LambdaFunctionConfiguration::builder()
        .id(statement_id)
        .lambda_function_arn(function_arn)
        .set_events(Some(s3_trigger_events(events)))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Invalid S3 Lambda notification configuration".to_string(),
            resource_id: Some(resource_id.to_string()),
        })
}

async fn get_s3_bucket_notification_configuration(
    client: &S3Client,
    bucket_name: &str,
) -> Result<NotificationConfiguration> {
    match client
        .get_bucket_notification_configuration()
        .bucket(bucket_name)
        .send()
        .await
    {
        Ok(output) => Ok(notification_configuration_from_get_output(output)),
        Err(err) if is_s3_get_notification_not_found(&err) => {
            Ok(NotificationConfiguration::builder().build())
        }
        Err(err) => Err(err
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "S3 GetBucketNotificationConfiguration API failed for bucket '{bucket_name}'"
                ),
                resource_id: None,
            })),
    }
}

async fn put_s3_bucket_notification_configuration(
    client: &S3Client,
    bucket_name: &str,
    config: &NotificationConfiguration,
) -> Result<PutBucketNotificationConfigurationOutput> {
    client
        .put_bucket_notification_configuration()
        .bucket(bucket_name)
        .notification_configuration(config.clone())
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "S3 PutBucketNotificationConfiguration API failed for bucket '{bucket_name}'"
            ),
            resource_id: None,
        })
}

fn is_s3_get_notification_not_found(
    error: &S3SdkError<GetBucketNotificationConfigurationError>,
) -> bool {
    error
        .as_service_error()
        .and_then(S3ProvideErrorMetadata::code)
        .is_some_and(|code| code == "NoSuchBucket")
}

fn notification_configuration_from_get_output(
    output: GetBucketNotificationConfigurationOutput,
) -> NotificationConfiguration {
    NotificationConfiguration::builder()
        .set_topic_configurations(output.topic_configurations)
        .set_queue_configurations(output.queue_configurations)
        .set_lambda_function_configurations(output.lambda_function_configurations)
        .set_event_bridge_configuration(output.event_bridge_configuration)
        .build()
}

impl AwsWorkerController {
    fn should_wait_for_lambda_vpc_enis(ctx: &ResourceControllerContext<'_>) -> bool {
        matches!(
            ctx.deployment_config.stack_settings.network,
            Some(NetworkSettings::Create { .. })
        )
    }

    fn resolve_domain_info(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<Option<DomainInfo>> {
        let stack_settings = &ctx.deployment_config.stack_settings;
        if let Some(custom) = stack_settings
            .domains
            .as_ref()
            .and_then(|domains| domains.custom_domains.as_ref())
            .and_then(|domains| domains.get(resource_id))
        {
            let cert_arn = custom
                .certificate
                .aws
                .as_ref()
                .map(|cert| cert.certificate_arn.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires an AWS certificate ARN".to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(Some(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                certificate_arn: Some(cert_arn),
                uses_custom_domain: true,
            }));
        }

        let Some(resource) = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|metadata| metadata.resources.get(resource_id))
        else {
            return Ok(None);
        };

        Ok(Some(DomainInfo {
            fqdn: resource.fqdn.clone(),
            certificate_id: Some(resource.certificate_id.clone()),
            certificate_arn: None,
            uses_custom_domain: false,
        }))
    }

    fn ensure_domain_info(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<bool> {
        if self.fqdn.is_some()
            && self.domain_name.is_some()
            && (self.certificate_id.is_some()
                || self.certificate_arn.is_some()
                || self.uses_custom_domain)
        {
            return Ok(true);
        }

        match Self::resolve_domain_info(ctx, resource_id)? {
            Some(domain_info) => {
                self.fqdn = Some(domain_info.fqdn.clone());
                self.domain_name = Some(domain_info.fqdn.clone());
                self.certificate_id = domain_info.certificate_id;
                self.certificate_arn = domain_info.certificate_arn;
                self.uses_custom_domain = domain_info.uses_custom_domain;
                if self.url.is_none() {
                    self.url = ctx
                        .deployment_config
                        .public_urls
                        .as_ref()
                        .and_then(|urls| urls.get(resource_id).cloned())
                        .or_else(|| Some(format!("https://{}", domain_info.fqdn)));
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn unexpected_update_wrapper_state(
        resource_id: &str,
        handler: &str,
        state: AwsWorkerState,
    ) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: resource_id.to_string(),
            message: format!("{handler} returned unexpected state during update: {state:?}"),
        })
    }
}

#[controller]
pub struct AwsWorkerController {
    pub(crate) arn: Option<String>,
    pub(crate) url: Option<String>,
    /// The logical AWS Lambda worker name (stack prefix + id). Stored to expose in outputs.
    pub(crate) worker_name: Option<String>,
    /// Event source mapping UUIDs for queue triggers
    pub(crate) event_source_mappings: Vec<String>,
    /// Fully qualified domain name for public ingress
    pub(crate) fqdn: Option<String>,
    /// Certificate ID for auto-managed domains
    pub(crate) certificate_id: Option<String>,
    /// ACM certificate ARN (auto-imported or custom)
    pub(crate) certificate_arn: Option<String>,
    /// API Gateway HTTP API ID
    pub(crate) api_id: Option<String>,
    /// API Gateway integration ID
    pub(crate) integration_id: Option<String>,
    /// API Gateway route ID
    pub(crate) route_id: Option<String>,
    /// API Gateway stage name
    pub(crate) stage_name: Option<String>,
    /// API Gateway API mapping ID
    pub(crate) api_mapping_id: Option<String>,
    /// API Gateway domain name
    pub(crate) domain_name: Option<String>,
    /// Endpoint metadata for DNS controller
    pub(crate) load_balancer: Option<LoadBalancerState>,
    /// Timestamp when certificate was imported (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,
    /// Whether this resource uses a customer-managed domain
    pub(crate) uses_custom_domain: bool,
    /// Statement IDs for Lambda permissions granted to S3 for storage triggers
    pub(crate) s3_permission_statement_ids: Vec<String>,
    /// EventBridge rule names for schedule triggers
    pub(crate) eventbridge_rule_names: Vec<String>,
    /// Statement IDs for Lambda permissions granted to EventBridge for schedule triggers
    pub(crate) eventbridge_permission_statement_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerEndpoint {
    pub dns_name: String,
    pub hosted_zone_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerState {
    pub endpoint: Option<LoadBalancerEndpoint>,
}

struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    certificate_arn: Option<String>,
    uses_custom_domain: bool,
}

fn lambda_state_as_str(state: &Option<LambdaState>) -> &str {
    state.as_ref().map(LambdaState::as_str).unwrap_or("unknown")
}

fn lambda_last_update_status_as_str(status: &Option<LambdaLastUpdateStatus>) -> &str {
    status
        .as_ref()
        .map(LambdaLastUpdateStatus::as_str)
        .unwrap_or("unknown")
}

fn emit_aws_lambda_worker_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    worker_config: &Worker,
    aws_worker_name: &str,
    function_info: &GetFunctionConfigurationOutput,
) {
    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: worker_config.id.clone(),
        resource_type: Worker::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Worker(WorkerHeartbeatData::AwsLambda(
            AwsLambdaWorkerHeartbeatData {
                status: WorkloadHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!("AWS Lambda function '{aws_worker_name}' is active")),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                function_name: function_info
                    .function_name
                    .clone()
                    .unwrap_or_else(|| aws_worker_name.to_string()),
                runtime: None,
                package_type: None,
                memory_size_mb: None,
                timeout_seconds: None,
                version: None,
                revision_id: None,
                last_modified: None,
                state: function_info
                    .state
                    .as_ref()
                    .map(|state| state.as_str().to_string()),
                state_reason: None,
                state_reason_code: None,
                last_update_status: function_info
                    .last_update_status
                    .as_ref()
                    .map(|status| status.as_str().to_string()),
                last_update_status_reason: None,
                last_update_status_reason_code: None,
                code_sha256: None,
                layer_count: 0,
                function_url_auth_type: None,
                function_url_cors_present: false,
                trigger_count: worker_config.triggers.len() as u32,
            },
        )),
        raw: vec![],
    });
}

#[controller]
impl AwsWorkerController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let cfg = ctx.desired_resource_config::<Worker>()?;
        info!(name=%cfg.id, code=?cfg.code, "Initiating creation");

        // Product limitation: Only allow at most one queue trigger per worker
        let queue_trigger_count = cfg
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    cfg.id, queue_trigger_count
                ),
                resource_id: Some(cfg.id.clone()),
            }));
        }

        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", cfg.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        // Get the ServiceAccount's role ARN
        let service_account_state = ctx
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                &service_account_ref,
            )?;

        let role_arn = service_account_state
            .role_arn
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: cfg.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        let image_uri = match &cfg.code {
            alien_core::WorkerCode::Image { image } => image.clone(),
            alien_core::WorkerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Worker is configured with source code, but only pre-built images are supported".to_string(),
                    resource_id: Some(cfg.id.clone()),
                }));
            }
        };

        // Resolve proxy URIs to native ECR URIs. Lambda can only pull from ECR.
        // The release stores proxy URIs; native_image_host carries the ECR prefix.
        let image_uri = if let Some(ref native_host) = ctx.deployment_config.native_image_host {
            alien_core::image_rewrite::resolve_native_image_uri(&image_uri, native_host)
                .unwrap_or(image_uri)
        } else {
            image_uri
        };

        // Lambda requires container images in the same region as the worker.
        // If the image URI points to ECR in a different region (e.g., the management
        // region), rewrite it to reference the local region where the replicated copy
        // lives. ECR private image replication must be configured separately.
        let image_uri = Self::rewrite_ecr_region_if_needed(&image_uri, &aws_cfg.region);

        let code = FunctionCode::builder().image_uri(image_uri).build();
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &cfg.id);
        let mut function_tags = standard_resource_tags(ctx.resource_prefix, &cfg.id);
        function_tags.insert("Name".to_string(), aws_worker_name.clone());

        if cfg.ingress == Ingress::Public {
            match Self::resolve_domain_info(ctx, &cfg.id)? {
                Some(domain_info) => {
                    self.fqdn = Some(domain_info.fqdn.clone());
                    self.certificate_id = domain_info.certificate_id;
                    self.certificate_arn = domain_info.certificate_arn;
                    self.uses_custom_domain = domain_info.uses_custom_domain;
                    self.domain_name = Some(domain_info.fqdn.clone());

                    // Check for URL override in deployment config, otherwise use domain FQDN
                    self.url = ctx
                        .deployment_config
                        .public_urls
                        .as_ref()
                        .and_then(|urls| urls.get(&cfg.id).cloned())
                        .or_else(|| Some(format!("https://{}", domain_info.fqdn)));
                }
                None => {
                    // Standalone mode: no domain metadata available.
                    // Use API Gateway with its default endpoint URL (no custom domain).
                    // The URL will be set after API Gateway creation.
                    info!(
                        worker=%cfg.id,
                        "No domain metadata — will use API Gateway default endpoint (standalone mode)"
                    );
                }
            }
        }

        // Prepare environment variables
        let env_vars = self
            .prepare_environment_variables(&cfg.environment, &cfg.links, ctx, &aws_worker_name)
            .await?;

        let environment = if !env_vars.is_empty() {
            Some(Environment::builder().set_variables(Some(env_vars)).build())
        } else {
            None
        };

        // Get VPC configuration if a Network resource exists
        let vpc_config = self.get_vpc_config(ctx)?;
        if vpc_config.is_some() {
            info!(name=%aws_worker_name, "Configuring Lambda worker to run inside VPC");
        }

        let request = CreateFunctionInput::builder()
            .function_name(aws_worker_name.clone())
            .role(role_arn)
            .code(code)
            .package_type(PackageType::Image)
            .description(format!("Runtime worker: {}", cfg.id))
            .timeout(cfg.timeout_seconds as i32)
            .memory_size(cfg.memory_mb as i32)
            .publish(false)
            .set_tags(Some(function_tags))
            .set_environment(environment)
            .architectures(LambdaArchitecture::Arm64)
            .set_vpc_config(vpc_config)
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to build Lambda CreateFunction request".to_string(),
                resource_id: Some(cfg.id.clone()),
            })?;

        let response = create_lambda_function(&client, request).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to create Lambda worker".to_string(),
                resource_id: Some(cfg.id.clone()),
            },
        )?;

        self.arn = response.function_arn.clone();
        self.worker_name = Some(aws_worker_name.clone());
        info!(name=%aws_worker_name, arn=%self.arn.as_deref().unwrap_or("unknown"), "Worker created, waiting for active state");

        Ok(HandlerAction::Continue {
            state: CreateWaitForActive,
            suggested_delay: Some(Duration::from_secs(3)),
        })
    }

    #[handler(
        state = CreateWaitForActive,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        debug!(name=%aws_worker_name, "Checking worker state");

        let response = get_lambda_function_configuration(&client, &aws_worker_name, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Lambda worker configuration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // Check if worker is active based on state and last_update_status
        let is_active = response.state.as_ref() == Some(&LambdaState::Active)
            && response.last_update_status.as_ref() == Some(&LambdaLastUpdateStatus::Successful);

        if is_active {
            if worker_config.ingress == Ingress::Public {
                let has_domain_info = self.ensure_domain_info(ctx, &worker_config.id)?;
                let next_state = if has_domain_info {
                    // Platform mode: wait for certificate then create API Gateway + custom domain
                    WaitingForCertificate
                } else {
                    // Standalone mode: skip certificate/custom domain, use API Gateway default endpoint
                    CreatingApiGateway
                };
                Ok(HandlerAction::Continue {
                    state: next_state,
                    suggested_delay: Some(Duration::from_secs(2)),
                })
            } else {
                Ok(HandlerAction::Continue {
                    state: ApplyingResourcePermissions,
                    suggested_delay: Some(Duration::from_secs(2)),
                })
            }
        } else {
            debug!(
                name = %aws_worker_name,
                state = %lambda_state_as_str(&response.state),
                last_update_status = %lambda_last_update_status_as_str(&response.last_update_status),
                "Worker not yet active, retrying"
            );
            Ok(HandlerAction::Stay {
                max_times: 20,
                suggested_delay: Some(Duration::from_secs(3)),
            })
        }
    }

    #[handler(
        state = WaitingForCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id));

        let status = metadata.map(|m| &m.certificate_status);
        if !self.ensure_domain_info(ctx, &worker_config.id)? {
            return Ok(HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }
        if self.uses_custom_domain && self.certificate_arn.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        match status {
            Some(CertificateStatus::Issued) => Ok(HandlerAction::Continue {
                state: ImportingCertificate,
                suggested_delay: None,
            }),
            Some(CertificateStatus::Failed) => {
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "Certificate issuance failed".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = ImportingCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn importing_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        self.ensure_domain_info(ctx, &worker_config.id)?;
        let resource = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for certificate import".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;

        // Certificate data is included in DeploymentConfig
        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let (leaf, chain) = split_certificate_chain(certificate_chain);

        let aws_cfg = ctx.get_aws_config()?;
        let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
        let tags = standard_resource_tags(ctx.resource_prefix, &worker_config.id)
            .into_iter()
            .map(|(key, value)| {
                let tag_key = key.clone();
                Tag::builder()
                    .key(key)
                    .value(value)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Invalid ACM tag '{tag_key}'"),
                        resource_id: Some(worker_config.id.clone()),
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let import_request = ImportCertificateInput::builder()
            .certificate(Blob::new(leaf.into_bytes()))
            .private_key(Blob::new(private_key.clone().into_bytes()))
            .set_certificate_chain(chain.map(|chain| Blob::new(chain.into_bytes())))
            .set_tags(Some(tags))
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Invalid ACM certificate import request".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
        let response = import_acm_certificate(&acm_client, import_request, "new")
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import certificate to ACM".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.certificate_arn = Some(
            response
                .certificate_arn()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: "ACM ImportCertificate response did not include certificateArn"
                            .to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    })
                })?
                .to_string(),
        );

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        Ok(HandlerAction::Continue {
            state: CreatingApiGateway,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingApiGateway,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.api_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiIntegration,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let api_tags = standard_resource_tags(ctx.resource_prefix, &worker_config.id);

        let api = create_api_gateway_api(
            &client,
            CreateApiInput::builder()
                .name(format!("{}-{}-api", ctx.resource_prefix, worker_config.id))
                .protocol_type(ProtocolType::Http)
                .set_tags(Some(api_tags))
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Invalid API Gateway HTTP API create request".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to create API Gateway HTTP API".to_string(),
            resource_id: Some(worker_config.id.clone()),
        })?;

        let api_id = api.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "API Gateway ID not returned".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        self.api_id = Some(api_id);

        Ok(HandlerAction::Continue {
            state: CreatingApiIntegration,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingApiIntegration,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_integration(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.integration_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiRoute,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for integration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let function_arn = self.arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN missing for integration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let integration = create_api_gateway_integration(
            &client,
            CreateIntegrationInput::builder()
                .api_id(api_id.clone())
                .integration_type(IntegrationType::AwsProxy)
                .integration_uri(function_arn)
                .payload_format_version("2.0")
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Invalid API Gateway integration create request".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to create API integration".to_string(),
            resource_id: Some(worker_config.id.clone()),
        })?;

        let integration_id = integration.integration_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Integration ID not returned".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        self.integration_id = Some(integration_id.clone());

        Ok(HandlerAction::Continue {
            state: CreatingApiRoute,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingApiRoute,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_route(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.route_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiStage,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for route".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let integration_id = self.integration_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Integration ID missing for route".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let route = create_api_gateway_route(
            &client,
            CreateRouteInput::builder()
                .api_id(api_id.clone())
                .route_key("$default")
                .target(format!("integrations/{}", integration_id))
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Invalid API Gateway route create request".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to create API route".to_string(),
            resource_id: Some(worker_config.id.clone()),
        })?;

        self.route_id = route.route_id.clone();

        Ok(HandlerAction::Continue {
            state: CreatingApiStage,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingApiStage,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_stage(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if self.stage_name.is_some() {
            if self.fqdn.is_some() {
                return Ok(HandlerAction::Continue {
                    state: CreatingApiDomain,
                    suggested_delay: Some(Duration::from_secs(1)),
                });
            }

            let aws_cfg = ctx.get_aws_config()?;
            let api_id = self.api_id.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "API ID missing for default endpoint".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;
            self.url = Some(format!(
                "https://{}.execute-api.{}.amazonaws.com",
                api_id, aws_cfg.region
            ));
            return Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for stage".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let stage = create_api_gateway_stage(
            &client,
            CreateStageInput::builder()
                .api_id(api_id.clone())
                .stage_name("$default")
                .auto_deploy(true)
                .set_tags(Some(standard_resource_tags(
                    ctx.resource_prefix,
                    &worker_config.id,
                )))
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Invalid API Gateway stage create request".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to create API stage".to_string(),
            resource_id: Some(worker_config.id.clone()),
        })?;

        self.stage_name = stage.stage_name.clone().or(Some("$default".to_string()));

        if self.fqdn.is_some() {
            // Platform mode: proceed to custom domain setup
            Ok(HandlerAction::Continue {
                state: CreatingApiDomain,
                suggested_delay: Some(Duration::from_secs(1)),
            })
        } else {
            // Standalone mode: use the default API Gateway endpoint URL
            let aws_cfg = ctx.get_aws_config()?;
            let region = &aws_cfg.region;
            let default_url = format!("https://{}.execute-api.{}.amazonaws.com", api_id, region);
            info!(
                worker=%worker_config.id,
                url=%default_url,
                "Using API Gateway default endpoint (no custom domain)"
            );
            self.url = Some(default_url);
            Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            })
        }
    }

    #[handler(
        state = CreatingApiDomain,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.load_balancer.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiMapping,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let fqdn = self.fqdn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "FQDN missing for API domain".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let cert_arn = self.certificate_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate ARN missing for API domain".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let domain = create_api_gateway_domain_name(
            &client,
            CreateDomainNameInput::builder()
                .domain_name(fqdn.clone())
                .domain_name_configurations(
                    DomainNameConfiguration::builder()
                        .certificate_arn(cert_arn)
                        .endpoint_type(EndpointType::Regional)
                        .security_policy(SecurityPolicy::Tls12)
                        .build(),
                )
                .set_tags(Some(standard_resource_tags(
                    ctx.resource_prefix,
                    &worker_config.id,
                )))
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Invalid API Gateway domain create request".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to create API domain name".to_string(),
            resource_id: Some(worker_config.id.clone()),
        })?;

        let endpoint = domain
            .domain_name_configurations
            .as_ref()
            .and_then(|configs| configs.first())
            .and_then(|config| {
                let dns_name = config.api_gateway_domain_name.clone()?;
                let hosted_zone_id = config.hosted_zone_id.clone()?;
                Some(LoadBalancerEndpoint {
                    dns_name,
                    hosted_zone_id,
                })
            });

        self.load_balancer = Some(LoadBalancerState { endpoint });

        Ok(HandlerAction::Continue {
            state: CreatingApiMapping,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingApiMapping,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.api_mapping_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let domain_name = self.domain_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain name missing for mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let stage = self
            .stage_name
            .clone()
            .unwrap_or_else(|| "$default".to_string());

        let mapping = create_api_gateway_mapping(
            &client,
            CreateApiMappingInput::builder()
                .domain_name(domain_name)
                .api_id(api_id.clone())
                .stage(stage)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Invalid API Gateway API mapping create request".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to create API mapping".to_string(),
            resource_id: Some(worker_config.id.clone()),
        })?;

        self.api_mapping_id = mapping.api_mapping_id.clone();

        Ok(HandlerAction::Continue {
            state: AddingApiGatewayPermission,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = AddingApiGatewayPermission,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn adding_api_gateway_permission(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);

        let request = AddPermissionInput::builder()
            .function_name(aws_worker_name.clone())
            .statement_id("ApiGatewayInvoke")
            .action("lambda:InvokeFunction")
            .principal("apigateway.amazonaws.com")
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Invalid Lambda API Gateway permission request".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        add_lambda_permission(&client, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to add API Gateway permission".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        if self.fqdn.is_some() {
            if self.uses_custom_domain {
                // Custom domain: readiness probe then done
                Ok(HandlerAction::Continue {
                    state: RunningReadinessProbe,
                    suggested_delay: None,
                })
            } else {
                // Platform-managed domain: wait for DNS propagation
                Ok(HandlerAction::Continue {
                    state: WaitingForDns,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        } else {
            // Standalone mode: no custom domain, skip DNS and readiness probe
            Ok(HandlerAction::Continue {
                state: ApplyingResourcePermissions,
                suggested_delay: None,
            })
        }
    }

    #[handler(
        state = WaitingForDns,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_dns(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id));

        let status = metadata.map(|m| &m.dns_status);

        match status {
            Some(DnsRecordStatus::Active) => Ok(HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay: None,
            }),
            Some(DnsRecordStatus::Failed) => {
                let fqdn = metadata.map(|m| m.fqdn.as_str()).unwrap_or("unknown");
                let detail = metadata
                    .and_then(|m| m.dns_error.as_deref())
                    .unwrap_or("unknown error");
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("DNS record creation failed for {fqdn}: {detail}"),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = RunningReadinessProbe,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn running_readiness_probe(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Only run readiness probe if configured and we have a URL (for public workers)
        if worker_config.readiness_probe.is_some() && worker_config.ingress == Ingress::Public {
            if let Some(url) = &self.url {
                let dns_override = readiness_probe_dns_override(
                    url,
                    self.fqdn.as_deref(),
                    self.load_balancer.as_ref(),
                );

                match run_readiness_probe_with_dns_override(ctx, url, dns_override).await {
                    Ok(()) => {
                        // Probe succeeded, proceed to Ready
                    }
                    Err(_) => {
                        // Probe failed, let the framework handle retries
                        return Ok(HandlerAction::Stay {
                            max_times: READINESS_PROBE_MAX_ATTEMPTS,
                            suggested_delay: Some(Duration::from_secs(5)),
                        });
                    }
                }
            }
        }

        // Either no readiness probe needed, or probe succeeded - proceed to ApplyingResourcePermissions
        Ok(HandlerAction::Continue {
            state: ApplyingResourcePermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingResourcePermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;

        info!(worker=%config.id, "Applying resource-scoped permissions for Lambda worker");

        // Apply resource-scoped permissions from the stack using the centralized helper.
        // This handles wildcard ("*") permissions and management SA permissions.
        if let Some(worker_name) = &self
            .arn
            .as_ref()
            .and_then(|arn| arn.split(':').last().map(|s| s.to_string()))
        {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                &worker_name,
                "worker",
            )
            .await?;
        }

        info!(worker=%config.id, "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: UpdatingEnvVarsWithSelfBinding,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingEnvVarsWithSelfBinding,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn updating_env_vars_with_self_binding(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;

        // Skip this step if the worker doesn't have public ingress
        // For private workers, the initial env vars already have complete self-binding
        // (no URL to add later)
        if config.ingress != Ingress::Public {
            info!(worker=%config.id, "Skipping env var update - no public URL to add");
            return Ok(HandlerAction::Continue {
                state: CreatingEventSourceMappings,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &config.id);

        // Now that we have the URL, update the environment variables
        // with the complete self-binding information including the URL
        let final_env_vars = self
            .prepare_environment_variables(
                &config.environment,
                &config.links,
                ctx,
                &aws_worker_name,
            )
            .await?;

        let lambda_environment = if !final_env_vars.is_empty() {
            Some(
                Environment::builder()
                    .set_variables(Some(final_env_vars))
                    .build(),
            )
        } else {
            None
        };

        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );
        let service_account_state = ctx
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                &service_account_ref,
            )?;
        let role_arn = service_account_state
            .role_arn
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: config.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        // Get VPC configuration if a Network resource exists
        let vpc_config = self.get_vpc_config(ctx)?;

        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for self-binding update".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let request = UpdateFunctionConfigurationInput::builder()
            .function_name(arn.clone())
            .role(role_arn)
            .timeout(config.timeout_seconds as i32)
            .memory_size(config.memory_mb as i32)
            .set_environment(lambda_environment)
            .set_vpc_config(vpc_config)
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Invalid Lambda configuration update request".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        update_lambda_function_configuration(&client, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update Lambda function with resolved self-bindings".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(worker=%config.id, "Successfully updated environment variables with complete self-binding");

        Ok(HandlerAction::Continue {
            state: CreatingEventSourceMappings,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = CreatingEventSourceMappings,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_event_source_mappings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;

        // Validation: Only allow at most one queue trigger per worker (non-retriable error)
        let queue_trigger_count = config
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    config.id, queue_trigger_count
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        // Linear flow principle: Always perform this state. Create mappings for ALL queue triggers
        let mut created_any = false;
        for trigger in &config.triggers {
            if let alien_core::WorkerTrigger::Queue { queue } = trigger {
                info!(worker=%config.id, queue=%queue.id, "Creating SQS event source mapping");
                self.create_queue_event_source_mapping(ctx, aws_cfg, &config, queue)
                    .await?;
                created_any = true;
            }
        }
        if !created_any {
            info!(worker=%config.id, "No queue triggers found, skipping event source mapping creation");
        }

        // Handle storage triggers: configure S3 bucket notifications to invoke this Lambda
        let worker_name = self.worker_name.as_deref().unwrap_or("unknown");
        let function_arn = self.arn.as_deref().unwrap_or("unknown");

        for trigger in &config.triggers {
            if let alien_core::WorkerTrigger::Storage {
                storage: storage_ref,
                events,
            } = trigger
            {
                info!(worker=%config.id, storage=%storage_ref.id, "Configuring S3 storage trigger");

                // Get storage controller to access bucket name
                let storage_controller =
                    ctx.require_dependency::<crate::storage::AwsStorageController>(storage_ref)?;
                let bucket_name = storage_controller.bucket_name.as_deref().ok_or_else(|| {
                    AlienError::new(ErrorData::DependencyNotReady {
                        resource_id: config.id.clone(),
                        dependency_id: storage_ref.id.clone(),
                    })
                })?;

                // Add Lambda permission for S3 to invoke this worker
                let statement_id = format!("{}-s3-{}", worker_name, storage_ref.id);
                let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
                let permission_request = AddPermissionInput::builder()
                    .function_name(worker_name)
                    .statement_id(statement_id.clone())
                    .action("lambda:InvokeFunction")
                    .principal("s3.amazonaws.com")
                    .source_arn(format!("arn:aws:s3:::{}", bucket_name))
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Invalid Lambda S3 permission request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;

                match add_lambda_permission(&lambda_client, permission_request).await {
                    Ok(_) => {}
                    Err(e) if is_remote_resource_conflict(&e) => {
                        info!(
                            worker=%config.id,
                            statement_id=%statement_id,
                            "S3 invoke permission already exists; treating as created"
                        );
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to add S3 invoke permission for storage '{}'",
                                storage_ref.id
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }

                // Get current notification config and merge in new Lambda config
                let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                let mut notification_config =
                    get_s3_bucket_notification_configuration(&s3_client, bucket_name)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to get notification configuration for bucket '{}'",
                                bucket_name
                            ),
                            resource_id: Some(config.id.clone()),
                        })?;

                replace_lambda_notification_config(
                    &mut notification_config,
                    s3_lambda_notification_config(&statement_id, function_arn, events, &config.id)?,
                );

                put_s3_bucket_notification_configuration(
                    &s3_client,
                    bucket_name,
                    &notification_config,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to put notification configuration for bucket '{}'",
                        bucket_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

                if !self.s3_permission_statement_ids.contains(&statement_id) {
                    self.s3_permission_statement_ids.push(statement_id.clone());
                }
                info!(
                    worker=%config.id,
                    storage=%storage_ref.id,
                    bucket=%bucket_name,
                    statement_id=%statement_id,
                    "S3 storage trigger configured"
                );
            }
        }

        // Continue to schedule trigger creation (linear flow)
        Ok(HandlerAction::Continue {
            state: CreatingScheduleTriggers,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingScheduleTriggers,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_schedule_triggers(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;

        let worker_name = self.worker_name.as_deref().unwrap_or("unknown");
        let function_arn = self.arn.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for schedule trigger".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        for (index, trigger) in config.triggers.iter().enumerate() {
            if let alien_core::WorkerTrigger::Schedule { cron } = trigger {
                info!(worker=%config.id, cron=%cron, index=%index, "Creating EventBridge schedule trigger");

                let rule_name = format!("{}-cron-{}", worker_name, index);

                // Convert standard 5-field cron to EventBridge format
                let schedule_expression =
                    crate::worker::crontab_to_eventbridge::crontab_to_eventbridge(cron).map_err(
                        |e| {
                            AlienError::new(ErrorData::ResourceConfigInvalid {
                                message: format!("Invalid cron expression '{}': {}", cron, e),
                                resource_id: Some(config.id.clone()),
                            })
                        },
                    )?;

                // Create EventBridge rule
                let eventbridge_client = ctx
                    .service_provider
                    .get_aws_eventbridge_client(aws_cfg)
                    .await?;

                let rule_response = put_eventbridge_rule(
                    &eventbridge_client,
                    PutRuleInput::builder()
                        .name(rule_name.clone())
                        .schedule_expression(schedule_expression)
                        .state(RuleState::Enabled)
                        .description(format!("Alien schedule trigger for worker '{}'", config.id))
                        .set_tags(Some(eventbridge_tags(ctx.resource_prefix, &config.id)?))
                        .build()
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: "Invalid EventBridge rule request".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create EventBridge rule '{}'", rule_name),
                    resource_id: Some(config.id.clone()),
                })?;

                let rule_arn = rule_response.rule_arn.unwrap_or_default();

                // Add Lambda permission for EventBridge
                let statement_id = format!("{}-eb-{}", worker_name, index);
                let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
                let permission_request = AddPermissionInput::builder()
                    .function_name(worker_name)
                    .statement_id(statement_id.clone())
                    .action("lambda:InvokeFunction")
                    .principal("events.amazonaws.com")
                    .source_arn(rule_arn)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Invalid Lambda EventBridge permission request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;

                match add_lambda_permission(&lambda_client, permission_request).await {
                    Ok(_) => {}
                    Err(e) if is_remote_resource_conflict(&e) => {
                        info!(
                            worker=%config.id,
                            statement_id=%statement_id,
                            "EventBridge invoke permission already exists; treating as created"
                        );
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to add EventBridge invoke permission for rule '{}'",
                                rule_name
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }

                // Add Lambda as the target of the rule
                put_eventbridge_targets(
                    &eventbridge_client,
                    PutTargetsInput::builder()
                        .rule(rule_name.clone())
                        .targets(
                            EventBridgeTarget::builder()
                                .id("1")
                                .arn(function_arn)
                                .build()
                                .into_alien_error()
                                .context(ErrorData::CloudPlatformError {
                                    message: "Invalid EventBridge target".to_string(),
                                    resource_id: Some(config.id.clone()),
                                })?,
                        )
                        .build()
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: "Invalid EventBridge targets request".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to add target to EventBridge rule '{}'", rule_name),
                    resource_id: Some(config.id.clone()),
                })?;

                if !self.eventbridge_rule_names.contains(&rule_name) {
                    self.eventbridge_rule_names.push(rule_name.clone());
                }
                if !self
                    .eventbridge_permission_statement_ids
                    .contains(&statement_id)
                {
                    self.eventbridge_permission_statement_ids
                        .push(statement_id.clone());
                }
                info!(
                    worker=%config.id,
                    rule_name=%rule_name,
                    statement_id=%statement_id,
                    "EventBridge schedule trigger created"
                );
            }
        }

        // Continue to concurrency configuration (linear flow)
        Ok(HandlerAction::Continue {
            state: SettingConcurrency,
            suggested_delay: None,
        })
    }

    #[handler(
        state = SettingConcurrency,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn setting_concurrency(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &config.id);

        if let Some(limit) = config.concurrency_limit {
            info!(worker=%config.id, limit=%limit, "Setting reserved concurrency on worker");
            put_lambda_function_concurrency(&client, &aws_worker_name, limit)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to set worker reserved concurrency".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
        } else {
            debug!(worker=%config.id, "No concurrency limit configured, skipping");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);

        // Heartbeat check: verify worker still exists and is in correct state
        let function_info = get_lambda_function_configuration(&client, &aws_worker_name, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get worker configuration during heartbeat check".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // Verify worker is in active state - drift is non-retryable
        if function_info.state.as_ref() != Some(&LambdaState::Active) {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: worker_config.id.clone(),
                message: format!(
                    "Worker state is '{}', expected 'Active'",
                    lambda_state_as_str(&function_info.state)
                ),
            }));
        }

        // Check if certificate was renewed (for public workers with auto-managed domains)
        if worker_config.ingress == Ingress::Public {
            if let Some(domain_metadata) = &ctx.deployment_config.domain_metadata {
                if let Some(resource_info) = domain_metadata.resources.get(&worker_config.id) {
                    if let Some(new_issued_at) = &resource_info.issued_at {
                        match &self.certificate_issued_at {
                            Some(stored) if new_issued_at != stored => {
                                // Certificate renewed! Trigger update flow to re-import
                                info!(
                                    name = %worker_config.id,
                                    old_issued_at = %stored,
                                    new_issued_at = %new_issued_at,
                                    "Certificate renewed, triggering update"
                                );
                                return Ok(HandlerAction::Continue {
                                    state: UpdateImportingCertificate,
                                    suggested_delay: None,
                                });
                            }
                            None => {
                                // First heartbeat after deployment, store the timestamp
                                self.certificate_issued_at = Some(new_issued_at.clone());
                            }
                            _ => {} // Same timestamp, no renewal
                        }
                    }
                }
            }
        }

        emit_aws_lambda_worker_heartbeat(ctx, &worker_config, &aws_worker_name, &function_info);

        debug!(name = %worker_config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateImportingCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if worker_config.ingress != Ingress::Public || self.uses_custom_domain {
            return Ok(HandlerAction::Continue {
                state: UpdateCodeStart,
                suggested_delay: None,
            });
        }

        let Some(resource) = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id))
        else {
            return Ok(HandlerAction::Continue {
                state: UpdateCodeStart,
                suggested_delay: None,
            });
        };

        if resource.issued_at == self.certificate_issued_at {
            return Ok(HandlerAction::Continue {
                state: UpdateCodeStart,
                suggested_delay: None,
            });
        }

        let Some(certificate_arn) = self.certificate_arn.clone() else {
            return Ok(HandlerAction::Continue {
                state: UpdateCodeStart,
                suggested_delay: None,
            });
        };
        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let (leaf, chain) = split_certificate_chain(certificate_chain);
        let aws_cfg = ctx.get_aws_config()?;
        let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
        let reimport_request = ImportCertificateInput::builder()
            .certificate_arn(certificate_arn)
            .certificate(Blob::new(leaf.into_bytes()))
            .private_key(Blob::new(private_key.clone().into_bytes()))
            .set_certificate_chain(chain.map(|chain| Blob::new(chain.into_bytes())))
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Invalid ACM certificate reimport request".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
        reimport_acm_certificate(&acm_client, reimport_request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to re-import renewed certificate to ACM".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.certificate_issued_at = resource.issued_at.clone();

        Ok(HandlerAction::Continue {
            state: UpdateCodeStart,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateCodeStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_code_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;
        let code_changed = current_config.code != previous_config.code;

        // UpdateCodeStart only handles code updates if needed
        if code_changed {
            let aws_cfg = ctx.get_aws_config()?;
            let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

            let image_uri = match &current_config.code {
                alien_core::WorkerCode::Image { image } => image.clone(),
                alien_core::WorkerCode::Source { .. } => {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Worker is configured with source code for update, but only pre-built images are supported".to_string(),
                        resource_id: Some(current_config.id.clone()),
                    }));
                }
            };

            // Resolve proxy URIs to native ECR URIs.
            let image_uri = if let Some(ref native_host) = ctx.deployment_config.native_image_host {
                alien_core::image_rewrite::resolve_native_image_uri(&image_uri, native_host)
                    .unwrap_or(image_uri)
            } else {
                image_uri
            };

            let image_uri = Self::rewrite_ecr_region_if_needed(&image_uri, &aws_cfg.region);

            let arn = self.arn.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Worker ARN not available for code update".to_string(),
                    resource_id: Some(current_config.id.clone()),
                })
            })?;

            let request = UpdateFunctionCodeInput::builder()
                .function_name(arn.clone())
                .image_uri(image_uri)
                .publish(true)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Invalid Lambda code update request".to_string(),
                    resource_id: Some(current_config.id.clone()),
                })?;

            update_lambda_function_code(&client, request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to update Lambda worker code".to_string(),
                    resource_id: Some(current_config.id.clone()),
                })?;
        }

        // Always transition to wait for code update (even if no code change) - linear flow
        Ok(HandlerAction::Continue {
            state: UpdateCodeWaitForActive,
            suggested_delay: Some(Duration::from_secs(3)),
        })
    }

    #[handler(
        state = UpdateCodeWaitForActive,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_code_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &current_config.id);
        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for code status check".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            })
        })?;
        let result = get_lambda_function_configuration(&client, arn, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get worker configuration for code update".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            })?;

        let is_active = result.state.as_ref() == Some(&LambdaState::Active);
        let is_successful =
            result.last_update_status.as_ref() == Some(&LambdaLastUpdateStatus::Successful);

        if is_active && is_successful {
            // Always proceed to config update phase - linear flow
            Ok(HandlerAction::Continue {
                state: UpdateConfigStart,
                suggested_delay: None,
            })
        } else if result.state.as_ref() == Some(&LambdaState::Pending)
            || result.last_update_status.as_ref() == Some(&LambdaLastUpdateStatus::InProgress)
        {
            Ok(HandlerAction::Stay {
                max_times: 20,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Code update failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status,
                ),
                resource_id: Some(aws_worker_name),
            }))
        }
    }

    #[handler(
        state = UpdateConfigStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_config_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;
        let config_changed = current_config.permissions != previous_config.permissions
            || current_config.memory_mb != previous_config.memory_mb
            || current_config.timeout_seconds != previous_config.timeout_seconds
            || current_config.environment != previous_config.environment
            || current_config.links != previous_config.links;

        if !config_changed {
            return Ok(HandlerAction::Continue {
                state: UpdateConfigWaitForActive,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &current_config.id);

        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", current_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        // Get the ServiceAccount's role ARN
        let service_account_state = ctx
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                &service_account_ref,
            )?;

        let role_arn = service_account_state
            .role_arn
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: current_config.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        let final_env_vars = self
            .prepare_environment_variables(
                &current_config.environment,
                &current_config.links,
                ctx,
                &aws_worker_name,
            )
            .await?;

        let lambda_environment = if !final_env_vars.is_empty() {
            Some(
                Environment::builder()
                    .set_variables(Some(final_env_vars))
                    .build(),
            )
        } else {
            None
        };

        // Get VPC configuration if a Network resource exists
        let vpc_config = self.get_vpc_config(ctx)?;

        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for config update".to_string(),
                resource_id: Some(current_config.id.clone()),
            })
        })?;

        let request = UpdateFunctionConfigurationInput::builder()
            .function_name(arn.clone())
            .role(role_arn)
            .timeout(current_config.timeout_seconds as i32)
            .memory_size(current_config.memory_mb as i32)
            .set_environment(lambda_environment)
            .set_vpc_config(vpc_config)
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Invalid Lambda configuration update request".to_string(),
                resource_id: Some(current_config.id.clone()),
            })?;

        update_lambda_function_configuration(&client, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update Lambda worker configuration".to_string(),
                resource_id: Some(current_config.id.clone()),
            })?;

        // Always transition to wait state - linear flow
        Ok(HandlerAction::Continue {
            state: UpdateConfigWaitForActive,
            suggested_delay: Some(Duration::from_secs(3)),
        })
    }

    #[handler(
        state = UpdateConfigWaitForActive,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_config_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &current_config.id);
        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for config status check".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            })
        })?;
        let result = get_lambda_function_configuration(&client, arn, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get worker configuration for config update".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            })?;

        let is_active = result.state.as_ref() == Some(&LambdaState::Active);
        let is_successful =
            result.last_update_status.as_ref() == Some(&LambdaLastUpdateStatus::Successful);

        if is_active && is_successful {
            Ok(HandlerAction::Continue {
                state: UpdateEnsuringPublicExposure,
                suggested_delay: None,
            })
        } else if result.state.as_ref() == Some(&LambdaState::Pending)
            || result.last_update_status.as_ref() == Some(&LambdaLastUpdateStatus::InProgress)
        {
            Ok(HandlerAction::Stay {
                max_times: 20,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Config update failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status
                ),
                resource_id: Some(aws_worker_name),
            }))
        }
    }

    #[handler(
        state = UpdateEnsuringPublicExposure,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_ensuring_public_exposure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;

        if current_config.ingress != Ingress::Public {
            return Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay: None,
            });
        }

        if previous_config.ingress != Ingress::Public && self.api_id.is_none() {
            self.url = None;
        }

        let has_domain_info = self.ensure_domain_info(ctx, &current_config.id)?;
        if self.api_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay: None,
            });
        }

        let next_state = if has_domain_info {
            UpdateWaitingForCertificate
        } else {
            UpdateCreatingApiGateway
        };

        Ok(HandlerAction::Continue {
            state: next_state,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = UpdateWaitingForCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_certificate(ctx).await? {
            HandlerAction::Continue {
                state: ImportingCertificate,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateImportingInitialCertificate,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiGateway,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_certificate",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateImportingInitialCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_initial_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.importing_certificate(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiGateway,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "importing_certificate",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingApiGateway,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_gateway(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiIntegration,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiIntegration,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_gateway",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingApiIntegration,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_integration(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_integration(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiRoute,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiRoute,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_integration",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingApiRoute,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_route(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_route(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiStage,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiStage,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_route",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingApiStage,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_stage(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_stage(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiDomain,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiDomain,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateAddingApiGatewayPermission,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_stage",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingApiDomain,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_domain(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiMapping,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiMapping,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_domain",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingApiMapping,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_mapping(ctx).await? {
            HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateAddingApiGatewayPermission,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_mapping",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateAddingApiGatewayPermission,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_adding_api_gateway_permission(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.adding_api_gateway_permission(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForDns,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: ApplyingResourcePermissions,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateApplyingResourcePermissions,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "adding_api_gateway_permission",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForDns,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_dns(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_dns(ctx).await? {
            HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_dns",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateRunningReadinessProbe,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_running_readiness_probe(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Only run readiness probe if configured and we have a URL (for public workers)
        if worker_config.readiness_probe.is_some() && worker_config.ingress == Ingress::Public {
            if let Some(url) = &self.url {
                let dns_override = readiness_probe_dns_override(
                    url,
                    self.fqdn.as_deref(),
                    self.load_balancer.as_ref(),
                );

                match run_readiness_probe_with_dns_override(ctx, url, dns_override).await {
                    Ok(()) => {
                        // Probe succeeded, proceed to Ready
                    }
                    Err(_) => {
                        // Probe failed, let the framework handle retries
                        return Ok(HandlerAction::Stay {
                            max_times: READINESS_PROBE_MAX_ATTEMPTS,
                            suggested_delay: Some(Duration::from_secs(5)),
                        });
                    }
                }
            }
        }

        // Either no readiness probe needed, or probe succeeded.
        Ok(HandlerAction::Continue {
            state: UpdateApplyingResourcePermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateApplyingResourcePermissions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.applying_resource_permissions(ctx).await? {
            HandlerAction::Continue {
                state: UpdatingEnvVarsWithSelfBinding,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateEnvVarsWithSelfBinding,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "applying_resource_permissions",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateEnvVarsWithSelfBinding,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_env_vars_with_self_binding(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.updating_env_vars_with_self_binding(ctx).await? {
            HandlerAction::Continue {
                state: CreatingEventSourceMappings,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateEventSourceMappings,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "updating_env_vars_with_self_binding",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateEventSourceMappings,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_event_source_mappings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;

        // Validation: Only allow at most one queue trigger per worker (non-retriable error)
        let queue_trigger_count = current_config
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    current_config.id, queue_trigger_count
                ),
                resource_id: Some(current_config.id.clone()),
            }));
        }

        // Check if triggers have changed
        let triggers_changed = current_config.triggers != previous_config.triggers;

        if triggers_changed {
            info!(worker=%current_config.id, "Worker triggers changed, updating event source mappings");

            // For simplicity, we'll delete old mappings and create new ones
            // In a production system, you might want to do a more sophisticated diff
            let aws_cfg = ctx.get_aws_config()?;
            let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

            // Delete existing mappings
            for uuid in &self.event_source_mappings.clone() {
                match delete_lambda_event_source_mapping(&client, uuid).await {
                    Ok(_) => {
                        info!(worker=%current_config.id, uuid=%uuid, "Deleted existing event source mapping");
                    }
                    Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                        info!(worker=%current_config.id, uuid=%uuid, "Event source mapping was already deleted");
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete existing event source mapping '{}'",
                                uuid
                            ),
                            resource_id: Some(current_config.id.clone()),
                        }));
                    }
                }
            }
            self.event_source_mappings.clear();

            // Create new mappings for ALL queue triggers
            for trigger in &current_config.triggers {
                if let alien_core::WorkerTrigger::Queue { queue } = trigger {
                    self.create_queue_event_source_mapping(ctx, aws_cfg, &current_config, queue)
                        .await?;
                }
            }

            // Clean up old S3 storage trigger notifications
            for trigger in &previous_config.triggers {
                if let alien_core::WorkerTrigger::Storage {
                    storage: storage_ref,
                    ..
                } = trigger
                {
                    if let Ok(storage_controller) =
                        ctx.require_dependency::<crate::storage::AwsStorageController>(storage_ref)
                    {
                        if let Some(bucket_name) = storage_controller.bucket_name.as_deref() {
                            let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                            let empty_config = NotificationConfiguration::builder().build();
                            if let Err(e) = put_s3_bucket_notification_configuration(
                                &s3_client,
                                bucket_name,
                                &empty_config,
                            )
                            .await
                            {
                                warn!(
                                    worker=%current_config.id,
                                    bucket=%bucket_name,
                                    error=%e,
                                    "Failed to clear old S3 notification configuration (best-effort)"
                                );
                            }
                        }
                    }
                }
            }
            self.s3_permission_statement_ids.clear();

            // Clean up old EventBridge schedule triggers
            if !self.eventbridge_rule_names.is_empty() {
                let eventbridge_client = ctx
                    .service_provider
                    .get_aws_eventbridge_client(aws_cfg)
                    .await?;

                for rule_name in &self.eventbridge_rule_names.clone() {
                    if let Err(e) = remove_eventbridge_targets(
                        &eventbridge_client,
                        rule_name,
                        vec!["1".to_string()],
                    )
                    .await
                    {
                        warn!(
                            worker=%current_config.id,
                            rule=%rule_name,
                            error=%e,
                            "Failed to remove targets from old EventBridge rule (best-effort)"
                        );
                    }
                    if let Err(e) = eventbridge_result(
                        eventbridge_client
                            .delete_rule()
                            .name(rule_name)
                            .send()
                            .await,
                        "DeleteRule",
                        "EventBridgeRule",
                        rule_name,
                    ) {
                        warn!(
                            worker=%current_config.id,
                            rule=%rule_name,
                            error=%e,
                            "Failed to delete old EventBridge rule (best-effort)"
                        );
                    }
                }
                self.eventbridge_rule_names.clear();
                self.eventbridge_permission_statement_ids.clear();
            }

            // Recreate storage triggers
            let worker_name = self.worker_name.as_deref().unwrap_or("unknown");
            let function_arn = self.arn.as_deref().unwrap_or("unknown");

            for trigger in &current_config.triggers {
                if let alien_core::WorkerTrigger::Storage {
                    storage: storage_ref,
                    events,
                } = trigger
                {
                    let storage_controller = ctx
                        .require_dependency::<crate::storage::AwsStorageController>(storage_ref)?;
                    let bucket_name =
                        storage_controller.bucket_name.as_deref().ok_or_else(|| {
                            AlienError::new(ErrorData::DependencyNotReady {
                                resource_id: current_config.id.clone(),
                                dependency_id: storage_ref.id.clone(),
                            })
                        })?;

                    let statement_id = format!("{}-s3-{}", worker_name, storage_ref.id);
                    let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
                    let permission_request = AddPermissionInput::builder()
                        .function_name(worker_name)
                        .statement_id(statement_id.clone())
                        .action("lambda:InvokeFunction")
                        .principal("s3.amazonaws.com")
                        .source_arn(format!("arn:aws:s3:::{}", bucket_name))
                        .build()
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: "Invalid Lambda S3 permission request".to_string(),
                            resource_id: Some(current_config.id.clone()),
                        })?;

                    match add_lambda_permission(&lambda_client, permission_request).await {
                        Ok(_) => {}
                        Err(e) if is_remote_resource_conflict(&e) => {
                            info!(
                                worker=%current_config.id,
                                statement_id=%statement_id,
                                "S3 invoke permission already exists; treating as created"
                            );
                        }
                        Err(e) => {
                            return Err(e.context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to add S3 invoke permission for storage '{}'",
                                    storage_ref.id
                                ),
                                resource_id: Some(current_config.id.clone()),
                            }));
                        }
                    }

                    let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                    let mut notification_config =
                        get_s3_bucket_notification_configuration(&s3_client, bucket_name)
                            .await
                            .context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to get notification configuration for bucket '{}'",
                                    bucket_name
                                ),
                                resource_id: Some(current_config.id.clone()),
                            })?;

                    replace_lambda_notification_config(
                        &mut notification_config,
                        s3_lambda_notification_config(
                            &statement_id,
                            function_arn,
                            events,
                            &current_config.id,
                        )?,
                    );

                    put_s3_bucket_notification_configuration(
                        &s3_client,
                        bucket_name,
                        &notification_config,
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to put notification configuration for bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(current_config.id.clone()),
                    })?;

                    if !self.s3_permission_statement_ids.contains(&statement_id) {
                        self.s3_permission_statement_ids.push(statement_id);
                    }
                }
            }

            // Recreate schedule triggers
            for (index, trigger) in current_config.triggers.iter().enumerate() {
                if let alien_core::WorkerTrigger::Schedule { cron } = trigger {
                    let rule_name = format!("{}-cron-{}", worker_name, index);
                    let schedule_expression =
                        crate::worker::crontab_to_eventbridge::crontab_to_eventbridge(cron)
                            .map_err(|e| {
                                AlienError::new(ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid cron expression '{}': {}", cron, e),
                                    resource_id: Some(current_config.id.clone()),
                                })
                            })?;

                    let eventbridge_client = ctx
                        .service_provider
                        .get_aws_eventbridge_client(aws_cfg)
                        .await?;

                    let rule_response = put_eventbridge_rule(
                        &eventbridge_client,
                        PutRuleInput::builder()
                            .name(rule_name.clone())
                            .schedule_expression(schedule_expression)
                            .state(RuleState::Enabled)
                            .description(format!(
                                "Alien schedule trigger for worker '{}'",
                                current_config.id
                            ))
                            .set_tags(Some(eventbridge_tags(
                                ctx.resource_prefix,
                                &current_config.id,
                            )?))
                            .build()
                            .into_alien_error()
                            .context(ErrorData::CloudPlatformError {
                                message: "Invalid EventBridge rule request".to_string(),
                                resource_id: Some(current_config.id.clone()),
                            })?,
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create EventBridge rule '{}'", rule_name),
                        resource_id: Some(current_config.id.clone()),
                    })?;

                    let rule_arn = rule_response.rule_arn.unwrap_or_default();
                    let statement_id = format!("{}-eb-{}", worker_name, index);
                    let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
                    let permission_request = AddPermissionInput::builder()
                        .function_name(worker_name)
                        .statement_id(statement_id.clone())
                        .action("lambda:InvokeFunction")
                        .principal("events.amazonaws.com")
                        .source_arn(rule_arn)
                        .build()
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: "Invalid Lambda EventBridge permission request".to_string(),
                            resource_id: Some(current_config.id.clone()),
                        })?;

                    match add_lambda_permission(&lambda_client, permission_request).await {
                        Ok(_) => {}
                        Err(e) if is_remote_resource_conflict(&e) => {
                            info!(
                                worker=%current_config.id,
                                statement_id=%statement_id,
                                "EventBridge invoke permission already exists; treating as created"
                            );
                        }
                        Err(e) => {
                            return Err(e.context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to add EventBridge invoke permission for rule '{}'",
                                    rule_name
                                ),
                                resource_id: Some(current_config.id.clone()),
                            }));
                        }
                    }

                    put_eventbridge_targets(
                        &eventbridge_client,
                        PutTargetsInput::builder()
                            .rule(rule_name.clone())
                            .targets(
                                EventBridgeTarget::builder()
                                    .id("1")
                                    .arn(function_arn)
                                    .build()
                                    .into_alien_error()
                                    .context(ErrorData::CloudPlatformError {
                                        message: "Invalid EventBridge target".to_string(),
                                        resource_id: Some(current_config.id.clone()),
                                    })?,
                            )
                            .build()
                            .into_alien_error()
                            .context(ErrorData::CloudPlatformError {
                                message: "Invalid EventBridge targets request".to_string(),
                                resource_id: Some(current_config.id.clone()),
                            })?,
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to add target to EventBridge rule '{}'",
                            rule_name
                        ),
                        resource_id: Some(current_config.id.clone()),
                    })?;

                    if !self.eventbridge_rule_names.contains(&rule_name) {
                        self.eventbridge_rule_names.push(rule_name);
                    }
                    if !self
                        .eventbridge_permission_statement_ids
                        .contains(&statement_id)
                    {
                        self.eventbridge_permission_statement_ids.push(statement_id);
                    }
                }
            }
        } else {
            info!(worker=%current_config.id, "No trigger changes detected");
        }

        Ok(HandlerAction::Continue {
            state: UpdatingConcurrency,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingConcurrency,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_concurrency(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let prev_config = ctx.previous_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &config.id);

        if config.concurrency_limit != prev_config.concurrency_limit {
            match config.concurrency_limit {
                Some(limit) => {
                    info!(worker=%config.id, limit=%limit, "Updating reserved concurrency on worker");
                    put_lambda_function_concurrency(&client, &aws_worker_name, limit)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to update worker reserved concurrency".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;
                }
                None => {
                    info!(worker=%config.id, "Removing reserved concurrency from worker");
                    delete_lambda_function_concurrency(&client, &aws_worker_name)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to remove worker reserved concurrency".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;
                }
            }
        } else {
            debug!(worker=%config.id, "Concurrency limit unchanged, skipping");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.url = None;
        Ok(HandlerAction::Continue {
            state: DeletingApiGateway,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingApiGateway,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_api_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Ordering matters: delete API mapping before domain name, domain name before API.
        if let (Some(domain_name), Some(api_mapping_id)) =
            (self.domain_name.as_ref(), self.api_mapping_id.as_ref())
        {
            let client = ctx
                .service_provider
                .get_aws_apigatewayv2_client(aws_cfg)
                .await?;
            match delete_api_gateway_mapping(&client, domain_name, api_mapping_id).await {
                Ok(()) => info!(worker=%worker_config.id, "API mapping deleted"),
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    info!(worker=%worker_config.id, "API mapping already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete API mapping".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }
        self.api_mapping_id = None;

        if let Some(domain_name) = self.domain_name.as_ref() {
            let client = ctx
                .service_provider
                .get_aws_apigatewayv2_client(aws_cfg)
                .await?;
            match api_gateway_v2_result(
                client
                    .delete_domain_name()
                    .domain_name(domain_name)
                    .send()
                    .await,
                "DeleteDomainName",
                "ApiGatewayDomainName",
                domain_name,
            ) {
                Ok(_) => {
                    info!(worker=%worker_config.id, domain=%domain_name, "Custom domain deleted")
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    info!(worker=%worker_config.id, "Custom domain already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete custom domain".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }
        self.domain_name = None;

        // Deleting the API cascades to routes, integrations, and stages.
        if let Some(api_id) = self.api_id.as_ref() {
            let client = ctx
                .service_provider
                .get_aws_apigatewayv2_client(aws_cfg)
                .await?;
            match api_gateway_v2_result(
                client.delete_api().api_id(api_id).send().await,
                "DeleteApi",
                "ApiGatewayApi",
                api_id,
            ) {
                Ok(_) => {
                    info!(worker=%worker_config.id, api_id=%api_id, "API Gateway deleted")
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    info!(worker=%worker_config.id, "API Gateway already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete API Gateway".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }
        self.api_id = None;
        self.integration_id = None;
        self.route_id = None;
        self.stage_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingEventSourceMappings,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingEventSourceMappings,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_event_source_mappings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Linear flow principle: Always perform this state, even if no event source mappings
        if !self.event_source_mappings.is_empty() {
            info!(worker=%worker_config.id, mappings=?self.event_source_mappings, "Deleting event source mappings");

            // Delete all event source mappings using best-effort approach (ignore NotFound)
            for uuid in &self.event_source_mappings.clone() {
                match delete_lambda_event_source_mapping(&client, uuid).await {
                    Ok(_) => {
                        info!(worker=%worker_config.id, uuid=%uuid, "Event source mapping deleted successfully");
                    }
                    Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                        info!(worker=%worker_config.id, uuid=%uuid, "Event source mapping was already deleted (not found)");
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete event source mapping '{}'", uuid),
                            resource_id: Some(worker_config.id.clone()),
                        }));
                    }
                }
            }

            // Clear the mapping list after successful deletion
            self.event_source_mappings.clear();
        } else {
            info!(worker=%worker_config.id, "No event source mappings to delete");
        }

        // Clean up S3 storage trigger notifications (best-effort)
        if !self.s3_permission_statement_ids.is_empty() {
            info!(worker=%worker_config.id, "Cleaning up S3 storage trigger notifications");

            // Best-effort: put empty notification configuration on any referenced buckets
            // We don't track which bucket each statement_id maps to, so we attempt to
            // clean up by iterating over storage triggers from the config
            for trigger in &worker_config.triggers {
                if let alien_core::WorkerTrigger::Storage {
                    storage: storage_ref,
                    ..
                } = trigger
                {
                    if let Ok(storage_controller) =
                        ctx.require_dependency::<crate::storage::AwsStorageController>(storage_ref)
                    {
                        if let Some(bucket_name) = storage_controller.bucket_name.as_deref() {
                            let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                            let empty_config = NotificationConfiguration::builder().build();
                            if let Err(e) = put_s3_bucket_notification_configuration(
                                &s3_client,
                                bucket_name,
                                &empty_config,
                            )
                            .await
                            {
                                warn!(
                                    worker=%worker_config.id,
                                    bucket=%bucket_name,
                                    error=%e,
                                    "Failed to clear S3 notification configuration (best-effort)"
                                );
                            } else {
                                info!(
                                    worker=%worker_config.id,
                                    bucket=%bucket_name,
                                    "S3 notification configuration cleared"
                                );
                            }
                        }
                    }
                }
            }
            self.s3_permission_statement_ids.clear();
        }

        // Always continue to DeletingScheduleTriggers state (linear flow)
        Ok(HandlerAction::Continue {
            state: DeletingScheduleTriggers,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingScheduleTriggers,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_schedule_triggers(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;

        // Delete EventBridge rules and their targets (best-effort)
        if !self.eventbridge_rule_names.is_empty() {
            info!(
                worker=%worker_config.id,
                rules=?self.eventbridge_rule_names,
                "Deleting EventBridge schedule triggers"
            );

            let eventbridge_client = ctx
                .service_provider
                .get_aws_eventbridge_client(aws_cfg)
                .await?;

            for rule_name in &self.eventbridge_rule_names.clone() {
                // Remove targets first (required before deleting rule)
                if let Err(e) = remove_eventbridge_targets(
                    &eventbridge_client,
                    rule_name,
                    vec!["1".to_string()],
                )
                .await
                {
                    warn!(
                        worker=%worker_config.id,
                        rule=%rule_name,
                        error=%e,
                        "Failed to remove targets from EventBridge rule (best-effort)"
                    );
                } else {
                    info!(worker=%worker_config.id, rule=%rule_name, "EventBridge rule targets removed");
                }

                // Delete the rule
                if let Err(e) = eventbridge_result(
                    eventbridge_client
                        .delete_rule()
                        .name(rule_name)
                        .send()
                        .await,
                    "DeleteRule",
                    "EventBridgeRule",
                    rule_name,
                ) {
                    warn!(
                        worker=%worker_config.id,
                        rule=%rule_name,
                        error=%e,
                        "Failed to delete EventBridge rule (best-effort)"
                    );
                } else {
                    info!(worker=%worker_config.id, rule=%rule_name, "EventBridge rule deleted");
                }
            }
            self.eventbridge_rule_names.clear();
        }

        // Clear EventBridge permission statement IDs
        // (Lambda permissions are removed when the worker is deleted)
        self.eventbridge_permission_statement_ids.clear();

        // Detach the Lambda from the VPC before deleting it. AWS otherwise
        // keeps Lambda-managed ENIs around after function deletion, which can
        // block Terraform/CloudFormation from deleting customer-owned subnets.
        Ok(HandlerAction::Continue {
            state: DetachingVpcConfig,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DetachingVpcConfig,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn detaching_vpc_config(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);

        if self.get_vpc_config(ctx)?.is_none() {
            return Ok(HandlerAction::Continue {
                state: DeletingWorker,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let function_identifier = self.arn.as_deref().unwrap_or(&aws_worker_name);
        let request = UpdateFunctionConfigurationInput::builder()
            .function_name(function_identifier)
            .vpc_config(
                VpcConfig::builder()
                    .set_subnet_ids(Some(Vec::new()))
                    .set_security_group_ids(Some(Vec::new()))
                    .build(),
            )
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Invalid Lambda VPC detach request".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        match update_lambda_function_configuration(&client, request).await {
            Ok(_) => {
                info!(worker=%worker_config.id, "Lambda VPC config detach requested");
                Ok(HandlerAction::Continue {
                    state: DetachVpcWaitForActive,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                info!(worker=%worker_config.id, "Lambda already gone while detaching VPC config");
                Ok(HandlerAction::Continue {
                    state: DeletingWorker,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to detach Lambda worker from VPC".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })),
        }
    }

    #[handler(
        state = DetachVpcWaitForActive,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn detach_vpc_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let function_identifier = self.arn.as_deref().unwrap_or(&aws_worker_name);
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

        match get_lambda_function_configuration(&client, function_identifier, None).await {
            Ok(result)
                if result.state.as_ref() == Some(&LambdaState::Active)
                    && result.last_update_status.as_ref()
                        == Some(&LambdaLastUpdateStatus::Successful) =>
            {
                Ok(HandlerAction::Continue {
                    state: DeletingWorker,
                    suggested_delay: None,
                })
            }
            Ok(result)
                if result.state.as_ref() == Some(&LambdaState::Pending)
                    || result.last_update_status.as_ref()
                        == Some(&LambdaLastUpdateStatus::InProgress) =>
            {
                Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
            Ok(result) => Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Lambda VPC detach failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status
                ),
                resource_id: Some(worker_config.id.clone()),
            })),
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                Ok(HandlerAction::Continue {
                    state: DeletingWorker,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to check Lambda VPC detach status".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })),
        }
    }

    #[handler(
        state = DeletingWorker,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_function(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        info!(name=%aws_worker_name, "Deleting worker itself: {}", aws_worker_name);

        match delete_lambda_function(&client, &aws_worker_name, None).await {
            Ok(_) => {
                info!(name=%aws_worker_name, "Worker deleted successfully, proceeding to DeleteWaitForNotFound state");
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                warn!(name=%aws_worker_name, "Worker was already deleted (not found), proceeding to DeleteWaitForNotFound state");
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete Lambda worker".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: DeleteWaitForNotFound,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeleteWaitForNotFound,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_wait_for_not_found(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let arn = self.arn.as_ref();
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let lookup_identifier = arn.map(|a| a.as_str()).unwrap_or(&aws_worker_name);

        match get_lambda_function_configuration(&client, lookup_identifier, None).await {
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                self.arn = None;
                self.url = None;
                self.worker_name = None;
                self.event_source_mappings.clear();
                if Self::should_wait_for_lambda_vpc_enis(ctx) {
                    Ok(HandlerAction::Continue {
                        state: WaitingForVpcEnisReleased,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                } else {
                    Ok(HandlerAction::Continue {
                        state: DeletingCertificate,
                        suggested_delay: None,
                    })
                }
            }
            Ok(_) => Ok(HandlerAction::Stay {
                max_times: 10,
                suggested_delay: Some(Duration::from_secs(10)),
            }),
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to check worker deletion status".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })),
        }
    }

    #[handler(
        state = WaitingForVpcEnisReleased,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_vpc_enis_released(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !Self::should_wait_for_lambda_vpc_enis(ctx) {
            info!(
                worker=%worker_config.id,
                "Skipping Lambda VPC network interface wait for externally managed network"
            );
            return Ok(HandlerAction::Continue {
                state: DeletingCertificate,
                suggested_delay: None,
            });
        }

        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let result = describe_ec2_network_interfaces(
            &client,
            DescribeNetworkInterfacesInput::builder()
                .set_filters(Some(vec![Filter::builder()
                    .name("description")
                    .values(format!("AWS Lambda VPC ENI-{}*", aws_worker_name))
                    .build()]))
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build Lambda VPC network interface lookup request"
                        .to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to check Lambda VPC network interface cleanup".to_string(),
            resource_id: Some(worker_config.id.clone()),
        })?;

        let network_interfaces = result.network_interfaces();

        if network_interfaces.is_empty() {
            return Ok(HandlerAction::Continue {
                state: DeletingCertificate,
                suggested_delay: None,
            });
        }

        let network_interface_ids = network_interfaces
            .iter()
            .filter_map(|eni| eni.network_interface_id())
            .collect::<Vec<_>>();

        info!(
            worker=%worker_config.id,
            network_interfaces=?network_interface_ids,
            "Waiting for Lambda VPC network interfaces to be released"
        );

        Ok(HandlerAction::Stay {
            max_times: 90,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = DeletingCertificate,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if let Some(certificate_arn) = self.certificate_arn.as_ref() {
            let aws_cfg = ctx.get_aws_config()?;
            let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
            match acm_result(
                acm_client
                    .delete_certificate()
                    .certificate_arn(certificate_arn)
                    .send()
                    .await,
                "DeleteCertificate",
                "Certificate",
                certificate_arn,
            ) {
                Ok(_) => info!(worker=%worker_config.id, "ACM certificate deleted"),
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    info!(worker=%worker_config.id, "ACM certificate already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete ACM certificate".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }
        self.certificate_arn = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.arn.as_ref().map(|arn| {
            // Map the load balancer endpoint for DNS management
            let load_balancer_endpoint = self
                .load_balancer
                .as_ref()
                .and_then(|lb| lb.endpoint.as_ref())
                .map(|endpoint| alien_core::LoadBalancerEndpoint {
                    dns_name: endpoint.dns_name.clone(),
                    hosted_zone_id: Some(endpoint.hosted_zone_id.clone()),
                });

            ResourceOutputs::new(WorkerOutputs {
                worker_name: self
                    .worker_name
                    .clone()
                    .unwrap_or_else(|| "worker-name-placeholder".to_string()),
                url: self.url.clone(),
                identifier: Some(arn.clone()),
                load_balancer_endpoint,
                commands_push_target: self.worker_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, LambdaWorkerBinding, WorkerBinding};

        if let (Some(worker_name), Some(arn)) = (&self.worker_name, &self.arn) {
            // Extract region from ARN: arn:aws:lambda:us-east-1:123456789:function:name
            let region = arn.split(':').nth(3).unwrap_or("us-east-1").to_string();

            let binding = WorkerBinding::Lambda(LambdaWorkerBinding {
                worker_name: BindingValue::Value(worker_name.clone()),
                region: BindingValue::Value(region),
                url: self.url.as_ref().map(|u| BindingValue::Value(u.clone())),
            });
            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

// Separate impl block for helper methods
impl AwsWorkerController {
    /// Rewrite an ECR image URI to use the given region if it points to a different one.
    ///
    /// Lambda requires container images in the same region as the worker.
    /// When the management account's ECR is in a different region and private
    /// image replication copies images to the target region, the image URI must
    /// reference the replicated copy.
    ///
    /// Only rewrites URIs matching the ECR format: `{account}.dkr.ecr.{region}.amazonaws.com/...`
    fn rewrite_ecr_region_if_needed(image_uri: &str, target_region: &str) -> String {
        // ECR URI format: {account_id}.dkr.ecr.{region}.amazonaws.com/{repo}:{tag}
        let Some(host_end) = image_uri.find('/') else {
            return image_uri.to_string();
        };
        let host = &image_uri[..host_end];
        let parts: Vec<&str> = host.split('.').collect();
        // parts: [account_id, "dkr", "ecr", region, "amazonaws", "com"]
        if parts.len() >= 6
            && parts[1] == "dkr"
            && parts[2] == "ecr"
            && parts[4] == "amazonaws"
            && parts[3] != target_region
        {
            let new_host = format!("{}.dkr.ecr.{}.amazonaws.com", parts[0], target_region);
            format!("{}{}", new_host, &image_uri[host_end..])
        } else {
            image_uri.to_string()
        }
    }

    /// Creates an SQS event source mapping for a queue trigger
    async fn create_queue_event_source_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        aws_cfg: &AwsClientConfig,
        worker_config: &alien_core::Worker,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<()> {
        let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

        // Get queue controller to access outputs
        let queue_controller =
            ctx.require_dependency::<crate::queue::aws::AwsQueueController>(queue_ref)?;
        let queue_outputs_wrapper = queue_controller.get_outputs().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;
        let queue_outputs = queue_outputs_wrapper
            .downcast_ref::<alien_core::QueueOutputs>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Invalid queue outputs type".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;

        // Extract queue name from the queue URL
        let queue_name = if let Some(url) = &queue_outputs.identifier {
            // SQS URL format: https://sqs.region.amazonaws.com/account-id/queue-name
            url.split('/')
                .last()
                .unwrap_or(&queue_outputs.queue_name)
                .to_string()
        } else {
            queue_outputs.queue_name.clone()
        };

        // Construct SQS queue ARN: arn:aws:sqs:region:account-id:queue-name
        let queue_arn = format!(
            "arn:aws:sqs:{}:{}:{}",
            aws_cfg.region, aws_cfg.account_id, queue_name
        );

        info!(
            worker=%worker_config.id,
            queue_arn=%queue_arn,
            "Creating SQS event source mapping"
        );

        let worker_name = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for event source mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let list_request = ListEventSourceMappingsInput::builder()
            .event_source_arn(queue_arn.clone())
            .function_name(worker_name.clone())
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Invalid Lambda event source mapping list request".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        let existing_mappings = list_lambda_event_source_mappings(&lambda_client, list_request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to list event source mappings for queue '{}'",
                    queue_name
                ),
                resource_id: Some(worker_config.id.clone()),
            })?;

        if let Some(existing_mapping) = existing_mappings
            .event_source_mappings
            .unwrap_or_default()
            .into_iter()
            .find(|mapping| {
                mapping.event_source_arn.as_deref() == Some(queue_arn.as_str())
                    && mapping.function_arn.as_deref() == Some(worker_name.as_str())
            })
        {
            if let Some(uuid) = existing_mapping.uuid {
                if !self.event_source_mappings.contains(&uuid) {
                    self.event_source_mappings.push(uuid.clone());
                }
                info!(
                    worker=%worker_config.id,
                    queue_arn=%queue_arn,
                    uuid=%uuid,
                    "SQS event source mapping already exists; treating as created"
                );
                return Ok(());
            }
        }

        let request = CreateEventSourceMappingInput::builder()
            .event_source_arn(queue_arn.clone())
            .function_name(worker_name.clone())
            .batch_size(1) // Always 1 message per invocation as per design
            .enabled(true)
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Invalid Lambda event source mapping create request".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        let mapping_uuid = match create_lambda_event_source_mapping(&lambda_client, request).await {
            Ok(response) => response.uuid,
            Err(e) if is_remote_resource_conflict(&e) => {
                let list_request = ListEventSourceMappingsInput::builder()
                    .event_source_arn(queue_arn.clone())
                    .function_name(worker_name.clone())
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Invalid Lambda event source mapping list request".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    })?;

                let existing_mappings =
                    list_lambda_event_source_mappings(&lambda_client, list_request)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                            "Failed to list event source mappings for queue '{}' after conflict",
                            queue_name
                        ),
                            resource_id: Some(worker_config.id.clone()),
                        })?;

                existing_mappings
                    .event_source_mappings
                    .unwrap_or_default()
                    .into_iter()
                    .find(|mapping| {
                        mapping.event_source_arn.as_deref() == Some(queue_arn.as_str())
                            && mapping.function_arn.as_deref() == Some(worker_name.as_str())
                    })
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: format!(
                                "Event source mapping for queue '{}' already exists but could not be found",
                                queue_name
                            ),
                            resource_id: Some(worker_config.id.clone()),
                        })
                    })?
                    .uuid
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create event source mapping for queue '{}'",
                        queue_name
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        };

        if let Some(uuid) = mapping_uuid {
            if !self.event_source_mappings.contains(&uuid) {
                self.event_source_mappings.push(uuid.clone());
            }
            info!(
                worker=%worker_config.id,
                queue_arn=%queue_arn,
                uuid=%uuid,
                "Successfully created SQS event source mapping"
            );
        }

        Ok(())
    }

    // ─────────────── HELPER METHODS ────────────────────────────

    /// Gets VPC configuration from the Network resource if one exists in the stack.
    ///
    /// If a Network resource exists (ID: "default-network"), this method retrieves
    /// the VPC ID, subnet IDs, and security group ID from the Network controller
    /// to configure the Lambda worker to run inside the VPC.
    ///
    /// Returns `None` if no Network resource exists in the stack.
    fn get_vpc_config(&self, ctx: &ResourceControllerContext<'_>) -> Result<Option<VpcConfig>> {
        // Check if the stack has a Network resource
        let network_id = "default-network";
        if !ctx.desired_stack.resources.contains_key(network_id) {
            return Ok(None);
        }

        // Get the Network controller state via require_dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, network_id.to_string());
        let network_state =
            ctx.require_dependency::<crate::network::AwsNetworkController>(&network_ref)?;

        // Only configure VPC if we have subnet IDs and a security group
        // For Lambda, we use private subnets (no public IP assignment)
        if network_state.private_subnet_ids.is_empty() {
            return Ok(None);
        }

        let security_group_ids = match &network_state.security_group_id {
            Some(sg) => vec![sg.clone()],
            None => vec![],
        };

        if security_group_ids.is_empty() {
            return Ok(None);
        }

        Ok(Some(
            VpcConfig::builder()
                .set_subnet_ids(Some(network_state.private_subnet_ids.clone()))
                .set_security_group_ids(Some(security_group_ids))
                .build(),
        ))
    }

    async fn prepare_environment_variables(
        &self,
        initial_env: &HashMap<String, String>,
        links: &[ResourceRef],
        ctx: &ResourceControllerContext<'_>,
        worker_name_for_error_logging: &str,
    ) -> Result<HashMap<String, String>> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Get the worker's own binding params (may be None during initial creation)
        let self_binding_params = self.get_binding_params()?;

        let env_vars = EnvironmentVariableBuilder::try_new(initial_env)?
            .add_worker_runtime_env_vars(ctx, &worker_config.id)?
            .add_linked_resources(links, ctx, worker_name_for_error_logging)
            .await?
            .add_self_worker_binding(&worker_config.id, self_binding_params.as_ref())?
            .build();

        Ok(env_vars)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(worker_name: &str) -> Self {
        Self {
            state: AwsWorkerState::Ready,
            arn: Some(format!(
                "arn:aws:lambda:us-east-1:123456789012:function:{}",
                worker_name
            )),
            url: Some(format!("https://abcd1234.lambda-url.us-east-1.on.aws/")),
            worker_name: Some(worker_name.to_string()),
            event_source_mappings: Vec::new(),
            fqdn: None,
            certificate_id: None,
            certificate_arn: None,
            api_id: None,
            integration_id: None,
            route_id: None,
            stage_name: None,
            api_mapping_id: None,
            domain_name: None,
            load_balancer: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            s3_permission_statement_ids: Vec::new(),
            eventbridge_rule_names: Vec::new(),
            eventbridge_permission_statement_ids: Vec::new(),
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # AWS Worker Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::collections::HashMap;
    use std::sync::Arc;

    use alien_core::{
        CertificateStatus, DnsRecordStatus, DomainMetadata, Ingress, Platform, ResourceDomainInfo,
        ResourceStatus, Worker, WorkerCode, WorkerOutputs, WorkerTrigger,
    };
    use aws_sdk_acm::{
        operation::{
            delete_certificate::DeleteCertificateOutput,
            import_certificate::ImportCertificateOutput,
        },
        Client as AcmClient,
    };
    use aws_sdk_apigatewayv2::{
        operation::{
            create_api::CreateApiOutput, create_api_mapping::CreateApiMappingOutput,
            create_domain_name::CreateDomainNameOutput,
            create_integration::CreateIntegrationOutput, create_route::CreateRouteOutput,
            create_stage::CreateStageOutput, delete_api::DeleteApiOutput,
            delete_api_mapping::DeleteApiMappingOutput, delete_domain_name::DeleteDomainNameOutput,
        },
        types::{DomainNameConfiguration, EndpointType, SecurityPolicy},
        Client as ApiGatewayV2Client,
    };
    use aws_sdk_eventbridge::{
        operation::{put_rule::PutRuleOutput, put_targets::PutTargetsOutput},
        types::RuleState,
        Client as EventBridgeClient,
    };
    use aws_sdk_iam::{operation::put_role_policy::PutRolePolicyOutput, Client as IamClient};
    use aws_sdk_lambda::{
        error::ErrorMetadata as LambdaErrorMetadata,
        operation::{
            add_permission::AddPermissionOutput,
            create_event_source_mapping::CreateEventSourceMappingOutput,
            create_function::CreateFunctionOutput,
            delete_event_source_mapping::DeleteEventSourceMappingOutput,
            delete_function::{DeleteFunctionError, DeleteFunctionOutput},
            delete_function_concurrency::DeleteFunctionConcurrencyOutput,
            get_function_configuration::GetFunctionConfigurationOutput,
            list_event_source_mappings::ListEventSourceMappingsOutput,
            put_function_concurrency::PutFunctionConcurrencyOutput,
            update_function_code::UpdateFunctionCodeOutput,
            update_function_configuration::UpdateFunctionConfigurationOutput,
        },
        types::{
            error::ResourceNotFoundException, Architecture as LambdaArchitecture,
            LastUpdateStatus as LambdaLastUpdateStatus, PackageType, State as LambdaState,
        },
        Client as LambdaClient,
    };
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};
    use aws_smithy_mocks::{mock, mock_client, MockResponse, RuleMode};
    use httpmock::prelude::*;
    use rstest::rstest;

    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use crate::worker::{
        fixtures::*, readiness_probe::test_utils::create_readiness_probe_mock, AwsWorkerController,
    };

    fn create_successful_create_function_response(worker_name: &str) -> CreateFunctionOutput {
        CreateFunctionOutput::builder()
            .function_name(worker_name)
            .function_arn(format!(
                "arn:aws:lambda:us-east-1:123456789012:function:{}",
                worker_name
            ))
            .state(LambdaState::Active)
            .last_update_status(LambdaLastUpdateStatus::Successful)
            .build()
    }

    fn create_successful_get_function_response(
        worker_name: &str,
    ) -> GetFunctionConfigurationOutput {
        GetFunctionConfigurationOutput::builder()
            .function_name(worker_name)
            .function_arn(format!(
                "arn:aws:lambda:us-east-1:123456789012:function:{}",
                worker_name
            ))
            .state(LambdaState::Active)
            .last_update_status(LambdaLastUpdateStatus::Successful)
            .build()
    }

    fn create_successful_update_code_response(worker_name: &str) -> UpdateFunctionCodeOutput {
        UpdateFunctionCodeOutput::builder()
            .function_name(worker_name)
            .function_arn(format!(
                "arn:aws:lambda:us-east-1:123456789012:function:{}",
                worker_name
            ))
            .state(LambdaState::Active)
            .last_update_status(LambdaLastUpdateStatus::Successful)
            .build()
    }

    fn create_successful_update_config_response(
        worker_name: &str,
    ) -> UpdateFunctionConfigurationOutput {
        UpdateFunctionConfigurationOutput::builder()
            .function_name(worker_name)
            .function_arn(format!(
                "arn:aws:lambda:us-east-1:123456789012:function:{}",
                worker_name
            ))
            .state(LambdaState::Active)
            .last_update_status(LambdaLastUpdateStatus::Successful)
            .build()
    }

    fn test_api_gateway_api(api_endpoint: Option<&str>) -> CreateApiOutput {
        let mut builder = CreateApiOutput::builder().api_id("test-api-id");
        if let Some(api_endpoint) = api_endpoint {
            builder = builder.api_endpoint(api_endpoint);
        }
        builder.build()
    }

    fn test_api_gateway_integration() -> CreateIntegrationOutput {
        CreateIntegrationOutput::builder()
            .integration_id("test-integration-id")
            .build()
    }

    fn test_api_gateway_route() -> CreateRouteOutput {
        CreateRouteOutput::builder()
            .route_id("test-route-id")
            .build()
    }

    fn test_api_gateway_stage() -> CreateStageOutput {
        CreateStageOutput::builder().stage_name("$default").build()
    }

    fn test_api_gateway_domain(domain_name: &str) -> CreateDomainNameOutput {
        CreateDomainNameOutput::builder()
            .domain_name(domain_name)
            .domain_name_configurations(
                DomainNameConfiguration::builder()
                    .certificate_arn("arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id")
                    .endpoint_type(EndpointType::Regional)
                    .security_policy(SecurityPolicy::Tls12)
                    .api_gateway_domain_name("test.execute-api.us-east-1.amazonaws.com")
                    .hosted_zone_id("Z1D633PJN98FT9")
                    .build(),
            )
            .build()
    }

    fn test_api_gateway_mapping() -> CreateApiMappingOutput {
        CreateApiMappingOutput::builder()
            .api_mapping_id("test-mapping-id")
            .build()
    }

    fn create_test_domain_metadata(resource_id: &str) -> DomainMetadata {
        let mut resources = HashMap::new();
        resources.insert(
            resource_id.to_string(),
            ResourceDomainInfo {
                fqdn: format!("{}.test.example.com", resource_id),
                certificate_id: "test-cert-id".to_string(),
                certificate_status: CertificateStatus::Issued,
                dns_status: DnsRecordStatus::Active,
                dns_error: None,
                certificate_chain: Some(
                    "-----BEGIN CERTIFICATE-----\nMIIBtest\n-----END CERTIFICATE-----\n"
                        .to_string(),
                ),
                private_key: Some(
                    "-----BEGIN RSA PRIVATE KEY-----\nMIIBtest\n-----END RSA PRIVATE KEY-----\n"
                        .to_string(),
                ),
                aliases: Vec::new(),
                issued_at: Some("2024-01-01T00:00:00Z".to_string()),
            },
        );
        DomainMetadata {
            base_domain: "test.example.com".to_string(),
            public_subdomain: "test".to_string(),
            hosted_zone_id: "Z1234567890ABC".to_string(),
            resources,
        }
    }

    fn create_acm_mock_for_creation() -> AcmClient {
        let import_rule = mock!(AcmClient::import_certificate)
            .match_requests(|request| {
                request.certificate().is_some() && request.private_key().is_some()
            })
            .then_output(test_import_certificate_response);

        mock_client!(
            aws_sdk_acm,
            RuleMode::Sequential,
            [&import_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn create_acm_mock_for_creation_and_deletion() -> AcmClient {
        let import_rule = mock!(AcmClient::import_certificate)
            .match_requests(|request| {
                request.certificate().is_some() && request.private_key().is_some()
            })
            .then_output(test_import_certificate_response);
        let delete_rule = mock!(AcmClient::delete_certificate)
            .match_requests(|request| request.certificate_arn().is_some())
            .then_output(|| DeleteCertificateOutput::builder().build());

        mock_client!(
            aws_sdk_acm,
            RuleMode::Sequential,
            [&import_rule, &delete_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn test_import_certificate_response() -> ImportCertificateOutput {
        ImportCertificateOutput::builder()
            .certificate_arn("arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id")
            .build()
    }

    fn create_apigatewayv2_mock_for_creation() -> ApiGatewayV2Client {
        let create_api_rule = mock!(ApiGatewayV2Client::create_api)
            .match_requests(|request| request.name().is_some())
            .then_output(|| {
                test_api_gateway_api(Some(
                    "https://test-api-id.execute-api.us-east-1.amazonaws.com",
                ))
            });
        let create_integration_rule = mock!(ApiGatewayV2Client::create_integration)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_integration);
        let create_route_rule = mock!(ApiGatewayV2Client::create_route)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_route);
        let create_stage_rule = mock!(ApiGatewayV2Client::create_stage)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_stage);
        let create_domain_rule = mock!(ApiGatewayV2Client::create_domain_name)
            .match_requests(|request| request.domain_name().is_some())
            .then_output(|| test_api_gateway_domain("test.example.com"));
        let create_mapping_rule = mock!(ApiGatewayV2Client::create_api_mapping)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_mapping);

        mock_client!(
            aws_sdk_apigatewayv2,
            RuleMode::Sequential,
            [
                &create_api_rule,
                &create_integration_rule,
                &create_route_rule,
                &create_stage_rule,
                &create_domain_rule,
                &create_mapping_rule
            ],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn create_apigatewayv2_mock_for_creation_and_deletion() -> ApiGatewayV2Client {
        let create_api_rule = mock!(ApiGatewayV2Client::create_api)
            .match_requests(|request| request.name().is_some())
            .then_output(|| {
                test_api_gateway_api(Some(
                    "https://test-api-id.execute-api.us-east-1.amazonaws.com",
                ))
            });
        let create_integration_rule = mock!(ApiGatewayV2Client::create_integration)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_integration);
        let create_route_rule = mock!(ApiGatewayV2Client::create_route)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_route);
        let create_stage_rule = mock!(ApiGatewayV2Client::create_stage)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_stage);
        let create_domain_rule = mock!(ApiGatewayV2Client::create_domain_name)
            .match_requests(|request| request.domain_name().is_some())
            .then_output(|| test_api_gateway_domain("test.example.com"));
        let create_mapping_rule = mock!(ApiGatewayV2Client::create_api_mapping)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_mapping);
        let delete_mapping_rule = mock!(ApiGatewayV2Client::delete_api_mapping)
            .match_requests(|request| request.api_mapping_id() == Some("test-mapping-id"))
            .then_output(|| DeleteApiMappingOutput::builder().build());
        let delete_domain_rule = mock!(ApiGatewayV2Client::delete_domain_name)
            .match_requests(|request| request.domain_name().is_some())
            .then_output(|| DeleteDomainNameOutput::builder().build());
        let delete_api_rule = mock!(ApiGatewayV2Client::delete_api)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(|| DeleteApiOutput::builder().build());

        mock_client!(
            aws_sdk_apigatewayv2,
            RuleMode::Sequential,
            [
                &create_api_rule,
                &create_integration_rule,
                &create_route_rule,
                &create_stage_rule,
                &create_domain_rule,
                &create_mapping_rule,
                &delete_mapping_rule,
                &delete_domain_rule,
                &delete_api_rule
            ],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn lambda_resource_not_found_error(resource_name: &str) -> ResourceNotFoundException {
        ResourceNotFoundException::builder()
            .message(format!("Lambda function '{resource_name}' was not found"))
            .meta(
                LambdaErrorMetadata::builder()
                    .code("ResourceNotFoundException")
                    .message(format!("Lambda function '{resource_name}' was not found"))
                    .build(),
            )
            .build()
    }

    fn create_lambda_mock_for_creation_and_update(
        worker_name: &str,
        allow_add_permission: bool,
    ) -> LambdaClient {
        let worker_name = worker_name.to_string();

        let create_worker_name = worker_name.clone();
        let create_rule = mock!(LambdaClient::create_function)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || create_successful_create_function_response(&create_worker_name));

        let get_worker_name = worker_name.clone();
        let get_rule = mock!(LambdaClient::get_function_configuration)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || create_successful_get_function_response(&get_worker_name));

        let update_code_worker_name = worker_name.clone();
        let update_code_rule = mock!(LambdaClient::update_function_code)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || create_successful_update_code_response(&update_code_worker_name));

        let update_config_worker_name = worker_name.clone();
        let update_config_rule = mock!(LambdaClient::update_function_configuration)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || {
                create_successful_update_config_response(&update_config_worker_name)
            });

        let put_concurrency_rule = mock!(LambdaClient::put_function_concurrency)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| PutFunctionConcurrencyOutput::builder().build());
        let delete_concurrency_rule = mock!(LambdaClient::delete_function_concurrency)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| DeleteFunctionConcurrencyOutput::builder().build());
        let list_mappings_rule = mock!(LambdaClient::list_event_source_mappings)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| ListEventSourceMappingsOutput::builder().build());
        let create_mapping_rule = mock!(LambdaClient::create_event_source_mapping)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| {
                CreateEventSourceMappingOutput::builder()
                    .uuid("test-event-source-mapping")
                    .build()
            });
        let delete_mapping_rule = mock!(LambdaClient::delete_event_source_mapping)
            .match_requests(|request| request.uuid().is_some())
            .then_output(|| DeleteEventSourceMappingOutput::builder().build());
        let delete_function_rule = mock!(LambdaClient::delete_function)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| DeleteFunctionOutput::builder().build());

        let add_permission_rule = mock!(LambdaClient::add_permission)
            .match_requests(move |request| {
                allow_add_permission && request.function_name().is_some()
            })
            .then_output(|| AddPermissionOutput::builder().build());

        mock_client!(
            aws_sdk_lambda,
            RuleMode::MatchAny,
            [
                &create_rule,
                &get_rule,
                &add_permission_rule,
                &update_code_rule,
                &update_config_rule,
                &put_concurrency_rule,
                &delete_concurrency_rule,
                &list_mappings_rule,
                &create_mapping_rule,
                &delete_mapping_rule,
                &delete_function_rule
            ],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn setup_mock_client_for_creation_and_update(worker_name: &str, has_url: bool) -> LambdaClient {
        create_lambda_mock_for_creation_and_update(worker_name, has_url)
    }

    fn setup_mock_client_for_creation_and_deletion(
        worker_name: &str,
        has_url: bool,
    ) -> LambdaClient {
        let worker_name = worker_name.to_string();
        let function_arn = format!(
            "arn:aws:lambda:us-east-1:123456789012:function:{}",
            worker_name
        );

        let create_worker_name = worker_name.clone();
        let create_rule = mock!(LambdaClient::create_function)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || create_successful_create_function_response(&create_worker_name));

        let active_worker_name = worker_name.clone();
        let get_active_worker_name = active_worker_name.clone();
        let get_active_rule = mock!(LambdaClient::get_function_configuration)
            .match_requests(move |request| request.function_name() == Some(worker_name.as_str()))
            .then_output(move || create_successful_get_function_response(&get_active_worker_name));

        let missing_arn = function_arn.clone();
        let get_missing_rule = mock!(LambdaClient::get_function_configuration)
            .match_requests(move |request| request.function_name() == Some(missing_arn.as_str()))
            .then_error(move || {
                aws_sdk_lambda::operation::get_function_configuration::GetFunctionConfigurationError::ResourceNotFoundException(
                    lambda_resource_not_found_error(&function_arn),
                )
            });

        let add_permission_rule = mock!(LambdaClient::add_permission)
            .match_requests(move |request| has_url && request.function_name().is_some())
            .then_output(|| AddPermissionOutput::builder().build());

        let update_config_worker_name = active_worker_name.clone();
        let update_config_rule = mock!(LambdaClient::update_function_configuration)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || {
                create_successful_update_config_response(&update_config_worker_name)
            });
        let put_concurrency_rule = mock!(LambdaClient::put_function_concurrency)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| PutFunctionConcurrencyOutput::builder().build());
        let delete_concurrency_rule = mock!(LambdaClient::delete_function_concurrency)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| DeleteFunctionConcurrencyOutput::builder().build());
        let delete_function_rule = mock!(LambdaClient::delete_function)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| DeleteFunctionOutput::builder().build());
        let list_mappings_rule = mock!(LambdaClient::list_event_source_mappings)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| ListEventSourceMappingsOutput::builder().build());
        let create_mapping_rule = mock!(LambdaClient::create_event_source_mapping)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| {
                CreateEventSourceMappingOutput::builder()
                    .uuid("test-event-source-mapping")
                    .build()
            });
        let delete_mapping_rule = mock!(LambdaClient::delete_event_source_mapping)
            .match_requests(|request| request.uuid().is_some())
            .then_output(|| DeleteEventSourceMappingOutput::builder().build());

        mock_client!(
            aws_sdk_lambda,
            RuleMode::MatchAny,
            [
                &create_rule,
                &get_active_rule,
                &get_missing_rule,
                &add_permission_rule,
                &update_config_rule,
                &put_concurrency_rule,
                &delete_concurrency_rule,
                &delete_function_rule,
                &list_mappings_rule,
                &create_mapping_rule,
                &delete_mapping_rule
            ],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn setup_mock_client_for_best_effort_deletion(
        worker_name: &str,
        function_missing: bool,
    ) -> LambdaClient {
        let worker_name = worker_name.to_string();
        let function_arn = format!(
            "arn:aws:lambda:us-east-1:123456789012:function:{}",
            worker_name
        );
        let delete_error_name = worker_name.clone();
        let delete_rule = mock!(LambdaClient::delete_function)
            .match_requests(|request| request.function_name().is_some())
            .then_compute_response(move |_| {
                if function_missing {
                    MockResponse::Error(DeleteFunctionError::ResourceNotFoundException(
                        lambda_resource_not_found_error(&delete_error_name),
                    ))
                } else {
                    MockResponse::Output(DeleteFunctionOutput::builder().build())
                }
            });

        let missing_arn = function_arn.clone();
        let get_missing_rule = mock!(LambdaClient::get_function_configuration)
            .match_requests(move |request| request.function_name() == Some(missing_arn.as_str()))
            .then_error(move || {
                aws_sdk_lambda::operation::get_function_configuration::GetFunctionConfigurationError::ResourceNotFoundException(
                    lambda_resource_not_found_error(&function_arn),
                )
            });

        mock_client!(
            aws_sdk_lambda,
            RuleMode::MatchAny,
            [&delete_rule, &get_missing_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn create_aws_iam_mock_for_resource_permissions() -> IamClient {
        let put_role_policy_rule = mock!(IamClient::put_role_policy)
            .match_requests(|request| {
                request.role_name().is_some()
                    && request.policy_name().is_some()
                    && request.policy_document().is_some()
            })
            .then_output(|| PutRolePolicyOutput::builder().build());

        mock_client!(
            aws_sdk_iam,
            RuleMode::MatchAny,
            [&put_role_policy_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn setup_mock_service_provider(
        mock_lambda: LambdaClient,
        mock_acm: Option<AcmClient>,
        mock_apigw: Option<ApiGatewayV2Client>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_aws_lambda_client()
            .returning(move |_| Ok(mock_lambda.clone()));

        // Mock IAM client for resource-scoped permissions (ApplyingResourcePermissions state)
        let mock_iam = create_aws_iam_mock_for_resource_permissions();
        mock_provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        if let Some(acm) = mock_acm {
            mock_provider
                .expect_get_aws_acm_client()
                .returning(move |_| Ok(acm.clone()));
        }

        if let Some(apigw) = mock_apigw {
            mock_provider
                .expect_get_aws_apigatewayv2_client()
                .returning(move |_| Ok(apigw.clone()));
        }

        Arc::new(mock_provider)
    }

    /// Sets up all mocks for a worker test, including Lambda, ACM, and API Gateway.
    ///
    /// Returns `(mock_provider, optional_mock_server, optional_domain_metadata, optional_public_urls)`.
    /// For public workers, `domain_metadata` and `public_urls` must be set on the executor builder.
    /// When a readiness probe is configured, `public_urls` overrides the FQDN URL so the probe
    /// hits the local mock HTTP server instead.
    fn setup_mocks_for_function(
        worker: &Worker,
        worker_name: &str,
        for_deletion: bool,
    ) -> (
        Arc<MockPlatformServiceProvider>,
        Option<MockServer>,
        Option<DomainMetadata>,
        Option<HashMap<String, String>>,
    ) {
        let has_url = worker.ingress == Ingress::Public;
        let needs_readiness_probe = has_url && worker.readiness_probe.is_some();

        // Set up mock server for readiness probe if needed
        let mock_server = if needs_readiness_probe {
            Some(create_readiness_probe_mock(worker))
        } else {
            None
        };

        // Set up Lambda client mock (same for both flows; URL config calls are removed)
        let lambda_mock = if for_deletion {
            setup_mock_client_for_creation_and_deletion(worker_name, has_url)
        } else {
            setup_mock_client_for_creation_and_update(worker_name, has_url)
        };

        // Set up ACM and API Gateway mocks for public workers
        let (acm_mock, apigw_mock, domain_metadata, public_urls) = if has_url {
            let dm = create_test_domain_metadata(&worker.id);
            let acm = if for_deletion {
                create_acm_mock_for_creation_and_deletion()
            } else {
                create_acm_mock_for_creation()
            };
            let apigw = if for_deletion {
                create_apigatewayv2_mock_for_creation_and_deletion()
            } else {
                create_apigatewayv2_mock_for_creation()
            };
            // For readiness probe tests, override the FQDN URL with the mock server URL
            let pub_urls = mock_server.as_ref().map(|server| {
                let mut map = HashMap::new();
                map.insert(worker.id.clone(), server.base_url());
                map
            });
            (Some(acm), Some(apigw), Some(dm), pub_urls)
        } else {
            (None, None, None, None)
        };

        let mock_provider = setup_mock_service_provider(lambda_mock, acm_mock, apigw_mock);

        (mock_provider, mock_server, domain_metadata, public_urls)
    }

    // ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

    #[rstest]
    #[case::basic(basic_function(), false)]
    #[case::env_vars(function_with_env_vars(), false)]
    #[case::storage_link(function_with_storage_link(), false)]
    #[case::env_and_storage(function_with_env_and_storage(), false)]
    #[case::multiple_storages(function_with_multiple_storages(), false)]
    #[case::public_ingress(function_public_ingress(), true)]
    #[case::private_ingress(function_private_ingress(), false)]
    #[case::concurrency(function_with_concurrency(), false)]
    #[case::custom_config(function_custom_config(), false)]
    #[case::readiness_probe(function_with_readiness_probe(), true)]
    #[case::complete_test(function_complete_test(), true)]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(#[case] worker: Worker, #[case] _has_url: bool) {
        let worker_name = format!("test-{}", worker.id);
        let (mock_provider, _mock_server, domain_metadata, public_urls) =
            setup_mocks_for_function(&worker, &worker_name, true);

        let mut builder = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies();

        if let Some(dm) = domain_metadata {
            builder = builder.domain_metadata(dm);
        }
        if let Some(urls) = public_urls {
            builder = builder.public_urls(urls);
        }

        let mut executor = builder.build().await.unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs are available
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        assert!(function_outputs.identifier.is_some());
        assert!(function_outputs.worker_name.starts_with("test-"));

        // Delete the worker
        executor.delete().unwrap();

        // Run delete flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    #[tokio::test]
    async fn create_schedule_trigger_uses_sdk_native_eventbridge_mock() {
        let worker = Worker::new("schedule-func".to_string())
            .code(WorkerCode::Image {
                image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/schedule:latest".to_string(),
            })
            .permissions("default-profile".to_string())
            .trigger(WorkerTrigger::Schedule {
                cron: "*/5 * * * *".to_string(),
            })
            .build();
        let worker_name = format!("test-{}", worker.id);
        let function_arn = format!(
            "arn:aws:lambda:us-east-1:123456789012:function:{}",
            worker_name
        );
        let rule_name = format!("{worker_name}-cron-0");
        let rule_arn = format!("arn:aws:events:us-east-1:123456789012:rule/{rule_name}");

        let expected_rule_name = rule_name.clone();
        let put_rule_output_arn = rule_arn.clone();
        let put_rule_rule = mock!(EventBridgeClient::put_rule)
            .match_requests(move |request| {
                request.name() == Some(expected_rule_name.as_str())
                    && request.schedule_expression() == Some("rate(5 minutes)")
                    && request.state() == Some(&RuleState::Enabled)
                    && request
                        .description()
                        .is_some_and(|description| description.contains("schedule-func"))
                    && request
                        .tags()
                        .iter()
                        .any(|tag| tag.key() == "resource" && tag.value() == "schedule-func")
            })
            .then_output(move || {
                PutRuleOutput::builder()
                    .rule_arn(put_rule_output_arn.clone())
                    .build()
            });

        let expected_target_rule_name = rule_name.clone();
        let expected_target_arn = function_arn.clone();
        let put_targets_rule = mock!(EventBridgeClient::put_targets)
            .match_requests(move |request| {
                request.rule() == Some(expected_target_rule_name.as_str())
                    && request.targets().iter().any(|target| {
                        target.id() == "1" && target.arn() == expected_target_arn.as_str()
                    })
            })
            .then_output(|| PutTargetsOutput::builder().failed_entry_count(0).build());

        let eventbridge_client = mock_client!(
            aws_sdk_eventbridge,
            RuleMode::Sequential,
            [&put_rule_rule, &put_targets_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );

        let lambda_client = create_lambda_mock_for_creation_and_update(&worker_name, true);

        let mut mock_provider = MockPlatformServiceProvider::new();
        mock_provider
            .expect_get_aws_lambda_client()
            .returning(move |_| Ok(lambda_client.clone()));
        let mock_iam = create_aws_iam_mock_for_resource_permissions();
        mock_provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));
        mock_provider
            .expect_get_aws_eventbridge_client()
            .returning(move |_| Ok(eventbridge_client.clone()));

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(Arc::new(mock_provider))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        assert_eq!(
            function_outputs.identifier.as_deref(),
            Some(function_arn.as_str())
        );
        assert_eq!(put_rule_rule.num_calls(), 1);
        assert_eq!(put_targets_rule.num_calls(), 1);
    }

    // ─────────────── UPDATE FLOW TESTS ────────────────────────────────

    #[rstest]
    #[case::basic_to_env(basic_function(), function_with_env_vars())]
    #[case::env_to_storage(function_with_env_vars(), function_with_storage_link())]
    #[case::storage_to_custom(function_with_storage_link(), function_custom_config())]
    #[case::custom_to_public(function_custom_config(), function_public_ingress())]
    #[case::public_to_complete(function_public_ingress(), function_complete_test())]
    #[case::complete_to_basic(function_complete_test(), basic_function())]
    #[tokio::test]
    async fn test_update_flow_succeeds(#[case] from_function: Worker, #[case] to_function: Worker) {
        // Ensure both workers have the same ID for valid updates
        let worker_id = "test-update-worker".to_string();
        let mut from_function = from_function;
        from_function.id = worker_id.clone();

        let mut to_function = to_function;
        to_function.id = worker_id.clone();

        let worker_name = format!("test-{}", worker_id);
        let (mock_provider, mock_server, domain_metadata, public_urls) =
            setup_mocks_for_function(&to_function, &worker_name, false);

        // Start with the "from" worker in Ready state
        let mut ready_controller = AwsWorkerController::mock_ready(&worker_name);

        // If the target worker has a readiness probe, update the controller URL to point to mock server
        if to_function.readiness_probe.is_some() && to_function.ingress == Ingress::Public {
            if let Some(ref server) = mock_server {
                ready_controller.url = Some(server.base_url());
            }
        }

        let mut builder = SingleControllerExecutor::builder()
            .resource(from_function)
            .controller(ready_controller)
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies();

        if let Some(dm) = domain_metadata {
            builder = builder.domain_metadata(dm);
        }
        if let Some(urls) = public_urls {
            builder = builder.public_urls(urls);
        }

        let mut executor = builder.build().await.unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to the new worker
        let target_is_public = to_function.ingress == Ingress::Public;
        executor.update(to_function).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        if target_is_public {
            let url = function_outputs.url.as_deref().unwrap();
            assert!(url.starts_with("http://") || url.starts_with("https://"));
            assert!(!url.contains("lambda-url"));
        }
    }

    // ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

    #[rstest]
    #[case::basic(basic_function(), false)]
    #[case::public_with_missing_function(function_public_ingress(), true)]
    #[case::public(function_public_ingress(), false)]
    #[case::private_with_missing_function(function_private_ingress(), true)]
    #[tokio::test]
    async fn test_best_effort_deletion_when_resources_missing(
        #[case] worker: Worker,
        #[case] function_missing: bool,
    ) {
        let worker_name = format!("test-{}", worker.id);
        let has_url = worker.ingress == Ingress::Public;
        let mock_lambda =
            setup_mock_client_for_best_effort_deletion(&worker_name, function_missing);
        let mock_provider = setup_mock_service_provider(mock_lambda, None, None);

        // Start with a ready controller
        let mut ready_controller = AwsWorkerController::mock_ready(&worker_name);
        if has_url {
            ready_controller.url =
                Some("https://example.execute-api.us-east-1.amazonaws.com".to_string());
        }

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(ready_controller)
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Delete the worker
        executor.delete().unwrap();

        // Run the delete flow - it should succeed even when resources are missing
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

    /// Test that verifies public workers go through ACM certificate import and API Gateway setup.
    #[tokio::test]
    async fn test_public_function_creates_api_gateway_and_certificate() {
        let worker = function_public_ingress();
        let worker_name = format!("test-{}", worker.id);
        let domain_metadata = create_test_domain_metadata(&worker.id);

        let worker_name_for_create = worker_name.clone();
        let create_lambda_rule = mock!(LambdaClient::create_function)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || {
                create_successful_create_function_response(&worker_name_for_create)
            });

        let worker_name_for_get = worker_name.clone();
        let get_lambda_rule = mock!(LambdaClient::get_function_configuration)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || create_successful_get_function_response(&worker_name_for_get));

        // Validate API Gateway permission is added with the correct apigateway principal
        let add_permission_rule = mock!(LambdaClient::add_permission)
            .match_requests(|request| {
                request.statement_id() == Some("ApiGatewayInvoke")
                    && request.action() == Some("lambda:InvokeFunction")
                    && request.principal() == Some("apigateway.amazonaws.com")
            })
            .then_output(|| AddPermissionOutput::builder().build());

        let worker_name_for_config_update = worker_name.clone();
        let update_config_rule = mock!(LambdaClient::update_function_configuration)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || {
                create_successful_update_config_response(&worker_name_for_config_update)
            });
        let put_concurrency_rule = mock!(LambdaClient::put_function_concurrency)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| PutFunctionConcurrencyOutput::builder().build());
        let lambda_client = mock_client!(
            aws_sdk_lambda,
            RuleMode::MatchAny,
            [
                &create_lambda_rule,
                &get_lambda_rule,
                &add_permission_rule,
                &update_config_rule,
                &put_concurrency_rule
            ],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );

        // Validate ACM certificate import through the generated ACM client.
        let import_certificate_rule = mock!(AcmClient::import_certificate)
            .match_requests(|request| {
                request.certificate().is_some()
                    && request.private_key().is_some()
                    && request
                        .tags()
                        .iter()
                        .any(|tag| tag.key() == "resource" && tag.value() == Some("public-func"))
            })
            .then_output(test_import_certificate_response);
        let acm_client = mock_client!(
            aws_sdk_acm,
            RuleMode::Sequential,
            [&import_certificate_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );

        // Validate API Gateway creation through the generated API Gateway V2 client.
        let create_api_rule = mock!(ApiGatewayV2Client::create_api)
            .match_requests(|request| {
                request
                    .name()
                    .is_some_and(|name| name.contains("public-func"))
            })
            .then_output(|| test_api_gateway_api(None));
        let create_integration_rule = mock!(ApiGatewayV2Client::create_integration)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_integration);
        let create_route_rule = mock!(ApiGatewayV2Client::create_route)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_route);
        let create_stage_rule = mock!(ApiGatewayV2Client::create_stage)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_stage);
        let create_domain_rule = mock!(ApiGatewayV2Client::create_domain_name)
            .match_requests(|request| request.domain_name() == Some("public-func.test.example.com"))
            .then_output(|| test_api_gateway_domain("public-func.test.example.com"));
        let create_mapping_rule = mock!(ApiGatewayV2Client::create_api_mapping)
            .match_requests(|request| request.api_id() == Some("test-api-id"))
            .then_output(test_api_gateway_mapping);
        let apigw_client = mock_client!(
            aws_sdk_apigatewayv2,
            RuleMode::Sequential,
            [
                &create_api_rule,
                &create_integration_rule,
                &create_route_rule,
                &create_stage_rule,
                &create_domain_rule,
                &create_mapping_rule
            ],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );

        let mock_provider =
            setup_mock_service_provider(lambda_client, Some(acm_client), Some(apigw_client));

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .domain_metadata(domain_metadata)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify URL is in outputs (derived from domain_metadata FQDN)
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        assert!(function_outputs.url.is_some());
        assert_eq!(import_certificate_rule.num_calls(), 1);
        assert_eq!(create_api_rule.num_calls(), 1);
    }

    /// Test that verifies private workers don't get URL creation
    #[tokio::test]
    async fn test_private_function_skips_url_creation() {
        let worker = function_private_ingress();
        let worker_name = format!("test-{}", worker.id);

        let lambda_client = create_lambda_mock_for_creation_and_update(&worker_name, false);
        let mock_provider = setup_mock_service_provider(lambda_client, None, None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify no URL in outputs
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        assert!(function_outputs.url.is_none());
    }

    /// Test that verifies correct worker configuration parameters
    #[tokio::test]
    async fn test_worker_configuration_validation() {
        let worker = function_custom_config();
        let worker_name = format!("test-{}", worker.id);

        // Validate worker creation request has correct parameters
        let worker_name_for_create = worker_name.clone();
        let create_rule = mock!(LambdaClient::create_function)
            .match_requests(|request| {
                request.memory_size() == Some(512)
                    && request.timeout() == Some(120)
                    && request.package_type() == Some(&PackageType::Image)
                    && request.architectures().contains(&LambdaArchitecture::Arm64)
            })
            .then_output(move || {
                create_successful_create_function_response(&worker_name_for_create)
            });

        let worker_name_for_get = worker_name.clone();
        let get_rule = mock!(LambdaClient::get_function_configuration)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || create_successful_get_function_response(&worker_name_for_get));
        let put_concurrency_rule = mock!(LambdaClient::put_function_concurrency)
            .match_requests(|request| request.function_name().is_some())
            .then_output(|| PutFunctionConcurrencyOutput::builder().build());
        let lambda_client = mock_client!(
            aws_sdk_lambda,
            RuleMode::MatchAny,
            [&create_rule, &get_rule, &put_concurrency_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );
        let mock_provider = setup_mock_service_provider(lambda_client, None, None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies environment variables are correctly passed
    #[tokio::test]
    async fn test_environment_variable_handling() {
        let worker = function_with_env_vars();
        let worker_name = format!("test-{}", worker.id);

        // Validate worker creation request has environment variables
        let worker_name_for_create = worker_name.clone();
        let create_rule = mock!(LambdaClient::create_function)
            .match_requests(|request| {
                let Some(env) = request.environment() else {
                    return false;
                };
                let Some(vars) = env.variables() else {
                    return false;
                };
                vars.get("APP_ENV") == Some(&"production".to_string())
                    && vars.get("LOG_LEVEL") == Some(&"debug".to_string())
                    && vars.get("DB_NAME") == Some(&"myapp".to_string())
            })
            .then_output(move || {
                create_successful_create_function_response(&worker_name_for_create)
            });

        let worker_name_for_get = worker_name.clone();
        let get_rule = mock!(LambdaClient::get_function_configuration)
            .match_requests(|request| request.function_name().is_some())
            .then_output(move || create_successful_get_function_response(&worker_name_for_get));
        let lambda_client = mock_client!(
            aws_sdk_lambda,
            RuleMode::MatchAny,
            [&create_rule, &get_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );
        let mock_provider = setup_mock_service_provider(lambda_client, None, None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}
