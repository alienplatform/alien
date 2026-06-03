use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    CertificateStatus, Ingress, KubernetesCertificateMode, KubernetesExposureSettings,
    KubernetesGatewayRouteProfile, KubernetesIngressRouteProfile, KubernetesRouteProfile,
    KubernetesRouteProviderOptions, KubernetesTlsSecretRef, LoadBalancerEndpoint,
};
use alien_error::{AlienError, Context, ContextError};
use k8s_openapi::api::core::v1::{Secret, Service, ServicePort, ServiceSpec};
use k8s_openapi::api::networking::v1::{
    HTTPIngressPath, HTTPIngressRuleValue, Ingress as K8sIngress, IngressBackend, IngressRule,
    IngressServiceBackend, IngressSpec, IngressTLS, ServiceBackendPort,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use k8s_openapi::ByteString;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;

#[cfg(feature = "aws")]
use crate::core::split_certificate_chain;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

const ENDPOINT_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KubernetesPublicEndpointState {
    pub(crate) service_name: Option<String>,
    pub(crate) ingress_name: Option<String>,
    pub(crate) gateway_name: Option<String>,
    pub(crate) http_route_name: Option<String>,
    pub(crate) gke_health_check_policy_name: Option<String>,
    pub(crate) azure_health_check_policy_name: Option<String>,
    pub(crate) managed_tls_secret_name: Option<String>,
    pub(crate) managed_acm_certificate_arn: Option<String>,
    pub(crate) public_url: Option<String>,
    pub(crate) load_balancer_endpoint: Option<LoadBalancerEndpoint>,
    pub(crate) published_certificate_id: Option<String>,
    pub(crate) published_certificate_issued_at: Option<String>,
}

impl KubernetesPublicEndpointState {
    pub(crate) fn effective_public_url(&self) -> Option<String> {
        self.public_url.clone().or_else(|| {
            self.load_balancer_endpoint
                .as_ref()
                .map(load_balancer_endpoint_url)
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct KubernetesPublicEndpointTarget<'a> {
    pub(crate) resource_id: &'a str,
    pub(crate) workload_name: &'a str,
    pub(crate) namespace: &'a str,
    pub(crate) component: &'a str,
    pub(crate) selector: BTreeMap<String, String>,
    pub(crate) service_port: u16,
    pub(crate) target_port: u16,
    pub(crate) health_check_path: Option<String>,
    pub(crate) public: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KubernetesEndpointAction {
    Ready,
    Waiting { suggested_delay: Duration },
}

#[derive(Debug, Clone)]
struct EndpointPlan {
    hostname: Option<String>,
    public_url: Option<String>,
    route: KubernetesRouteProfile,
    certificate: EndpointCertificate,
}

#[derive(Debug, Clone)]
enum EndpointPlanResolution {
    Disabled,
    Waiting,
    Ready(EndpointPlan),
}

#[derive(Debug, Clone)]
enum EndpointCertificate {
    ManagedTlsSecret {
        name: String,
        certificate_id: String,
        issued_at: Option<String>,
        certificate_chain: String,
        private_key: String,
    },
    TlsSecretRef(KubernetesTlsSecretRef),
    AwsAcmArn(String),
    ManagedAcmImport {
        region: Option<String>,
        tags: HashMap<String, String>,
        certificate_id: String,
        issued_at: Option<String>,
        certificate_chain: String,
        private_key: String,
    },
    None,
}

struct ManagedAcmCertificateInput {
    region: Option<String>,
    tags: HashMap<String, String>,
    certificate_id: String,
    issued_at: Option<String>,
    certificate_chain: String,
    private_key: String,
}

pub(crate) async fn reconcile_kubernetes_public_endpoint(
    ctx: &ResourceControllerContext<'_>,
    target: KubernetesPublicEndpointTarget<'_>,
    state: &mut KubernetesPublicEndpointState,
) -> Result<KubernetesEndpointAction> {
    let mut plan = match resolve_endpoint_plan(ctx, &target)? {
        EndpointPlanResolution::Disabled => {
            delete_kubernetes_public_endpoint(ctx, target.resource_id, target.namespace, state)
                .await?;
            state.public_url = None;
            state.load_balancer_endpoint = None;
            return Ok(KubernetesEndpointAction::Ready);
        }
        EndpointPlanResolution::Waiting => {
            return Ok(KubernetesEndpointAction::Waiting {
                suggested_delay: ENDPOINT_WAIT,
            });
        }
        EndpointPlanResolution::Ready(plan) => plan,
    };
    let previous_ingress_name = state.ingress_name.clone();
    let previous_gateway_name = state.gateway_name.clone();
    let previous_http_route_name = state.http_route_name.clone();
    let previous_gke_health_check_policy_name = state.gke_health_check_policy_name.clone();
    let previous_azure_health_check_policy_name = state.azure_health_check_policy_name.clone();
    let previous_managed_tls_secret_name = state.managed_tls_secret_name.clone();

    let kubernetes_config = ctx.get_kubernetes_config()?;
    let service_client = ctx
        .service_provider
        .get_kubernetes_service_client(kubernetes_config)
        .await?;
    let route_client = ctx
        .service_provider
        .get_kubernetes_route_client(kubernetes_config)
        .await?;

    let service_name = format!("{}-public", target.workload_name);
    let service = build_service(&target, &service_name);
    upsert_service(
        &service_client,
        target.namespace,
        &service_name,
        service,
        target.resource_id,
    )
    .await?;
    state.service_name = Some(service_name.clone());

    let mut active_managed_tls_secret_name = None;
    let mut active_managed_acm_certificate = false;
    let tls_ref = match &plan.certificate {
        EndpointCertificate::ManagedTlsSecret {
            name,
            certificate_id,
            issued_at,
            certificate_chain,
            private_key,
        } => {
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            upsert_tls_secret(
                &secrets_client,
                target.namespace,
                name,
                certificate_chain,
                private_key,
                certificate_id,
                issued_at.as_deref(),
                target.resource_id,
            )
            .await?;
            state.managed_tls_secret_name = Some(name.clone());
            state.published_certificate_id = Some(certificate_id.clone());
            state.published_certificate_issued_at = issued_at.clone();
            active_managed_tls_secret_name = Some(name.clone());
            Some(KubernetesTlsSecretRef {
                secret_name: name.clone(),
                namespace: Some(target.namespace.to_string()),
            })
        }
        EndpointCertificate::TlsSecretRef(secret_ref) => {
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            let secret_namespace = resolve_tls_secret_namespace(
                secret_ref.namespace.as_deref(),
                target.namespace,
                target.resource_id,
            )?;
            secrets_client
                .get_secret(secret_namespace, &secret_ref.secret_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Kubernetes TLS Secret '{}' was not found",
                        secret_ref.secret_name
                    ),
                    resource_id: Some(target.resource_id.to_string()),
                })?;
            Some(KubernetesTlsSecretRef {
                secret_name: secret_ref.secret_name.clone(),
                namespace: Some(secret_namespace.to_string()),
            })
        }
        EndpointCertificate::AwsAcmArn(_) | EndpointCertificate::None => None,
        EndpointCertificate::ManagedAcmImport {
            region,
            tags,
            certificate_id,
            issued_at,
            certificate_chain,
            private_key,
        } => {
            if !is_aws_alb_ingress(&plan.route) {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: target.resource_id.to_string(),
                    message:
                        "managedAcmImport certificate mode requires an AWS ALB Ingress route profile"
                            .to_string(),
                }));
            }
            let certificate_arn = publish_managed_acm_certificate(
                ctx,
                &target,
                state,
                ManagedAcmCertificateInput {
                    region: region.clone(),
                    tags: tags.clone(),
                    certificate_id: certificate_id.clone(),
                    issued_at: issued_at.clone(),
                    certificate_chain: certificate_chain.clone(),
                    private_key: private_key.clone(),
                },
            )
            .await?;
            active_managed_acm_certificate = true;
            plan.certificate = EndpointCertificate::AwsAcmArn(certificate_arn);
            None
        }
    };

    let endpoint = match &plan.route {
        KubernetesRouteProfile::Ingress(profile) => {
            let ingress_name = format!("{}-ingress", target.workload_name);
            let ingress = build_ingress(
                &target,
                &plan,
                profile,
                &service_name,
                &ingress_name,
                tls_ref.as_ref(),
            )?;
            upsert_ingress(
                &route_client,
                target.namespace,
                &ingress_name,
                ingress,
                target.resource_id,
            )
            .await?;
            state.ingress_name = Some(ingress_name.clone());
            state.gateway_name = None;
            state.http_route_name = None;
            state.gke_health_check_policy_name = None;
            state.azure_health_check_policy_name = None;
            observe_ingress_endpoint(&route_client, target.namespace, &ingress_name, profile)
                .await?
        }
        KubernetesRouteProfile::Gateway(profile) => {
            let gateway_name = format!("{}-gateway", target.workload_name);
            let route_name = format!("{}-route", target.workload_name);
            let gateway = build_gateway(&target, &plan, profile, &gateway_name, tls_ref.as_ref())?;
            let http_route =
                build_http_route(&target, &plan, &service_name, &gateway_name, &route_name);
            let health_check_policy_name =
                gke_health_check_policy_name(&target, &plan, &service_name);
            let azure_health_check_policy_name =
                azure_health_check_policy_name(&target, &plan, &service_name);
            upsert_gateway(
                &route_client,
                target.namespace,
                &gateway_name,
                gateway,
                target.resource_id,
            )
            .await?;
            upsert_http_route(
                &route_client,
                target.namespace,
                &route_name,
                http_route,
                target.resource_id,
            )
            .await?;
            if let Some((policy_name, health_check_path)) = health_check_policy_name
                .as_deref()
                .zip(target.health_check_path.as_deref())
            {
                let policy = build_gke_health_check_policy(
                    &target,
                    &service_name,
                    policy_name,
                    health_check_path,
                );
                upsert_gke_health_check_policy(
                    &route_client,
                    target.namespace,
                    policy_name,
                    policy,
                    target.resource_id,
                )
                .await?;
            }
            if let Some((policy_name, health_check_path)) = azure_health_check_policy_name
                .as_deref()
                .zip(target.health_check_path.as_deref())
            {
                let policy = build_azure_health_check_policy(
                    &target,
                    &service_name,
                    policy_name,
                    health_check_path,
                );
                upsert_azure_health_check_policy(
                    &route_client,
                    target.namespace,
                    policy_name,
                    policy,
                    target.resource_id,
                )
                .await?;
            }
            state.gateway_name = Some(gateway_name.clone());
            state.http_route_name = Some(route_name);
            state.gke_health_check_policy_name = health_check_policy_name;
            state.azure_health_check_policy_name = azure_health_check_policy_name;
            state.ingress_name = None;
            observe_gateway_endpoint(&route_client, target.namespace, &gateway_name).await?
        }
    };

    cleanup_stale_endpoint_objects(
        ctx,
        target.namespace,
        target.resource_id,
        &route_client,
        PreviousEndpointObjects {
            ingress_name: previous_ingress_name,
            gateway_name: previous_gateway_name,
            http_route_name: previous_http_route_name,
            gke_health_check_policy_name: previous_gke_health_check_policy_name,
            azure_health_check_policy_name: previous_azure_health_check_policy_name,
            managed_tls_secret_name: previous_managed_tls_secret_name,
        },
        ActiveEndpointObjects {
            ingress_name: state.ingress_name.clone(),
            gateway_name: state.gateway_name.clone(),
            http_route_name: state.http_route_name.clone(),
            gke_health_check_policy_name: state.gke_health_check_policy_name.clone(),
            azure_health_check_policy_name: state.azure_health_check_policy_name.clone(),
            managed_tls_secret_name: active_managed_tls_secret_name,
            managed_acm_certificate: active_managed_acm_certificate,
        },
        state,
    )
    .await?;

    state.public_url = plan
        .public_url
        .clone()
        .or_else(|| endpoint.as_ref().map(load_balancer_endpoint_url));
    state.load_balancer_endpoint = endpoint;

    if state.load_balancer_endpoint.is_none() {
        return Ok(KubernetesEndpointAction::Waiting {
            suggested_delay: ENDPOINT_WAIT,
        });
    }

    Ok(KubernetesEndpointAction::Ready)
}

pub(crate) async fn delete_kubernetes_public_endpoint(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    namespace: &str,
    state: &mut KubernetesPublicEndpointState,
) -> Result<()> {
    let kubernetes_config = ctx.get_kubernetes_config()?;
    let service_client = ctx
        .service_provider
        .get_kubernetes_service_client(kubernetes_config)
        .await?;
    let route_client = ctx
        .service_provider
        .get_kubernetes_route_client(kubernetes_config)
        .await?;
    let secrets_client = ctx
        .service_provider
        .get_kubernetes_secrets_client(kubernetes_config)
        .await?;

    if let Some(route_name) = state.http_route_name.take() {
        delete_not_found_ok(
            route_client.delete_http_route(namespace, &route_name).await,
            &route_name,
        )?;
    }
    if let Some(policy_name) = state.gke_health_check_policy_name.take() {
        delete_not_found_ok(
            route_client
                .delete_gke_health_check_policy(namespace, &policy_name)
                .await,
            &policy_name,
        )?;
    }
    if let Some(policy_name) = state.azure_health_check_policy_name.take() {
        delete_not_found_ok(
            route_client
                .delete_azure_health_check_policy(namespace, &policy_name)
                .await,
            &policy_name,
        )?;
    }
    if let Some(gateway_name) = state.gateway_name.take() {
        delete_not_found_ok(
            route_client.delete_gateway(namespace, &gateway_name).await,
            &gateway_name,
        )?;
    }
    if let Some(ingress_name) = state.ingress_name.take() {
        delete_not_found_ok(
            route_client.delete_ingress(namespace, &ingress_name).await,
            &ingress_name,
        )?;
    }
    if let Some(service_name) = state.service_name.take() {
        delete_not_found_ok(
            service_client
                .delete_service(namespace, &service_name)
                .await,
            &service_name,
        )?;
    }
    if let Some(secret_name) = state.managed_tls_secret_name.take() {
        delete_not_found_ok(
            secrets_client.delete_secret(namespace, &secret_name).await,
            &secret_name,
        )?;
    }
    delete_managed_acm_certificate(ctx, resource_id, state).await?;

    state.public_url = None;
    state.load_balancer_endpoint = None;
    state.published_certificate_id = None;
    state.published_certificate_issued_at = None;
    Ok(())
}

struct PreviousEndpointObjects {
    ingress_name: Option<String>,
    gateway_name: Option<String>,
    http_route_name: Option<String>,
    gke_health_check_policy_name: Option<String>,
    azure_health_check_policy_name: Option<String>,
    managed_tls_secret_name: Option<String>,
}

struct ActiveEndpointObjects {
    ingress_name: Option<String>,
    gateway_name: Option<String>,
    http_route_name: Option<String>,
    gke_health_check_policy_name: Option<String>,
    azure_health_check_policy_name: Option<String>,
    managed_tls_secret_name: Option<String>,
    managed_acm_certificate: bool,
}

async fn cleanup_stale_endpoint_objects(
    ctx: &ResourceControllerContext<'_>,
    namespace: &str,
    resource_id: &str,
    route_client: &std::sync::Arc<dyn alien_k8s_clients::RouteApi>,
    previous: PreviousEndpointObjects,
    active: ActiveEndpointObjects,
    state: &mut KubernetesPublicEndpointState,
) -> Result<()> {
    if let Some(route_name) = previous.http_route_name {
        if Some(route_name.as_str()) != active.http_route_name.as_deref() {
            delete_not_found_ok(
                route_client.delete_http_route(namespace, &route_name).await,
                &route_name,
            )?;
        }
    }
    if let Some(policy_name) = previous.gke_health_check_policy_name {
        if Some(policy_name.as_str()) != active.gke_health_check_policy_name.as_deref() {
            delete_not_found_ok(
                route_client
                    .delete_gke_health_check_policy(namespace, &policy_name)
                    .await,
                &policy_name,
            )?;
        }
    }
    if let Some(policy_name) = previous.azure_health_check_policy_name {
        if Some(policy_name.as_str()) != active.azure_health_check_policy_name.as_deref() {
            delete_not_found_ok(
                route_client
                    .delete_azure_health_check_policy(namespace, &policy_name)
                    .await,
                &policy_name,
            )?;
        }
    }
    if let Some(gateway_name) = previous.gateway_name {
        if Some(gateway_name.as_str()) != active.gateway_name.as_deref() {
            delete_not_found_ok(
                route_client.delete_gateway(namespace, &gateway_name).await,
                &gateway_name,
            )?;
        }
    }
    if let Some(ingress_name) = previous.ingress_name {
        if Some(ingress_name.as_str()) != active.ingress_name.as_deref() {
            delete_not_found_ok(
                route_client.delete_ingress(namespace, &ingress_name).await,
                &ingress_name,
            )?;
        }
    }

    if let Some(secret_name) = previous.managed_tls_secret_name {
        if Some(secret_name.as_str()) != active.managed_tls_secret_name.as_deref() {
            let kubernetes_config = ctx.get_kubernetes_config()?;
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            delete_not_found_ok(
                secrets_client.delete_secret(namespace, &secret_name).await,
                &secret_name,
            )?;
        }
    }
    state.managed_tls_secret_name = active.managed_tls_secret_name;
    state.gke_health_check_policy_name = active.gke_health_check_policy_name;
    state.azure_health_check_policy_name = active.azure_health_check_policy_name;

    if !active.managed_acm_certificate {
        delete_managed_acm_certificate(ctx, resource_id, state).await?;
    }

    Ok(())
}

#[cfg(feature = "aws")]
async fn publish_managed_acm_certificate(
    ctx: &ResourceControllerContext<'_>,
    target: &KubernetesPublicEndpointTarget<'_>,
    state: &mut KubernetesPublicEndpointState,
    input: ManagedAcmCertificateInput,
) -> Result<String> {
    if let Some(certificate_arn) = &state.managed_acm_certificate_arn {
        if state.published_certificate_id.as_deref() == Some(input.certificate_id.as_str())
            && state.published_certificate_issued_at == input.issued_at
        {
            return Ok(certificate_arn.clone());
        }
    }

    let mut aws_config = ctx.get_aws_config()?.clone();
    if let Some(region) = input.region.as_ref() {
        aws_config.region = region.clone();
    }

    let acm_client = ctx.service_provider.get_aws_acm_client(&aws_config).await?;
    let tags = acm_tags(ctx.resource_prefix, target.resource_id, input.tags);
    let (leaf, chain) = split_certificate_chain(&input.certificate_chain);

    let certificate_arn = if let Some(certificate_arn) = state.managed_acm_certificate_arn.clone() {
        acm_client
            .reimport_certificate(
                alien_aws_clients::acm::ReimportCertificateRequest::builder()
                    .certificate_arn(certificate_arn.clone())
                    .certificate(leaf)
                    .private_key(input.private_key)
                    .maybe_certificate_chain(chain)
                    .tags(tags)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to re-import Kubernetes public endpoint certificate to ACM"
                    .to_string(),
                resource_id: Some(target.resource_id.to_string()),
            })?;
        certificate_arn
    } else {
        acm_client
            .import_certificate(
                alien_aws_clients::acm::ImportCertificateRequest::builder()
                    .certificate(leaf)
                    .private_key(input.private_key)
                    .maybe_certificate_chain(chain)
                    .tags(tags)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import Kubernetes public endpoint certificate to ACM"
                    .to_string(),
                resource_id: Some(target.resource_id.to_string()),
            })?
            .certificate_arn
    };

    state.managed_acm_certificate_arn = Some(certificate_arn.clone());
    state.published_certificate_id = Some(input.certificate_id);
    state.published_certificate_issued_at = input.issued_at;

    Ok(certificate_arn)
}

#[cfg(not(feature = "aws"))]
async fn publish_managed_acm_certificate(
    _ctx: &ResourceControllerContext<'_>,
    target: &KubernetesPublicEndpointTarget<'_>,
    _state: &mut KubernetesPublicEndpointState,
    _input: ManagedAcmCertificateInput,
) -> Result<String> {
    Err(AlienError::new(ErrorData::ResourceControllerConfigError {
        resource_id: target.resource_id.to_string(),
        message: "managedAcmImport certificate mode requires the aws feature".to_string(),
    }))
}

#[cfg(feature = "aws")]
async fn delete_managed_acm_certificate(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    state: &mut KubernetesPublicEndpointState,
) -> Result<()> {
    let Some(certificate_arn) = state.managed_acm_certificate_arn.clone() else {
        return Ok(());
    };

    let aws_config = ctx.get_aws_config()?;
    let acm_client = ctx.service_provider.get_aws_acm_client(aws_config).await?;
    match acm_client.delete_certificate(&certificate_arn).await {
        Ok(()) => {
            info!(certificate_arn=%certificate_arn, "Deleted Kubernetes public endpoint ACM certificate");
            state.managed_acm_certificate_arn = None;
            Ok(())
        }
        Err(e)
            if matches!(
                e.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            state.managed_acm_certificate_arn = None;
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: "Failed to delete Kubernetes public endpoint ACM certificate".to_string(),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

#[cfg(not(feature = "aws"))]
async fn delete_managed_acm_certificate(
    _ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    state: &mut KubernetesPublicEndpointState,
) -> Result<()> {
    if state.managed_acm_certificate_arn.is_none() {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::ResourceControllerConfigError {
        resource_id: resource_id.to_string(),
        message: "Deleting a managed ACM certificate requires the aws feature".to_string(),
    }))
}

pub(crate) fn worker_public_endpoint_target<'a>(
    resource_id: &'a str,
    workload_name: &'a str,
    namespace: &'a str,
    selector: BTreeMap<String, String>,
    ingress: &Ingress,
    health_check_path: Option<&str>,
) -> KubernetesPublicEndpointTarget<'a> {
    KubernetesPublicEndpointTarget {
        resource_id,
        workload_name,
        namespace,
        component: "worker",
        selector,
        service_port: 80,
        target_port: 8080,
        health_check_path: health_check_path.map(ToString::to_string),
        public: matches!(ingress, Ingress::Public),
    }
}

pub(crate) fn container_public_endpoint_target<'a>(
    resource_id: &'a str,
    workload_name: &'a str,
    namespace: &'a str,
    selector: BTreeMap<String, String>,
    ports: &'a [alien_core::ContainerPort],
    health_check_path: Option<&str>,
) -> Result<KubernetesPublicEndpointTarget<'a>> {
    let http_port = ports
        .iter()
        .find(|port| port.expose == Some(alien_core::ExposeProtocol::Http))
        .map(|port| port.port);

    Ok(KubernetesPublicEndpointTarget {
        resource_id,
        workload_name,
        namespace,
        component: "container",
        selector,
        service_port: http_port.unwrap_or(80),
        target_port: http_port.unwrap_or(80),
        health_check_path: health_check_path.map(ToString::to_string),
        public: http_port.is_some(),
    })
}

fn resolve_endpoint_plan(
    ctx: &ResourceControllerContext<'_>,
    target: &KubernetesPublicEndpointTarget<'_>,
) -> Result<EndpointPlanResolution> {
    if !target.public {
        return Ok(EndpointPlanResolution::Disabled);
    }

    let exposure = ctx
        .deployment_config
        .stack_settings
        .kubernetes
        .as_ref()
        .and_then(|settings| settings.exposure.as_ref());

    let Some(exposure) = exposure else {
        return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: target.resource_id.to_string(),
            message: "Public Kubernetes workload requires stackSettings.kubernetes.exposure"
                .to_string(),
        }));
    };

    match exposure {
        KubernetesExposureSettings::Disabled => Ok(EndpointPlanResolution::Disabled),
        KubernetesExposureSettings::Generated { route, certificate } => {
            let domain = ctx
                .deployment_config
                .domain_metadata
                .as_ref()
                .and_then(|metadata| metadata.resources.get(target.resource_id));

            let Some(domain) = domain else {
                if matches!(certificate, KubernetesCertificateMode::None) {
                    return Ok(EndpointPlanResolution::Ready(EndpointPlan {
                        hostname: None,
                        public_url: None,
                        route: route.clone(),
                        certificate: EndpointCertificate::None,
                    }));
                }

                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: target.resource_id.to_string(),
                    message:
                        "Generated Kubernetes exposure requires domainMetadata for managed TLS"
                            .to_string(),
                }));
            };

            if domain.certificate_status != CertificateStatus::Issued
                && !matches!(
                    certificate,
                    KubernetesCertificateMode::None
                        | KubernetesCertificateMode::AwsAcmArn { .. }
                        | KubernetesCertificateMode::TlsSecretRef(_)
                )
            {
                return Ok(EndpointPlanResolution::Waiting);
            }

            if domain.certificate_status != CertificateStatus::Issued
                && matches!(certificate, KubernetesCertificateMode::None)
            {
                return Ok(EndpointPlanResolution::Ready(EndpointPlan {
                    hostname: Some(domain.fqdn.clone()),
                    public_url: Some(format!("http://{}", domain.fqdn)),
                    route: route.clone(),
                    certificate: EndpointCertificate::None,
                }));
            }

            if domain.certificate_status != CertificateStatus::Issued {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: target.resource_id.to_string(),
                    message:
                        "Generated Kubernetes exposure references a non-managed certificate but domainMetadata certificate is not issued"
                            .to_string(),
                }));
            }

            let certificate = match certificate {
                KubernetesCertificateMode::ManagedTlsSecret {
                    secret_name_template,
                } => {
                    let certificate_chain = domain.certificate_chain.clone().ok_or_else(|| {
                        AlienError::new(ErrorData::ResourceControllerConfigError {
                            resource_id: target.resource_id.to_string(),
                            message: "Issued Kubernetes certificate is missing certificateChain"
                                .to_string(),
                        })
                    })?;
                    let private_key = domain.private_key.clone().ok_or_else(|| {
                        AlienError::new(ErrorData::ResourceControllerConfigError {
                            resource_id: target.resource_id.to_string(),
                            message: "Issued Kubernetes certificate is missing privateKey"
                                .to_string(),
                        })
                    })?;
                    EndpointCertificate::ManagedTlsSecret {
                        name: render_secret_name_template(
                            secret_name_template,
                            target.resource_id,
                            target.workload_name,
                        ),
                        certificate_id: domain.certificate_id.clone(),
                        issued_at: domain.issued_at.clone(),
                        certificate_chain,
                        private_key,
                    }
                }
                KubernetesCertificateMode::ManagedAcmImport { region, tags } => {
                    let certificate_chain = domain.certificate_chain.clone().ok_or_else(|| {
                        AlienError::new(ErrorData::ResourceControllerConfigError {
                            resource_id: target.resource_id.to_string(),
                            message: "Issued Kubernetes certificate is missing certificateChain"
                                .to_string(),
                        })
                    })?;
                    let private_key = domain.private_key.clone().ok_or_else(|| {
                        AlienError::new(ErrorData::ResourceControllerConfigError {
                            resource_id: target.resource_id.to_string(),
                            message: "Issued Kubernetes certificate is missing privateKey"
                                .to_string(),
                        })
                    })?;
                    EndpointCertificate::ManagedAcmImport {
                        region: region.clone(),
                        tags: tags.clone(),
                        certificate_id: domain.certificate_id.clone(),
                        issued_at: domain.issued_at.clone(),
                        certificate_chain,
                        private_key,
                    }
                }
                KubernetesCertificateMode::None => EndpointCertificate::None,
                KubernetesCertificateMode::AwsAcmArn { certificate_arn } => {
                    EndpointCertificate::AwsAcmArn(certificate_arn.clone())
                }
                KubernetesCertificateMode::TlsSecretRef(secret_ref) => {
                    EndpointCertificate::TlsSecretRef(secret_ref.clone())
                }
            };

            let public_url = if matches!(certificate, EndpointCertificate::None) {
                format!("http://{}", domain.fqdn)
            } else {
                format!("https://{}", domain.fqdn)
            };

            Ok(EndpointPlanResolution::Ready(EndpointPlan {
                hostname: Some(domain.fqdn.clone()),
                public_url: Some(public_url),
                route: route.clone(),
                certificate,
            }))
        }
        KubernetesExposureSettings::Custom {
            domain,
            route,
            certificate,
        } => {
            let certificate = match certificate {
                KubernetesCertificateMode::TlsSecretRef(secret_ref) => {
                    EndpointCertificate::TlsSecretRef(secret_ref.clone())
                }
                KubernetesCertificateMode::AwsAcmArn { certificate_arn } => {
                    EndpointCertificate::AwsAcmArn(certificate_arn.clone())
                }
                KubernetesCertificateMode::None => EndpointCertificate::None,
                KubernetesCertificateMode::ManagedTlsSecret { .. }
                | KubernetesCertificateMode::ManagedAcmImport { .. } => {
                    return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: target.resource_id.to_string(),
                        message: "Custom Kubernetes exposure must reference customer-owned certificate material".to_string(),
                    }));
                }
            };
            let public_url = if matches!(certificate, EndpointCertificate::None) {
                format!("http://{}", domain)
            } else {
                format!("https://{}", domain)
            };
            Ok(EndpointPlanResolution::Ready(EndpointPlan {
                hostname: Some(domain.clone()),
                public_url: Some(public_url),
                route: route.clone(),
                certificate,
            }))
        }
    }
}

fn build_service(target: &KubernetesPublicEndpointTarget<'_>, service_name: &str) -> Service {
    Service {
        metadata: ObjectMeta {
            name: Some(service_name.to_string()),
            namespace: Some(target.namespace.to_string()),
            labels: Some(endpoint_labels(target, service_name)),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            type_: Some("ClusterIP".to_string()),
            selector: Some(target.selector.clone()),
            ports: Some(vec![ServicePort {
                name: Some("http".to_string()),
                port: target.service_port as i32,
                protocol: Some("TCP".to_string()),
                target_port: Some(IntOrString::Int(target.target_port as i32)),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_ingress(
    target: &KubernetesPublicEndpointTarget<'_>,
    plan: &EndpointPlan,
    profile: &KubernetesIngressRouteProfile,
    service_name: &str,
    ingress_name: &str,
    tls_ref: Option<&KubernetesTlsSecretRef>,
) -> Result<K8sIngress> {
    let mut annotations = profile.annotations.clone();
    if let Some(KubernetesRouteProviderOptions::AwsAlb { target_type, .. }) = &profile.provider {
        annotations
            .entry("alb.ingress.kubernetes.io/target-type".to_string())
            .or_insert_with(|| target_type.clone());
        if let Some(path) = target.health_check_path.as_ref() {
            annotations
                .entry("alb.ingress.kubernetes.io/healthcheck-path".to_string())
                .or_insert_with(|| path.clone());
            annotations
                .entry("alb.ingress.kubernetes.io/success-codes".to_string())
                .or_insert_with(|| "200".to_string());
        }
    }
    if let EndpointCertificate::AwsAcmArn(certificate_arn) = &plan.certificate {
        match &profile.provider {
            Some(KubernetesRouteProviderOptions::AwsAlb { .. }) => {
                annotations.insert(
                    "alb.ingress.kubernetes.io/certificate-arn".to_string(),
                    certificate_arn.clone(),
                );
            }
            _ => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: target.resource_id.to_string(),
                    message: "awsAcmArn certificate mode requires an AWS ALB Ingress route profile"
                        .to_string(),
                }));
            }
        }
    }

    let metadata = ObjectMeta {
        name: Some(ingress_name.to_string()),
        namespace: Some(target.namespace.to_string()),
        labels: Some(merge_labels(
            endpoint_labels(target, ingress_name),
            &profile.labels,
        )),
        annotations: if annotations.is_empty() {
            None
        } else {
            Some(annotations.into_iter().collect())
        },
        ..Default::default()
    };

    Ok(K8sIngress {
        metadata,
        spec: Some(IngressSpec {
            ingress_class_name: Some(profile.ingress_class_name.clone()),
            rules: Some(vec![IngressRule {
                host: plan.hostname.clone(),
                http: Some(HTTPIngressRuleValue {
                    paths: vec![HTTPIngressPath {
                        path: Some("/".to_string()),
                        path_type: "Prefix".to_string(),
                        backend: IngressBackend {
                            service: Some(IngressServiceBackend {
                                name: service_name.to_string(),
                                port: Some(ServiceBackendPort {
                                    number: Some(target.service_port as i32),
                                    name: None,
                                }),
                            }),
                            ..Default::default()
                        },
                    }],
                }),
            }]),
            tls: tls_ref.and_then(|secret| {
                plan.hostname.as_ref().map(|hostname| {
                    vec![IngressTLS {
                        hosts: Some(vec![hostname.clone()]),
                        secret_name: Some(secret.secret_name.clone()),
                    }]
                })
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn is_aws_alb_ingress(route: &KubernetesRouteProfile) -> bool {
    matches!(
        route,
        KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
            provider: Some(KubernetesRouteProviderOptions::AwsAlb { .. }),
            ..
        })
    )
}

fn build_gateway(
    target: &KubernetesPublicEndpointTarget<'_>,
    plan: &EndpointPlan,
    profile: &KubernetesGatewayRouteProfile,
    gateway_name: &str,
    tls_ref: Option<&KubernetesTlsSecretRef>,
) -> Result<Value> {
    if tls_ref.is_none() && !matches!(plan.certificate, EndpointCertificate::None) {
        return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: target.resource_id.to_string(),
            message: "Gateway HTTPS exposure requires a Kubernetes TLS Secret reference"
                .to_string(),
        }));
    }

    let labels = merge_labels(endpoint_labels(target, gateway_name), &profile.labels);
    let mut annotations = btree_from_hash(&profile.annotations);
    if let Some(KubernetesRouteProviderOptions::AzureApplicationGatewayForContainers {
        alb_namespace,
        alb_name,
        ..
    }) = &profile.provider
    {
        if let Some(alb_namespace) = alb_namespace {
            annotations
                .entry("alb.networking.azure.io/alb-namespace".to_string())
                .or_insert_with(|| alb_namespace.clone());
        }
        if let Some(alb_name) = alb_name {
            annotations
                .entry("alb.networking.azure.io/alb-name".to_string())
                .or_insert_with(|| alb_name.clone());
        }
    }
    let uses_tls = tls_ref.is_some();
    let mut listener = json!({
        "name": if uses_tls { "https" } else { "http" },
        "port": profile.listener_port,
        "protocol": if uses_tls { "HTTPS" } else { "HTTP" },
        "allowedRoutes": {
            "namespaces": { "from": "Same" }
        }
    });
    if let Some(hostname) = &plan.hostname {
        listener["hostname"] = json!(hostname);
    }

    if let Some(secret_ref) = tls_ref {
        listener["tls"] = json!({
            "mode": "Terminate",
            "certificateRefs": [{
                "kind": "Secret",
                "name": secret_ref.secret_name,
            }]
        });
    }

    Ok(json!({
        "apiVersion": "gateway.networking.k8s.io/v1",
        "kind": "Gateway",
        "metadata": {
            "name": gateway_name,
            "namespace": target.namespace,
            "labels": labels,
            "annotations": annotations,
        },
        "spec": {
            "gatewayClassName": profile.gateway_class_name,
            "listeners": [listener],
        }
    }))
}

fn build_http_route(
    target: &KubernetesPublicEndpointTarget<'_>,
    plan: &EndpointPlan,
    service_name: &str,
    gateway_name: &str,
    route_name: &str,
) -> Value {
    let mut route = json!({
        "apiVersion": "gateway.networking.k8s.io/v1",
        "kind": "HTTPRoute",
        "metadata": {
            "name": route_name,
            "namespace": target.namespace,
            "labels": endpoint_labels(target, route_name),
        },
        "spec": {
            "parentRefs": [{
                "name": gateway_name,
            }],
            "rules": [{
                "matches": [{
                    "path": {
                        "type": "PathPrefix",
                        "value": "/",
                    }
                }],
                "backendRefs": [{
                    "name": service_name,
                    "port": target.service_port,
                }]
            }]
        }
    });
    if let Some(hostname) = &plan.hostname {
        route["spec"]["hostnames"] = json!([hostname]);
    }
    route
}

fn gke_health_check_policy_name(
    target: &KubernetesPublicEndpointTarget<'_>,
    plan: &EndpointPlan,
    service_name: &str,
) -> Option<String> {
    let is_gke_gateway = matches!(
        &plan.route,
        KubernetesRouteProfile::Gateway(KubernetesGatewayRouteProfile {
            provider: Some(KubernetesRouteProviderOptions::GkeGateway { .. }),
            ..
        })
    );
    if is_gke_gateway && target.health_check_path.is_some() {
        Some(format!("{service_name}-health-check"))
    } else {
        None
    }
}

fn build_gke_health_check_policy(
    target: &KubernetesPublicEndpointTarget<'_>,
    service_name: &str,
    policy_name: &str,
    health_check_path: &str,
) -> Value {
    json!({
        "apiVersion": "networking.gke.io/v1",
        "kind": "HealthCheckPolicy",
        "metadata": {
            "name": policy_name,
            "namespace": target.namespace,
            "labels": endpoint_labels(target, policy_name),
        },
        "spec": {
            "default": {
                "checkIntervalSec": 15,
                "timeoutSec": 15,
                "healthyThreshold": 1,
                "unhealthyThreshold": 2,
                "config": {
                    "type": "HTTP",
                    "httpHealthCheck": {
                        "requestPath": health_check_path,
                    },
                },
            },
            "targetRef": {
                "group": "",
                "kind": "Service",
                "name": service_name,
            },
        },
    })
}

fn azure_health_check_policy_name(
    target: &KubernetesPublicEndpointTarget<'_>,
    plan: &EndpointPlan,
    service_name: &str,
) -> Option<String> {
    let is_azure_agc_gateway = matches!(
        &plan.route,
        KubernetesRouteProfile::Gateway(KubernetesGatewayRouteProfile {
            provider: Some(
                KubernetesRouteProviderOptions::AzureApplicationGatewayForContainers { .. }
            ),
            ..
        })
    );
    if is_azure_agc_gateway && target.health_check_path.is_some() {
        Some(format!("{service_name}-health-check"))
    } else {
        None
    }
}

fn build_azure_health_check_policy(
    target: &KubernetesPublicEndpointTarget<'_>,
    service_name: &str,
    policy_name: &str,
    health_check_path: &str,
) -> Value {
    json!({
        "apiVersion": "alb.networking.azure.io/v1",
        "kind": "HealthCheckPolicy",
        "metadata": {
            "name": policy_name,
            "namespace": target.namespace,
            "labels": endpoint_labels(target, policy_name),
        },
        "spec": {
            "targetRef": {
                "group": "",
                "kind": "Service",
                "name": service_name,
            },
            "default": {
                "interval": "5s",
                "timeout": "3s",
                "healthyThreshold": 1,
                "unhealthyThreshold": 1,
                "port": target.service_port,
                "http": {
                    "host": "localhost",
                    "path": health_check_path,
                    "match": {
                        "statusCodes": [{
                            "start": 200,
                            "end": 299,
                        }],
                    },
                },
            },
        },
    })
}

async fn upsert_service(
    client: &std::sync::Arc<dyn alien_k8s_clients::ServiceApi>,
    namespace: &str,
    name: &str,
    mut service: Service,
    resource_id: &str,
) -> Result<()> {
    match client.create_service(namespace, &service).await {
        Ok(_) => Ok(()),
        Err(e) if is_already_exists(&e) => {
            let existing = client.get_service(namespace, name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get Service '{}' before update", name),
                    resource_id: Some(resource_id.to_string()),
                },
            )?;
            service.metadata.resource_version = existing.metadata.resource_version;
            client
                .update_service(namespace, name, &service)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update Service '{}'", name),
                    resource_id: Some(resource_id.to_string()),
                })?;
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!("Failed to create Service '{}'", name),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

async fn upsert_tls_secret(
    client: &std::sync::Arc<dyn alien_k8s_clients::SecretsApi>,
    namespace: &str,
    name: &str,
    certificate_chain: &str,
    private_key: &str,
    certificate_id: &str,
    issued_at: Option<&str>,
    resource_id: &str,
) -> Result<()> {
    let mut annotations = BTreeMap::new();
    annotations.insert("certificate-id".to_string(), certificate_id.to_string());
    if let Some(issued_at) = issued_at {
        annotations.insert("certificate-issued-at".to_string(), issued_at.to_string());
    }

    let mut secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            annotations: Some(annotations),
            ..Default::default()
        },
        type_: Some("kubernetes.io/tls".to_string()),
        data: Some(BTreeMap::from([
            (
                "tls.crt".to_string(),
                ByteString(certificate_chain.as_bytes().to_vec()),
            ),
            (
                "tls.key".to_string(),
                ByteString(private_key.as_bytes().to_vec()),
            ),
        ])),
        ..Default::default()
    };

    match client.create_secret(namespace, &secret).await {
        Ok(_) => Ok(()),
        Err(e) if is_already_exists(&e) => {
            let existing = client.get_secret(namespace, name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get TLS Secret '{}' before update", name),
                    resource_id: Some(resource_id.to_string()),
                },
            )?;
            secret.metadata.resource_version = existing.metadata.resource_version;
            client
                .update_secret(namespace, name, &secret)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update TLS Secret '{}'", name),
                    resource_id: Some(resource_id.to_string()),
                })?;
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!("Failed to create TLS Secret '{}'", name),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

async fn upsert_ingress(
    client: &std::sync::Arc<dyn alien_k8s_clients::RouteApi>,
    namespace: &str,
    name: &str,
    mut ingress: K8sIngress,
    resource_id: &str,
) -> Result<()> {
    match client.create_ingress(namespace, &ingress).await {
        Ok(_) => Ok(()),
        Err(e) if is_already_exists(&e) => {
            let existing = client.get_ingress(namespace, name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get Ingress '{}' before update", name),
                    resource_id: Some(resource_id.to_string()),
                },
            )?;
            ingress.metadata.resource_version = existing.metadata.resource_version;
            client
                .update_ingress(namespace, name, &ingress)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update Ingress '{}'", name),
                    resource_id: Some(resource_id.to_string()),
                })?;
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!("Failed to create Ingress '{}'", name),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

async fn upsert_gateway(
    client: &std::sync::Arc<dyn alien_k8s_clients::RouteApi>,
    namespace: &str,
    name: &str,
    mut gateway: Value,
    resource_id: &str,
) -> Result<()> {
    match client.create_gateway(namespace, &gateway).await {
        Ok(_) => Ok(()),
        Err(e) if is_already_exists(&e) => {
            let existing = client.get_gateway(namespace, name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get Gateway '{}' before update", name),
                    resource_id: Some(resource_id.to_string()),
                },
            )?;
            copy_resource_version(&mut gateway, &existing);
            client
                .update_gateway(namespace, name, &gateway)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update Gateway '{}'", name),
                    resource_id: Some(resource_id.to_string()),
                })?;
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!("Failed to create Gateway '{}'", name),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

async fn upsert_http_route(
    client: &std::sync::Arc<dyn alien_k8s_clients::RouteApi>,
    namespace: &str,
    name: &str,
    mut route: Value,
    resource_id: &str,
) -> Result<()> {
    match client.create_http_route(namespace, &route).await {
        Ok(_) => Ok(()),
        Err(e) if is_already_exists(&e) => {
            let existing = client.get_http_route(namespace, name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get HTTPRoute '{}' before update", name),
                    resource_id: Some(resource_id.to_string()),
                },
            )?;
            copy_resource_version(&mut route, &existing);
            client
                .update_http_route(namespace, name, &route)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update HTTPRoute '{}'", name),
                    resource_id: Some(resource_id.to_string()),
                })?;
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!("Failed to create HTTPRoute '{}'", name),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

async fn upsert_gke_health_check_policy(
    client: &std::sync::Arc<dyn alien_k8s_clients::RouteApi>,
    namespace: &str,
    name: &str,
    mut policy: Value,
    resource_id: &str,
) -> Result<()> {
    match client
        .create_gke_health_check_policy(namespace, &policy)
        .await
    {
        Ok(_) => Ok(()),
        Err(e) if is_already_exists(&e) => {
            let existing = client
                .get_gke_health_check_policy(namespace, name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to get GKE HealthCheckPolicy '{}' before update",
                        name
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;
            copy_resource_version(&mut policy, &existing);
            client
                .update_gke_health_check_policy(namespace, name, &policy)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update GKE HealthCheckPolicy '{}'", name),
                    resource_id: Some(resource_id.to_string()),
                })?;
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!("Failed to create GKE HealthCheckPolicy '{}'", name),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

async fn upsert_azure_health_check_policy(
    client: &std::sync::Arc<dyn alien_k8s_clients::RouteApi>,
    namespace: &str,
    name: &str,
    mut policy: Value,
    resource_id: &str,
) -> Result<()> {
    match client
        .create_azure_health_check_policy(namespace, &policy)
        .await
    {
        Ok(_) => Ok(()),
        Err(e) if is_already_exists(&e) => {
            let existing = client
                .get_azure_health_check_policy(namespace, name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to get Azure HealthCheckPolicy '{}' before update",
                        name
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;
            copy_resource_version(&mut policy, &existing);
            client
                .update_azure_health_check_policy(namespace, name, &policy)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update Azure HealthCheckPolicy '{}'", name),
                    resource_id: Some(resource_id.to_string()),
                })?;
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!("Failed to create Azure HealthCheckPolicy '{}'", name),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

async fn observe_ingress_endpoint(
    client: &std::sync::Arc<dyn alien_k8s_clients::RouteApi>,
    namespace: &str,
    name: &str,
    profile: &KubernetesIngressRouteProfile,
) -> Result<Option<LoadBalancerEndpoint>> {
    let ingress =
        client
            .get_ingress(namespace, name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get Ingress '{}'", name),
                resource_id: None,
            })?;
    let Some(status) = ingress.status else {
        return Ok(None);
    };
    let Some(load_balancer) = status.load_balancer else {
        return Ok(None);
    };
    let Some(entries) = load_balancer.ingress else {
        return Ok(None);
    };
    let Some(entry) = entries.first() else {
        return Ok(None);
    };

    let dns_name = entry
        .hostname
        .clone()
        .or_else(|| entry.ip.clone())
        .filter(|value| !value.is_empty());

    Ok(dns_name.map(|dns_name| LoadBalancerEndpoint {
        dns_name,
        hosted_zone_id: aws_hosted_zone_id(profile),
    }))
}

async fn observe_gateway_endpoint(
    client: &std::sync::Arc<dyn alien_k8s_clients::RouteApi>,
    namespace: &str,
    name: &str,
) -> Result<Option<LoadBalancerEndpoint>> {
    let gateway =
        client
            .get_gateway(namespace, name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get Gateway '{}'", name),
                resource_id: None,
            })?;
    let addresses = gateway
        .pointer("/status/addresses")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let dns_name = addresses
        .iter()
        .filter_map(|address| address.get("value").and_then(Value::as_str))
        .find(|value| !value.is_empty())
        .map(str::to_string);

    Ok(dns_name.map(|dns_name| LoadBalancerEndpoint {
        dns_name,
        hosted_zone_id: None,
    }))
}

fn load_balancer_endpoint_url(endpoint: &LoadBalancerEndpoint) -> String {
    if endpoint.dns_name.starts_with("http://") || endpoint.dns_name.starts_with("https://") {
        endpoint.dns_name.clone()
    } else {
        format!("http://{}", endpoint.dns_name)
    }
}

fn delete_not_found_ok(result: alien_client_core::Result<()>, name: &str) -> Result<()> {
    match result {
        Ok(()) => {
            info!(resource_name=%name, "Deleted Kubernetes public endpoint object");
            Ok(())
        }
        Err(e)
            if matches!(
                e.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to delete Kubernetes public endpoint object '{}'",
                name
            ),
            resource_id: None,
        })),
    }
}

fn endpoint_labels(
    target: &KubernetesPublicEndpointTarget<'_>,
    name: &str,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("app".to_string(), target.workload_name.to_string()),
        ("component".to_string(), target.component.to_string()),
        ("managed-by".to_string(), "runtime".to_string()),
        ("endpoint".to_string(), name.to_string()),
    ])
}

fn merge_labels(
    mut base: BTreeMap<String, String>,
    extra: &HashMap<String, String>,
) -> BTreeMap<String, String> {
    for (key, value) in extra {
        base.insert(key.clone(), value.clone());
    }
    base
}

fn btree_from_hash(values: &HashMap<String, String>) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(feature = "aws")]
fn acm_tags(
    resource_prefix: &str,
    resource_id: &str,
    mut custom_tags: HashMap<String, String>,
) -> Vec<alien_aws_clients::acm::Tag> {
    for (key, value) in alien_core::standard_resource_tags(resource_prefix, resource_id) {
        custom_tags.insert(key, value);
    }

    custom_tags
        .into_iter()
        .map(|(key, value)| alien_aws_clients::acm::Tag { key, value })
        .collect()
}

fn render_secret_name_template(template: &str, resource_id: &str, workload_name: &str) -> String {
    template
        .replace("{{ resourceId }}", resource_id)
        .replace("{{resourceId}}", resource_id)
        .replace("{{ resource_id }}", resource_id)
        .replace("{{resource_id}}", resource_id)
        .replace("{{ workloadName }}", workload_name)
        .replace("{{workloadName}}", workload_name)
}

fn resolve_tls_secret_namespace<'a>(
    configured_namespace: Option<&'a str>,
    release_namespace: &'a str,
    resource_id: &str,
) -> Result<&'a str> {
    match configured_namespace {
        Some(namespace) if namespace != release_namespace => {
            Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: resource_id.to_string(),
                message: format!(
                    "Kubernetes TLS Secret references must use the release namespace '{}'; cross-namespace Secret references are not supported",
                    release_namespace
                ),
            }))
        }
        Some(namespace) => Ok(namespace),
        None => Ok(release_namespace),
    }
}

fn copy_resource_version(resource: &mut Value, existing: &Value) {
    if let Some(resource_version) = existing
        .pointer("/metadata/resourceVersion")
        .and_then(Value::as_str)
    {
        resource["metadata"]["resourceVersion"] = Value::String(resource_version.to_string());
    }
}

fn is_already_exists(error: &alien_client_core::Error) -> bool {
    let err = format!("{error}");
    err.contains("AlreadyExists") || err.contains("409")
}

fn aws_hosted_zone_id(profile: &KubernetesIngressRouteProfile) -> Option<String> {
    match &profile.provider {
        Some(KubernetesRouteProviderOptions::AwsAlb { .. }) => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ContainerPort, ExposeProtocol};

    fn endpoint_target() -> KubernetesPublicEndpointTarget<'static> {
        KubernetesPublicEndpointTarget {
            resource_id: "api",
            workload_name: "api-v1",
            namespace: "app",
            component: "container",
            selector: BTreeMap::from([("app".to_string(), "api".to_string())]),
            service_port: 8080,
            target_port: 8080,
            health_check_path: None,
            public: true,
        }
    }

    #[test]
    fn container_target_is_public_only_for_http_exposed_port() {
        let target = container_public_endpoint_target(
            "api",
            "api",
            "default",
            BTreeMap::new(),
            &[ContainerPort {
                port: 8080,
                expose: Some(ExposeProtocol::Http),
            }],
            None,
        )
        .expect("target");

        assert!(target.public);
        assert_eq!(target.service_port, 8080);
        assert_eq!(target.target_port, 8080);
    }

    #[test]
    fn ingress_with_byo_acm_arn_sets_alb_certificate_annotation() {
        let target = endpoint_target();
        let plan = EndpointPlan {
            hostname: Some("api.example.com".to_string()),
            public_url: Some("https://api.example.com".to_string()),
            route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
                ingress_class_name: "alb".to_string(),
                provider: Some(KubernetesRouteProviderOptions::AwsAlb {
                    scheme: "internet-facing".to_string(),
                    target_type: "ip".to_string(),
                    ip_address_type: None,
                    subnet_ids: vec![],
                }),
                ..Default::default()
            }),
            certificate: EndpointCertificate::AwsAcmArn(
                "arn:aws:acm:us-east-1:123456789012:certificate/customer".to_string(),
            ),
        };
        let KubernetesRouteProfile::Ingress(profile) = &plan.route else {
            panic!("expected ingress profile");
        };

        let ingress = build_ingress(&target, &plan, profile, "api-public", "api-ingress", None)
            .expect("ingress");
        let annotations = ingress
            .metadata
            .annotations
            .expect("ALB certificate annotation");

        assert_eq!(
            annotations.get("alb.ingress.kubernetes.io/certificate-arn"),
            Some(&"arn:aws:acm:us-east-1:123456789012:certificate/customer".to_string())
        );
    }

    #[test]
    fn aws_alb_ingress_uses_declared_health_check_path() {
        let mut target = endpoint_target();
        target.health_check_path = Some("/ready".to_string());
        let plan = EndpointPlan {
            hostname: None,
            public_url: None,
            route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
                ingress_class_name: "alb".to_string(),
                provider: Some(KubernetesRouteProviderOptions::AwsAlb {
                    scheme: "internet-facing".to_string(),
                    target_type: "ip".to_string(),
                    ip_address_type: None,
                    subnet_ids: vec![],
                }),
                ..Default::default()
            }),
            certificate: EndpointCertificate::None,
        };
        let KubernetesRouteProfile::Ingress(profile) = &plan.route else {
            panic!("expected ingress profile");
        };

        let ingress = build_ingress(&target, &plan, profile, "api-public", "api-ingress", None)
            .expect("ingress");
        let annotations = ingress
            .metadata
            .annotations
            .expect("ALB health check annotations");

        assert_eq!(
            annotations.get("alb.ingress.kubernetes.io/healthcheck-path"),
            Some(&"/ready".to_string())
        );
        assert_eq!(
            annotations.get("alb.ingress.kubernetes.io/success-codes"),
            Some(&"200".to_string())
        );
    }

    #[test]
    fn gke_gateway_uses_declared_health_check_policy_path() {
        let mut target = endpoint_target();
        target.health_check_path = Some("/ready".to_string());
        let plan = EndpointPlan {
            hostname: None,
            public_url: None,
            route: KubernetesRouteProfile::Gateway(KubernetesGatewayRouteProfile {
                gateway_class_name: "gke-l7-global-external-managed".to_string(),
                listener_port: 80,
                provider: Some(KubernetesRouteProviderOptions::GkeGateway {
                    static_address_name: None,
                }),
                ..Default::default()
            }),
            certificate: EndpointCertificate::None,
        };

        let policy_name = gke_health_check_policy_name(&target, &plan, "api-public")
            .expect("health check policy");
        let policy = build_gke_health_check_policy(&target, "api-public", &policy_name, "/ready");

        assert_eq!(policy_name, "api-public-health-check");
        assert_eq!(
            policy.pointer("/spec/default/config/httpHealthCheck/requestPath"),
            Some(&json!("/ready"))
        );
        assert_eq!(
            policy.pointer("/spec/targetRef/name"),
            Some(&json!("api-public"))
        );
    }

    #[test]
    fn azure_gateway_provider_sets_alb_reference_annotations() {
        let target = endpoint_target();
        let plan = EndpointPlan {
            hostname: None,
            public_url: None,
            route: KubernetesRouteProfile::Gateway(KubernetesGatewayRouteProfile {
                gateway_class_name: "azure-alb-external".to_string(),
                listener_port: 80,
                provider: Some(
                    KubernetesRouteProviderOptions::AzureApplicationGatewayForContainers {
                        alb_namespace: Some("alien-test".to_string()),
                        alb_name: Some("alien-alb".to_string()),
                        frontend: "public".to_string(),
                    },
                ),
                ..Default::default()
            }),
            certificate: EndpointCertificate::None,
        };
        let KubernetesRouteProfile::Gateway(profile) = &plan.route else {
            panic!("expected gateway profile");
        };

        let gateway = build_gateway(&target, &plan, profile, "api-gateway", None)
            .expect("gateway should render");

        assert_eq!(
            gateway.pointer("/metadata/annotations/alb.networking.azure.io~1alb-namespace"),
            Some(&json!("alien-test"))
        );
        assert_eq!(
            gateway.pointer("/metadata/annotations/alb.networking.azure.io~1alb-name"),
            Some(&json!("alien-alb"))
        );
    }

    #[test]
    fn azure_gateway_uses_declared_health_check_policy_path() {
        let mut target = endpoint_target();
        target.health_check_path = Some("/ready".to_string());
        let plan = EndpointPlan {
            hostname: None,
            public_url: None,
            route: KubernetesRouteProfile::Gateway(KubernetesGatewayRouteProfile {
                gateway_class_name: "azure-alb-external".to_string(),
                listener_port: 80,
                provider: Some(
                    KubernetesRouteProviderOptions::AzureApplicationGatewayForContainers {
                        alb_namespace: Some("alien-test".to_string()),
                        alb_name: Some("alien-alb".to_string()),
                        frontend: "public".to_string(),
                    },
                ),
                ..Default::default()
            }),
            certificate: EndpointCertificate::None,
        };

        let policy_name = azure_health_check_policy_name(&target, &plan, "api-public")
            .expect("health check policy");
        let policy = build_azure_health_check_policy(&target, "api-public", &policy_name, "/ready");

        assert_eq!(policy_name, "api-public-health-check");
        assert_eq!(
            policy.pointer("/spec/default/http/path"),
            Some(&json!("/ready"))
        );
        assert_eq!(
            policy.pointer("/spec/default/http/match/statusCodes/0/start"),
            Some(&json!(200))
        );
        assert_eq!(
            policy.pointer("/spec/default/port"),
            Some(&json!(target.service_port))
        );
        assert_eq!(
            policy.pointer("/spec/targetRef/name"),
            Some(&json!("api-public"))
        );
    }

    #[test]
    fn gke_gateway_without_declared_health_check_does_not_invent_policy() {
        let target = endpoint_target();
        let plan = EndpointPlan {
            hostname: None,
            public_url: None,
            route: KubernetesRouteProfile::Gateway(KubernetesGatewayRouteProfile {
                gateway_class_name: "gke-l7-global-external-managed".to_string(),
                listener_port: 80,
                provider: Some(KubernetesRouteProviderOptions::GkeGateway {
                    static_address_name: None,
                }),
                ..Default::default()
            }),
            certificate: EndpointCertificate::None,
        };

        assert_eq!(
            gke_health_check_policy_name(&target, &plan, "api-public"),
            None
        );
    }

    #[test]
    fn aws_alb_ingress_without_declared_health_check_does_not_invent_path() {
        let target = endpoint_target();
        let plan = EndpointPlan {
            hostname: None,
            public_url: None,
            route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
                ingress_class_name: "alb".to_string(),
                provider: Some(KubernetesRouteProviderOptions::AwsAlb {
                    scheme: "internet-facing".to_string(),
                    target_type: "ip".to_string(),
                    ip_address_type: None,
                    subnet_ids: vec![],
                }),
                ..Default::default()
            }),
            certificate: EndpointCertificate::None,
        };
        let KubernetesRouteProfile::Ingress(profile) = &plan.route else {
            panic!("expected ingress profile");
        };

        let ingress = build_ingress(&target, &plan, profile, "api-public", "api-ingress", None)
            .expect("ingress");

        assert_eq!(ingress.metadata.annotations, None);
    }

    #[test]
    fn endpoint_state_derives_url_from_observed_load_balancer() {
        let state = KubernetesPublicEndpointState {
            load_balancer_endpoint: Some(LoadBalancerEndpoint {
                dns_name: "k8s-api.example.elb.amazonaws.com".to_string(),
                hosted_zone_id: None,
            }),
            ..Default::default()
        };

        assert_eq!(
            state.effective_public_url().as_deref(),
            Some("http://k8s-api.example.elb.amazonaws.com")
        );
    }

    #[test]
    fn aws_alb_ingress_keeps_explicit_health_check_annotations() {
        let mut target = endpoint_target();
        target.health_check_path = Some("/ready".to_string());
        let plan = EndpointPlan {
            hostname: None,
            public_url: None,
            route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
                ingress_class_name: "alb".to_string(),
                provider: Some(KubernetesRouteProviderOptions::AwsAlb {
                    scheme: "internet-facing".to_string(),
                    target_type: "ip".to_string(),
                    ip_address_type: None,
                    subnet_ids: vec![],
                }),
                annotations: HashMap::from([
                    (
                        "alb.ingress.kubernetes.io/healthcheck-path".to_string(),
                        "/custom".to_string(),
                    ),
                    (
                        "alb.ingress.kubernetes.io/success-codes".to_string(),
                        "200-399".to_string(),
                    ),
                ]),
                ..Default::default()
            }),
            certificate: EndpointCertificate::None,
        };
        let KubernetesRouteProfile::Ingress(profile) = &plan.route else {
            panic!("expected ingress profile");
        };

        let ingress = build_ingress(&target, &plan, profile, "api-public", "api-ingress", None)
            .expect("ingress");
        let annotations = ingress.metadata.annotations.expect("annotations");

        assert_eq!(
            annotations.get("alb.ingress.kubernetes.io/healthcheck-path"),
            Some(&"/custom".to_string())
        );
        assert_eq!(
            annotations.get("alb.ingress.kubernetes.io/success-codes"),
            Some(&"200-399".to_string())
        );
    }

    #[test]
    fn ingress_without_hostname_omits_host_rule_and_tls() {
        let target = endpoint_target();
        let plan = EndpointPlan {
            hostname: None,
            public_url: None,
            route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
                ingress_class_name: "alb".to_string(),
                ..Default::default()
            }),
            certificate: EndpointCertificate::None,
        };
        let KubernetesRouteProfile::Ingress(profile) = &plan.route else {
            panic!("expected ingress profile");
        };

        let ingress = build_ingress(&target, &plan, profile, "api-public", "api-ingress", None)
            .expect("ingress");
        let spec = ingress.spec.expect("ingress spec");
        let rule = spec.rules.expect("rules").into_iter().next().expect("rule");

        assert_eq!(rule.host, None);
        assert_eq!(spec.tls, None);
    }

    #[test]
    fn gateway_with_byo_tls_secret_uses_same_namespace_certificate_ref() {
        let target = endpoint_target();
        let plan = EndpointPlan {
            hostname: Some("api.example.com".to_string()),
            public_url: Some("https://api.example.com".to_string()),
            route: KubernetesRouteProfile::Gateway(KubernetesGatewayRouteProfile {
                gateway_class_name: "shared-gateway".to_string(),
                listener_port: 443,
                ..Default::default()
            }),
            certificate: EndpointCertificate::TlsSecretRef(KubernetesTlsSecretRef {
                secret_name: "api-tls".to_string(),
                namespace: None,
            }),
        };
        let KubernetesRouteProfile::Gateway(profile) = &plan.route else {
            panic!("expected gateway profile");
        };
        let secret_ref = KubernetesTlsSecretRef {
            secret_name: "api-tls".to_string(),
            namespace: Some("app".to_string()),
        };

        let gateway = build_gateway(&target, &plan, profile, "api-gateway", Some(&secret_ref))
            .expect("gateway");

        assert_eq!(
            gateway.pointer("/kind").and_then(Value::as_str),
            Some("Gateway")
        );
        assert_eq!(
            gateway
                .pointer("/spec/gatewayClassName")
                .and_then(Value::as_str),
            Some("shared-gateway")
        );
        assert_eq!(
            gateway
                .pointer("/spec/listeners/0/tls/certificateRefs/0/name")
                .and_then(Value::as_str),
            Some("api-tls")
        );
    }

    #[test]
    fn secret_template_supports_resource_tokens() {
        assert_eq!(
            render_secret_name_template("deployment-{{ resourceId }}-tls", "api", "api-v1"),
            "deployment-api-tls"
        );
        assert_eq!(
            render_secret_name_template("alien-{{ workloadName }}-tls", "api", "api-v1"),
            "alien-api-v1-tls"
        );
    }

    #[test]
    fn aws_alb_detection_requires_ingress_provider() {
        let route = KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
            ingress_class_name: "alb".to_string(),
            provider: Some(KubernetesRouteProviderOptions::AwsAlb {
                scheme: "internet-facing".to_string(),
                target_type: "ip".to_string(),
                ip_address_type: None,
                subnet_ids: vec![],
            }),
            ..Default::default()
        });

        assert!(is_aws_alb_ingress(&route));
    }

    #[test]
    fn tls_secret_ref_defaults_to_release_namespace() {
        assert_eq!(
            resolve_tls_secret_namespace(None, "app", "api").expect("namespace"),
            "app"
        );
        assert_eq!(
            resolve_tls_secret_namespace(Some("app"), "app", "api").expect("namespace"),
            "app"
        );
    }

    #[test]
    fn tls_secret_ref_rejects_cross_namespace_reference() {
        let error = resolve_tls_secret_namespace(Some("shared"), "app", "api")
            .expect_err("cross-namespace refs fail");

        assert!(
            format!("{error}").contains("cross-namespace Secret references are not supported"),
            "unexpected error: {error}"
        );
    }

    #[cfg(feature = "aws")]
    #[test]
    fn acm_tags_keep_runtime_boundary_tags_authoritative() {
        let tags = acm_tags(
            "stack-1",
            "api",
            HashMap::from([
                ("deployment".to_string(), "wrong".to_string()),
                ("team".to_string(), "platform".to_string()),
            ]),
        );
        let tags: HashMap<_, _> = tags.into_iter().map(|tag| (tag.key, tag.value)).collect();

        assert_eq!(tags.get("deployment"), Some(&"stack-1".to_string()));
        assert_eq!(tags.get("resource"), Some(&"api".to_string()));
        assert_eq!(tags.get("managed-by"), Some(&"runtime".to_string()));
        assert_eq!(tags.get("team"), Some(&"platform".to_string()));
    }
}
