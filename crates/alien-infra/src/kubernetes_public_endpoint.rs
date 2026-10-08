use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    CertificateStatus, KubernetesCertificateMode, KubernetesExposureSettings,
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
use tokio::net::lookup_host;
use tracing::info;
use uuid::Uuid;

#[cfg(feature = "aws")]
use crate::core::aws_tag_scoped::{
    certificates_imported_with_token, delete_imported_certificate, with_import_token,
};
use crate::core::kubernetes_errors::is_remote_resource_conflict;
#[cfg(feature = "aws")]
use crate::core::split_certificate_chain;
use crate::core::{kubernetes_cleanup_resource_labels, ResourceControllerContext};
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
    /// Token tagged on the certificate a `managedAcmImport` import makes, recorded before the
    /// import so a retry after a lost response, and the delete, find that certificate.
    #[serde(default)]
    pub(crate) managed_acm_import_token: Option<String>,
    /// Region the managed ACM certificate is imported into, recorded with the import token.
    /// `managedAcmImport` may name a region other than the cluster's, and the lookup and
    /// delete must use it even after the endpoint switches to another certificate mode.
    #[serde(default)]
    pub(crate) managed_acm_region: Option<String>,
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
    pub(crate) wildcard_subdomains: bool,
    pub(crate) deployment_labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KubernetesEndpointAction {
    Ready,
    Waiting { suggested_delay: Duration },
}

#[derive(Debug, Clone)]
struct EndpointPlan {
    // Generated metadata already includes endpoint aliases and wildcard names.
    managed_hostnames: Option<Vec<String>>,
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
    mut target: KubernetesPublicEndpointTarget<'_>,
    state: &mut KubernetesPublicEndpointState,
) -> Result<KubernetesEndpointAction> {
    target.deployment_labels = kubernetes_cleanup_resource_labels(ctx, target.resource_id);
    let mut plan = match resolve_endpoint_plan(ctx, &target)? {
        EndpointPlanResolution::Disabled => {
            delete_kubernetes_public_endpoint(
                ctx,
                target.resource_id,
                target.namespace,
                target.workload_name,
                target.component,
                true,
                state,
            )
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
    // A first managed ACM import needs its token saved by a step of its own, before anything
    // else this reconcile changes: the executor saves state only between steps, so a token
    // made in the step that imports would be lost if the process stopped after ACM accepted
    // the import.
    if let EndpointCertificate::ManagedAcmImport { region, .. } = &plan.certificate {
        if is_aws_alb_ingress(&plan.route)
            && state.managed_acm_certificate_arn.is_none()
            && state.managed_acm_import_token.is_none()
        {
            state.managed_acm_region = Some(resolve_managed_acm_region(
                ctx,
                target.resource_id,
                region.as_ref(),
            )?);
            state.managed_acm_import_token = Some(Uuid::new_v4().to_string());
            return Ok(KubernetesEndpointAction::Waiting {
                suggested_delay: Duration::from_secs(1),
            });
        }
    }
    let previous_ingress_name = state.ingress_name.clone();
    let previous_gateway_name = state.gateway_name.clone();
    let previous_http_route_name = state.http_route_name.clone();
    let previous_gke_health_check_policy_name = state.gke_health_check_policy_name.clone();
    let previous_azure_health_check_policy_name = state.azure_health_check_policy_name.clone();
    let previous_managed_tls_secret_name = state.managed_tls_secret_name.clone();
    let previous_managed_tls_certificate_id = state.published_certificate_id.clone();
    let mut pending_state = state.clone();

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
    pending_state.service_name = Some(service_name.clone());

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
                endpoint_labels(&target, name),
                previous_managed_tls_secret_name.as_deref(),
                previous_managed_tls_certificate_id.as_deref(),
            )
            .await?;
            pending_state.managed_tls_secret_name = Some(name.clone());
            pending_state.published_certificate_id = Some(certificate_id.clone());
            pending_state.published_certificate_issued_at = issued_at.clone();
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
                &mut pending_state,
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
            // The import has a provider-assigned ARN, so it is kept even when a later step of
            // this reconcile fails and the rest of `pending_state` is dropped.
            state.managed_acm_certificate_arn = pending_state.managed_acm_certificate_arn.clone();
            state.managed_acm_region = pending_state.managed_acm_region.clone();
            state.published_certificate_id = pending_state.published_certificate_id.clone();
            state.published_certificate_issued_at =
                pending_state.published_certificate_issued_at.clone();
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
            pending_state.ingress_name = Some(ingress_name.clone());
            pending_state.gateway_name = None;
            pending_state.http_route_name = None;
            pending_state.gke_health_check_policy_name = None;
            pending_state.azure_health_check_policy_name = None;
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
            pending_state.gateway_name = Some(gateway_name.clone());
            pending_state.http_route_name = Some(route_name);
            pending_state.gke_health_check_policy_name = health_check_policy_name;
            pending_state.azure_health_check_policy_name = azure_health_check_policy_name;
            pending_state.ingress_name = None;
            observe_gateway_endpoint(&route_client, target.namespace, &gateway_name).await?
        }
    };

    let cleanup_result = cleanup_stale_endpoint_objects(
        ctx,
        target.namespace,
        target.resource_id,
        target.workload_name,
        target.component,
        &route_client,
        PreviousEndpointObjects {
            ingress_name: previous_ingress_name,
            gateway_name: previous_gateway_name,
            http_route_name: previous_http_route_name,
            gke_health_check_policy_name: previous_gke_health_check_policy_name,
            azure_health_check_policy_name: previous_azure_health_check_policy_name,
            managed_tls_secret_name: previous_managed_tls_secret_name,
            managed_tls_certificate_id: previous_managed_tls_certificate_id,
        },
        ActiveEndpointObjects {
            ingress_name: pending_state.ingress_name.clone(),
            gateway_name: pending_state.gateway_name.clone(),
            http_route_name: pending_state.http_route_name.clone(),
            gke_health_check_policy_name: pending_state.gke_health_check_policy_name.clone(),
            azure_health_check_policy_name: pending_state.azure_health_check_policy_name.clone(),
            managed_tls_secret_name: active_managed_tls_secret_name,
            managed_acm_certificate: active_managed_acm_certificate,
        },
        &mut pending_state,
    )
    .await;
    if let Err(error) = cleanup_result {
        // Route and Secret names remain at their previous values so the retry still knows
        // exactly which stale objects to remove. A managed ACM import was already kept above.
        return Err(error);
    }
    *state = pending_state;

    let Some(endpoint) = endpoint else {
        state.public_url = None;
        state.load_balancer_endpoint = None;
        return Ok(KubernetesEndpointAction::Waiting {
            suggested_delay: ENDPOINT_WAIT,
        });
    };

    if !load_balancer_endpoint_dns_resolves(&endpoint).await {
        state.public_url = None;
        state.load_balancer_endpoint = None;
        return Ok(KubernetesEndpointAction::Waiting {
            suggested_delay: ENDPOINT_WAIT,
        });
    }

    let public_url = plan
        .public_url
        .clone()
        .unwrap_or_else(|| load_balancer_endpoint_url(&endpoint));

    if let Some(health_check_path) = target.health_check_path.as_deref() {
        if !public_endpoint_health_check_succeeds(&public_url, health_check_path).await {
            state.public_url = None;
            state.load_balancer_endpoint = None;
            return Ok(KubernetesEndpointAction::Waiting {
                suggested_delay: ENDPOINT_WAIT,
            });
        }
    }

    state.public_url = Some(public_url);
    state.load_balancer_endpoint = Some(endpoint);

    Ok(KubernetesEndpointAction::Ready)
}

pub(crate) async fn delete_kubernetes_public_endpoint(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    namespace: &str,
    workload_name: &str,
    component: &str,
    legacy_owner_proven: bool,
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
    let desired_scope = kubernetes_cleanup_resource_labels(ctx, resource_id);

    if let Some(route_name) = state.http_route_name.clone() {
        match get_endpoint_object_or_none(
            route_client.get_http_route(namespace, &route_name).await,
            &route_name,
        )? {
            None => state.http_route_name = None,
            Some(object)
                if endpoint_json_has_delete_owner(&object, &desired_scope, &route_name)
                    || (legacy_owner_proven
                        && endpoint_json_has_legacy_delete_owner(
                            &object,
                            &route_name,
                            workload_name,
                            component,
                        )) =>
            {
                delete_not_found_ok(
                    route_client.delete_http_route(namespace, &route_name).await,
                    &route_name,
                )?;
                state.http_route_name = None;
            }
            Some(_) => {
                return Err(endpoint_delete_refusal(
                    "HTTPRoute",
                    &route_name,
                    resource_id,
                ))
            }
        }
    }
    if let Some(policy_name) = state.gke_health_check_policy_name.clone() {
        match get_endpoint_object_or_none(
            route_client
                .get_gke_health_check_policy(namespace, &policy_name)
                .await,
            &policy_name,
        )? {
            None => state.gke_health_check_policy_name = None,
            Some(object)
                if endpoint_json_has_delete_owner(&object, &desired_scope, &policy_name)
                    || (legacy_owner_proven
                        && endpoint_json_has_legacy_delete_owner(
                            &object,
                            &policy_name,
                            workload_name,
                            component,
                        )) =>
            {
                delete_not_found_ok(
                    route_client
                        .delete_gke_health_check_policy(namespace, &policy_name)
                        .await,
                    &policy_name,
                )?;
                state.gke_health_check_policy_name = None;
            }
            Some(_) => {
                return Err(endpoint_delete_refusal(
                    "GKE HealthCheckPolicy",
                    &policy_name,
                    resource_id,
                ));
            }
        }
    }
    if let Some(policy_name) = state.azure_health_check_policy_name.clone() {
        match get_endpoint_object_or_none(
            route_client
                .get_azure_health_check_policy(namespace, &policy_name)
                .await,
            &policy_name,
        )? {
            None => state.azure_health_check_policy_name = None,
            Some(object)
                if endpoint_json_has_delete_owner(&object, &desired_scope, &policy_name)
                    || (legacy_owner_proven
                        && endpoint_json_has_legacy_delete_owner(
                            &object,
                            &policy_name,
                            workload_name,
                            component,
                        )) =>
            {
                delete_not_found_ok(
                    route_client
                        .delete_azure_health_check_policy(namespace, &policy_name)
                        .await,
                    &policy_name,
                )?;
                state.azure_health_check_policy_name = None;
            }
            Some(_) => {
                return Err(endpoint_delete_refusal(
                    "Azure HealthCheckPolicy",
                    &policy_name,
                    resource_id,
                ));
            }
        }
    }
    if let Some(gateway_name) = state.gateway_name.clone() {
        match get_endpoint_object_or_none(
            route_client.get_gateway(namespace, &gateway_name).await,
            &gateway_name,
        )? {
            None => state.gateway_name = None,
            Some(object)
                if endpoint_json_has_delete_owner(&object, &desired_scope, &gateway_name)
                    || (legacy_owner_proven
                        && endpoint_json_has_legacy_delete_owner(
                            &object,
                            &gateway_name,
                            workload_name,
                            component,
                        )) =>
            {
                delete_not_found_ok(
                    route_client.delete_gateway(namespace, &gateway_name).await,
                    &gateway_name,
                )?;
                state.gateway_name = None;
            }
            Some(_) => {
                return Err(endpoint_delete_refusal(
                    "Gateway",
                    &gateway_name,
                    resource_id,
                ));
            }
        }
    }
    if let Some(ingress_name) = state.ingress_name.clone() {
        match get_endpoint_object_or_none(
            route_client.get_ingress(namespace, &ingress_name).await,
            &ingress_name,
        )? {
            None => state.ingress_name = None,
            Some(object)
                if endpoint_labels_match_delete_owner(
                    object.metadata.labels.as_ref(),
                    &desired_scope,
                    &ingress_name,
                ) || (legacy_owner_proven
                    && endpoint_labels_match_legacy_delete_owner(
                        object.metadata.labels.as_ref(),
                        &ingress_name,
                        workload_name,
                        component,
                    )) =>
            {
                delete_not_found_ok(
                    route_client.delete_ingress(namespace, &ingress_name).await,
                    &ingress_name,
                )?;
                state.ingress_name = None;
            }
            Some(_) => {
                return Err(endpoint_delete_refusal(
                    "Ingress",
                    &ingress_name,
                    resource_id,
                ));
            }
        }
    }
    if let Some(service_name) = state.service_name.clone() {
        match get_endpoint_object_or_none(
            service_client.get_service(namespace, &service_name).await,
            &service_name,
        )? {
            None => state.service_name = None,
            Some(object)
                if endpoint_labels_match_delete_owner(
                    object.metadata.labels.as_ref(),
                    &desired_scope,
                    &service_name,
                ) || (legacy_owner_proven
                    && endpoint_labels_match_legacy_delete_owner(
                        object.metadata.labels.as_ref(),
                        &service_name,
                        workload_name,
                        component,
                    )) =>
            {
                delete_not_found_ok(
                    service_client
                        .delete_service(namespace, &service_name)
                        .await,
                    &service_name,
                )?;
                state.service_name = None;
            }
            Some(_) => {
                return Err(endpoint_delete_refusal(
                    "Service",
                    &service_name,
                    resource_id,
                ));
            }
        }
    }
    if let Some(secret_name) = state.managed_tls_secret_name.clone() {
        match get_endpoint_object_or_none(
            secrets_client.get_secret(namespace, &secret_name).await,
            &secret_name,
        )? {
            None => state.managed_tls_secret_name = None,
            Some(object) => {
                let certificate_matches =
                    state
                        .published_certificate_id
                        .as_deref()
                        .is_some_and(|certificate_id| {
                            object
                                .metadata
                                .annotations
                                .as_ref()
                                .and_then(|annotations| annotations.get("certificate-id"))
                                .map(String::as_str)
                                == Some(certificate_id)
                        });
                let current_owner = endpoint_labels_match_delete_owner(
                    object.metadata.labels.as_ref(),
                    &desired_scope,
                    &secret_name,
                );
                let legacy_owner = object
                    .metadata
                    .labels
                    .as_ref()
                    .is_none_or(BTreeMap::is_empty);
                if object.type_.as_deref() != Some("kubernetes.io/tls")
                    || !certificate_matches
                    || !(current_owner || legacy_owner)
                {
                    return Err(endpoint_delete_refusal(
                        "TLS Secret",
                        &secret_name,
                        resource_id,
                    ));
                }
                delete_not_found_ok(
                    secrets_client.delete_secret(namespace, &secret_name).await,
                    &secret_name,
                )?;
                state.managed_tls_secret_name = None;
            }
        }
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
    managed_tls_certificate_id: Option<String>,
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
    workload_name: &str,
    component: &str,
    route_client: &std::sync::Arc<dyn alien_k8s_clients::RouteApi>,
    previous: PreviousEndpointObjects,
    active: ActiveEndpointObjects,
    state: &mut KubernetesPublicEndpointState,
) -> Result<()> {
    let desired_scope = kubernetes_cleanup_resource_labels(ctx, resource_id);
    if let Some(route_name) = previous.http_route_name {
        if Some(route_name.as_str()) != active.http_route_name.as_deref() {
            match get_endpoint_object_or_none(
                route_client.get_http_route(namespace, &route_name).await,
                &route_name,
            )? {
                None => {}
                Some(object)
                    if endpoint_json_has_delete_owner(&object, &desired_scope, &route_name)
                        || endpoint_json_has_legacy_delete_owner(
                            &object,
                            &route_name,
                            workload_name,
                            component,
                        ) =>
                {
                    delete_not_found_ok(
                        route_client.delete_http_route(namespace, &route_name).await,
                        &route_name,
                    )?;
                }
                Some(_) => {
                    return Err(endpoint_delete_refusal(
                        "HTTPRoute",
                        &route_name,
                        resource_id,
                    ))
                }
            }
        }
    }
    if let Some(policy_name) = previous.gke_health_check_policy_name {
        if Some(policy_name.as_str()) != active.gke_health_check_policy_name.as_deref() {
            match get_endpoint_object_or_none(
                route_client
                    .get_gke_health_check_policy(namespace, &policy_name)
                    .await,
                &policy_name,
            )? {
                None => {}
                Some(object)
                    if endpoint_json_has_delete_owner(&object, &desired_scope, &policy_name)
                        || endpoint_json_has_legacy_delete_owner(
                            &object,
                            &policy_name,
                            workload_name,
                            component,
                        ) =>
                {
                    delete_not_found_ok(
                        route_client
                            .delete_gke_health_check_policy(namespace, &policy_name)
                            .await,
                        &policy_name,
                    )?;
                }
                Some(_) => {
                    return Err(endpoint_delete_refusal(
                        "GKE HealthCheckPolicy",
                        &policy_name,
                        resource_id,
                    ))
                }
            }
        }
    }
    if let Some(policy_name) = previous.azure_health_check_policy_name {
        if Some(policy_name.as_str()) != active.azure_health_check_policy_name.as_deref() {
            match get_endpoint_object_or_none(
                route_client
                    .get_azure_health_check_policy(namespace, &policy_name)
                    .await,
                &policy_name,
            )? {
                None => {}
                Some(object)
                    if endpoint_json_has_delete_owner(&object, &desired_scope, &policy_name)
                        || endpoint_json_has_legacy_delete_owner(
                            &object,
                            &policy_name,
                            workload_name,
                            component,
                        ) =>
                {
                    delete_not_found_ok(
                        route_client
                            .delete_azure_health_check_policy(namespace, &policy_name)
                            .await,
                        &policy_name,
                    )?;
                }
                Some(_) => {
                    return Err(endpoint_delete_refusal(
                        "Azure HealthCheckPolicy",
                        &policy_name,
                        resource_id,
                    ))
                }
            }
        }
    }
    if let Some(gateway_name) = previous.gateway_name {
        if Some(gateway_name.as_str()) != active.gateway_name.as_deref() {
            match get_endpoint_object_or_none(
                route_client.get_gateway(namespace, &gateway_name).await,
                &gateway_name,
            )? {
                None => {}
                Some(object)
                    if endpoint_json_has_delete_owner(&object, &desired_scope, &gateway_name)
                        || endpoint_json_has_legacy_delete_owner(
                            &object,
                            &gateway_name,
                            workload_name,
                            component,
                        ) =>
                {
                    delete_not_found_ok(
                        route_client.delete_gateway(namespace, &gateway_name).await,
                        &gateway_name,
                    )?;
                }
                Some(_) => {
                    return Err(endpoint_delete_refusal(
                        "Gateway",
                        &gateway_name,
                        resource_id,
                    ))
                }
            }
        }
    }
    if let Some(ingress_name) = previous.ingress_name {
        if Some(ingress_name.as_str()) != active.ingress_name.as_deref() {
            match get_endpoint_object_or_none(
                route_client.get_ingress(namespace, &ingress_name).await,
                &ingress_name,
            )? {
                None => {}
                Some(object)
                    if endpoint_labels_match_delete_owner(
                        object.metadata.labels.as_ref(),
                        &desired_scope,
                        &ingress_name,
                    ) || endpoint_labels_match_legacy_delete_owner(
                        object.metadata.labels.as_ref(),
                        &ingress_name,
                        workload_name,
                        component,
                    ) =>
                {
                    delete_not_found_ok(
                        route_client.delete_ingress(namespace, &ingress_name).await,
                        &ingress_name,
                    )?;
                }
                Some(_) => {
                    return Err(endpoint_delete_refusal(
                        "Ingress",
                        &ingress_name,
                        resource_id,
                    ))
                }
            }
        }
    }

    if let Some(secret_name) = previous.managed_tls_secret_name {
        if Some(secret_name.as_str()) != active.managed_tls_secret_name.as_deref() {
            let kubernetes_config = ctx.get_kubernetes_config()?;
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            match get_endpoint_object_or_none(
                secrets_client.get_secret(namespace, &secret_name).await,
                &secret_name,
            )? {
                None => {}
                Some(object) => {
                    let certificate_matches = previous
                        .managed_tls_certificate_id
                        .as_deref()
                        .is_some_and(|certificate_id| {
                            object
                                .metadata
                                .annotations
                                .as_ref()
                                .and_then(|annotations| annotations.get("certificate-id"))
                                .map(String::as_str)
                                == Some(certificate_id)
                        });
                    let current_owner = endpoint_labels_match_delete_owner(
                        object.metadata.labels.as_ref(),
                        &desired_scope,
                        &secret_name,
                    );
                    let legacy_owner = object
                        .metadata
                        .labels
                        .as_ref()
                        .is_none_or(BTreeMap::is_empty);
                    if object.type_.as_deref() != Some("kubernetes.io/tls")
                        || !certificate_matches
                        || !(current_owner || legacy_owner)
                    {
                        return Err(endpoint_delete_refusal(
                            "TLS Secret",
                            &secret_name,
                            resource_id,
                        ));
                    }
                    delete_not_found_ok(
                        secrets_client.delete_secret(namespace, &secret_name).await,
                        &secret_name,
                    )?;
                }
            }
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

    // A recorded certificate (or import token) stays in the region it was imported into.
    let configured_region =
        resolve_managed_acm_region(ctx, target.resource_id, input.region.as_ref())?;
    let region = match recorded_managed_acm_region(state) {
        Some(recorded) if recorded != configured_region => {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: target.resource_id.to_string(),
                message: format!(
                    "The managed ACM certificate was imported into '{recorded}', but managedAcmImport now names '{configured_region}'. Moving an imported certificate to another region is not supported; switch to another certificate mode first, then back"
                ),
            }));
        }
        Some(recorded) => recorded,
        None => configured_region,
    };
    let mut aws_config = ctx.get_aws_config()?.clone();
    aws_config.region = region.clone();

    let acm_client = ctx.service_provider.get_aws_acm_client(&aws_config).await?;
    let tags = acm_tags(ctx.resource_prefix, target.resource_id, input.tags);
    let (leaf, chain) = split_certificate_chain(&input.certificate_chain);

    let certificate_arn = if let Some(certificate_arn) = state.managed_acm_certificate_arn.clone() {
        // ACM rejects tags on a reimport; the certificate keeps the ones it was imported with.
        acm_client
            .reimport_certificate(
                alien_aws_clients::acm::ReimportCertificateRequest::builder()
                    .certificate_arn(certificate_arn.clone())
                    .certificate(leaf)
                    .private_key(input.private_key)
                    .maybe_certificate_chain(chain)
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
        let token = state.managed_acm_import_token.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: target.resource_id.to_string(),
                message: "A managed ACM certificate import has no recorded import token"
                    .to_string(),
            })
        })?;
        // Every ImportCertificate without an ARN makes a new certificate, so an earlier import
        // under this token whose response was lost is looked up first.
        let earlier_import = certificates_imported_with_token(acm_client.as_ref(), &token)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to look up certificates imported by an earlier attempt"
                    .to_string(),
                resource_id: Some(target.resource_id.to_string()),
            })?
            .and_then(|found| found.into_iter().next());
        match earlier_import {
            Some(certificate_arn) => {
                info!(certificate_arn=%certificate_arn, "Adopting the Kubernetes public endpoint certificate an earlier import made");
                certificate_arn
            }
            None => {
                acm_client
                    .import_certificate(
                        alien_aws_clients::acm::ImportCertificateRequest::builder()
                            .certificate(leaf)
                            .private_key(input.private_key)
                            .maybe_certificate_chain(chain)
                            .tags(with_import_token(tags, &token))
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to import Kubernetes public endpoint certificate to ACM"
                            .to_string(),
                        resource_id: Some(target.resource_id.to_string()),
                    })?
                    .certificate_arn
            }
        }
    };

    state.managed_acm_certificate_arn = Some(certificate_arn.clone());
    state.managed_acm_region = Some(region);
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
    if state.managed_acm_certificate_arn.is_none() && state.managed_acm_import_token.is_none() {
        return Ok(());
    }

    // The certificate lives where it was imported, whatever the endpoint's mode is now.
    let mut aws_config = ctx.get_aws_config()?.clone();
    if let Some(region) = recorded_managed_acm_region(state) {
        aws_config.region = region;
    }
    let acm_client = ctx.service_provider.get_aws_acm_client(&aws_config).await?;
    let mut certificate_arns: Vec<String> =
        state.managed_acm_certificate_arn.iter().cloned().collect();
    // An import whose response was lost left a certificate that only its token finds.
    if let Some(token) = state.managed_acm_import_token.as_deref() {
        let found = certificates_imported_with_token(acm_client.as_ref(), token)
            .await
            .context(ErrorData::CloudPlatformError {
                message:
                    "Failed to look up Kubernetes public endpoint certificates imported to ACM"
                        .to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;
        for certificate_arn in found.into_iter().flatten() {
            if !certificate_arns.contains(&certificate_arn) {
                certificate_arns.push(certificate_arn);
            }
        }
    }
    for certificate_arn in &certificate_arns {
        // The delete and read are granted only on certificates carrying this resource's tags,
        // so ACM answers both with AccessDenied for a certificate that is already gone.
        let outcome = delete_imported_certificate(acm_client.as_ref(), certificate_arn)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to delete Kubernetes public endpoint ACM certificate '{certificate_arn}'"
                ),
                resource_id: Some(resource_id.to_string()),
            })?;
        info!(certificate_arn=%certificate_arn, outcome=?outcome, "Kubernetes public endpoint ACM certificate removed");
    }
    state.managed_acm_certificate_arn = None;
    state.managed_acm_import_token = None;
    state.managed_acm_region = None;
    Ok(())
}

/// The region `managedAcmImport` imports into: the configured one, or the cluster's.
#[cfg(feature = "aws")]
fn resolve_managed_acm_region(
    ctx: &ResourceControllerContext<'_>,
    _resource_id: &str,
    configured: Option<&String>,
) -> Result<String> {
    match configured {
        Some(region) => Ok(region.clone()),
        None => Ok(ctx.get_aws_config()?.region.clone()),
    }
}

#[cfg(not(feature = "aws"))]
fn resolve_managed_acm_region(
    _ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    _configured: Option<&String>,
) -> Result<String> {
    Err(AlienError::new(ErrorData::ResourceControllerConfigError {
        resource_id: resource_id.to_string(),
        message: "managedAcmImport certificate mode requires the aws feature".to_string(),
    }))
}

/// The region of the recorded managed ACM certificate: the one saved with the import, else the
/// one in the recorded ARN (state saved before the region was recorded).
#[cfg(feature = "aws")]
fn recorded_managed_acm_region(state: &KubernetesPublicEndpointState) -> Option<String> {
    state.managed_acm_region.clone().or_else(|| {
        state
            .managed_acm_certificate_arn
            .as_deref()
            .and_then(|arn| arn.split(':').nth(3))
            .filter(|region| !region.is_empty())
            .map(str::to_string)
    })
}

#[cfg(not(feature = "aws"))]
async fn delete_managed_acm_certificate(
    _ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    state: &mut KubernetesPublicEndpointState,
) -> Result<()> {
    if state.managed_acm_certificate_arn.is_none() && state.managed_acm_import_token.is_none() {
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
    public: bool,
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
        public,
        wildcard_subdomains: false,
        deployment_labels: BTreeMap::new(),
    }
}

/// Same shape as the container target: a Daemon exposes at most one HTTP
/// endpoint, and the Service selects the DaemonSet's pods on every node.
pub(crate) fn daemon_public_endpoint_target<'a>(
    resource_id: &'a str,
    workload_name: &'a str,
    namespace: &'a str,
    selector: BTreeMap<String, String>,
    public_endpoints: &'a [alien_core::PublicEndpoint],
    health_check_path: Option<&str>,
) -> Result<KubernetesPublicEndpointTarget<'a>> {
    let http_endpoint = public_endpoints
        .iter()
        .find(|endpoint| endpoint.protocol == alien_core::ExposeProtocol::Http);
    let http_port = http_endpoint.map(|endpoint| endpoint.port);

    Ok(KubernetesPublicEndpointTarget {
        resource_id,
        workload_name,
        namespace,
        component: "daemon",
        selector,
        service_port: http_port.unwrap_or(80),
        target_port: http_port.unwrap_or(80),
        health_check_path: health_check_path.map(ToString::to_string),
        public: http_port.is_some(),
        wildcard_subdomains: public_endpoints.iter().any(|endpoint| {
            endpoint.protocol == alien_core::ExposeProtocol::Http
                && Some(endpoint.port) == http_port
                && endpoint.wildcard_subdomains
        }),
        deployment_labels: BTreeMap::new(),
    })
}

pub(crate) fn container_public_endpoint_target<'a>(
    resource_id: &'a str,
    workload_name: &'a str,
    namespace: &'a str,
    selector: BTreeMap<String, String>,
    public_endpoints: &'a [alien_core::PublicEndpoint],
    health_check_path: Option<&str>,
) -> Result<KubernetesPublicEndpointTarget<'a>> {
    let http_endpoint = public_endpoints
        .iter()
        .find(|endpoint| endpoint.protocol == alien_core::ExposeProtocol::Http);
    let http_port = http_endpoint.map(|endpoint| endpoint.port);

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
        wildcard_subdomains: public_endpoints.iter().any(|endpoint| {
            endpoint.protocol == alien_core::ExposeProtocol::Http
                && Some(endpoint.port) == http_port
                && endpoint.wildcard_subdomains
        }),
        deployment_labels: BTreeMap::new(),
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
                        managed_hostnames: None,
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
                    managed_hostnames: Some(
                        std::iter::once(domain.fqdn.clone())
                            .chain(domain.aliases.iter().map(|alias| alias.fqdn.clone()))
                            .collect(),
                    ),
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

            if matches!(
                certificate,
                EndpointCertificate::ManagedTlsSecret { .. }
                    | EndpointCertificate::ManagedAcmImport { .. }
            ) {
                validate_managed_alias_certificates(target.resource_id, domain)?;
            }

            let public_url = if matches!(certificate, EndpointCertificate::None) {
                format!("http://{}", domain.fqdn)
            } else {
                format!("https://{}", domain.fqdn)
            };

            Ok(EndpointPlanResolution::Ready(EndpointPlan {
                managed_hostnames: Some(
                    std::iter::once(domain.fqdn.clone())
                        .chain(domain.aliases.iter().map(|alias| alias.fqdn.clone()))
                        .collect(),
                ),
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
                managed_hostnames: None,
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

/// The runtime publishes one managed certificate per resource. Reject aliases
/// that need separate certificate material before creating any Kubernetes objects.
fn validate_managed_alias_certificates(
    resource_id: &str,
    domain: &alien_core::ResourceDomainInfo,
) -> Result<()> {
    if let Some(alias) = domain
        .aliases
        .iter()
        .find(|alias| alias.certificate_id != domain.certificate_id)
    {
        return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: resource_id.to_string(),
            message: format!("Generated Kubernetes alias '{}' requires a separate certificate; all aliases must share the primary managed certificate", alias.fqdn),
        }));
    }
    Ok(())
}

/// Keep routing and TLS names aligned. A wildcard does not include its base host.
fn endpoint_hostnames(
    target: &KubernetesPublicEndpointTarget<'_>,
    plan: &EndpointPlan,
) -> Vec<String> {
    if let Some(hostnames) = &plan.managed_hostnames {
        return hostnames.clone();
    }
    let Some(hostname) = &plan.hostname else {
        return Vec::new();
    };
    let mut hostnames = vec![hostname.clone()];
    if target.wildcard_subdomains && !hostname.starts_with("*.") {
        hostnames.push(format!("*.{hostname}"));
    }
    hostnames
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

    let hostnames = endpoint_hostnames(target, plan);
    // A hostname-free endpoint retains its catch-all rule.
    let rule_hosts = if hostnames.is_empty() {
        vec![None]
    } else {
        hostnames.iter().cloned().map(Some).collect()
    };

    Ok(K8sIngress {
        metadata,
        spec: Some(IngressSpec {
            ingress_class_name: Some(profile.ingress_class_name.clone()),
            rules: Some(
                rule_hosts
                    .into_iter()
                    .map(|host| IngressRule {
                        host,
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
                    })
                    .collect(),
            ),
            tls: tls_ref.filter(|_| !hostnames.is_empty()).map(|secret| {
                vec![IngressTLS {
                    hosts: Some(hostnames),
                    secret_name: Some(secret.secret_name.clone()),
                }]
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

    let mut listeners = vec![listener];
    for (index, hostname) in endpoint_hostnames(target, plan)
        .into_iter()
        .enumerate()
        .skip(1)
    {
        let mut additional_listener = listeners[0].clone();
        additional_listener["name"] = json!(format!(
            "{}-{index}",
            if uses_tls { "https" } else { "http" }
        ));
        additional_listener["hostname"] = json!(hostname);
        listeners.push(additional_listener);
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
            "listeners": listeners,
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
    let hostnames = endpoint_hostnames(target, plan);
    if !hostnames.is_empty() {
        route["spec"]["hostnames"] = json!(hostnames);
    }
    route
}

fn is_gke_gateway_route(route: &KubernetesRouteProfile) -> bool {
    matches!(
        route,
        KubernetesRouteProfile::Gateway(KubernetesGatewayRouteProfile {
            provider: Some(KubernetesRouteProviderOptions::GkeGateway { .. }),
            ..
        })
    )
}

fn gke_health_check_policy_name(
    target: &KubernetesPublicEndpointTarget<'_>,
    plan: &EndpointPlan,
    service_name: &str,
) -> Option<String> {
    if is_gke_gateway_route(&plan.route) && target.health_check_path.is_some() {
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
        Err(e) if is_remote_resource_conflict(&e) => {
            let existing = client.get_service(namespace, name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get Service '{}' before update", name),
                    resource_id: Some(resource_id.to_string()),
                },
            )?;
            ensure_endpoint_labels_allow_adoption(
                existing.metadata.labels.as_ref(),
                service.metadata.labels.as_ref(),
                "Service",
                name,
                resource_id,
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
    labels: BTreeMap<String, String>,
    previous_managed_name: Option<&str>,
    previous_certificate_id: Option<&str>,
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
            labels: Some(labels),
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
        Err(e) if is_remote_resource_conflict(&e) => {
            let existing = client.get_secret(namespace, name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get TLS Secret '{}' before update", name),
                    resource_id: Some(resource_id.to_string()),
                },
            )?;
            let scoped_owner = ensure_endpoint_labels_allow_adoption(
                existing.metadata.labels.as_ref(),
                secret.metadata.labels.as_ref(),
                "Secret",
                name,
                resource_id,
            );
            let existing_certificate_id = existing
                .metadata
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.get("certificate-id"))
                .map(String::as_str);
            let legacy_owner_proven = existing
                .metadata
                .labels
                .as_ref()
                .is_none_or(BTreeMap::is_empty)
                && previous_managed_name == Some(name)
                && previous_certificate_id.is_some()
                && existing_certificate_id == previous_certificate_id;
            if existing.type_.as_deref() != Some("kubernetes.io/tls") {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Refusing to mutate Kubernetes TLS Secret '{name}' because it is not a TLS Secret owned by public endpoint resource '{resource_id}'"
                    ),
                    resource_id: Some(resource_id.to_string()),
                }));
            }
            if !legacy_owner_proven {
                scoped_owner?;
            }
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
        Err(e) if is_remote_resource_conflict(&e) => {
            let existing = client.get_ingress(namespace, name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get Ingress '{}' before update", name),
                    resource_id: Some(resource_id.to_string()),
                },
            )?;
            ensure_endpoint_labels_allow_adoption(
                existing.metadata.labels.as_ref(),
                ingress.metadata.labels.as_ref(),
                "Ingress",
                name,
                resource_id,
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
        Err(e) if is_remote_resource_conflict(&e) => {
            let existing = client.get_gateway(namespace, name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get Gateway '{}' before update", name),
                    resource_id: Some(resource_id.to_string()),
                },
            )?;
            ensure_endpoint_json_labels_allow_adoption(
                &existing,
                &gateway,
                "Gateway",
                name,
                resource_id,
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
        Err(e) if is_remote_resource_conflict(&e) => {
            let existing = client.get_http_route(namespace, name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get HTTPRoute '{}' before update", name),
                    resource_id: Some(resource_id.to_string()),
                },
            )?;
            ensure_endpoint_json_labels_allow_adoption(
                &existing,
                &route,
                "HTTPRoute",
                name,
                resource_id,
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
        Err(e) if is_remote_resource_conflict(&e) => {
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
            ensure_endpoint_json_labels_allow_adoption(
                &existing,
                &policy,
                "GKE HealthCheckPolicy",
                name,
                resource_id,
            )?;
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
        Err(e) if is_remote_resource_conflict(&e) => {
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
            ensure_endpoint_json_labels_allow_adoption(
                &existing,
                &policy,
                "Azure HealthCheckPolicy",
                name,
                resource_id,
            )?;
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

async fn load_balancer_endpoint_dns_resolves(endpoint: &LoadBalancerEndpoint) -> bool {
    let (host, port) = dns_lookup_target(&endpoint.dns_name);
    let lookup_result = lookup_host((host.as_str(), port)).await;
    match lookup_result {
        Ok(addrs) => {
            let addrs = addrs.collect::<Vec<_>>();
            if !addrs.is_empty() {
                return true;
            }
            info!(
                dns_name = %endpoint.dns_name,
                host = %host,
                "Kubernetes public endpoint DNS resolved no addresses yet"
            );
            false
        }
        Err(err) => {
            info!(
                dns_name = %endpoint.dns_name,
                host = %host,
                error = %err,
                "Kubernetes public endpoint DNS is not ready yet"
            );
            false
        }
    }
}

async fn public_endpoint_health_check_succeeds(public_url: &str, health_check_path: &str) -> bool {
    let health_check_url = endpoint_health_check_url(public_url, health_check_path);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            info!(
                url = %health_check_url,
                error = %err,
                "Kubernetes public endpoint health check client could not be built"
            );
            return false;
        }
    };

    match client.get(&health_check_url).send().await {
        Ok(response) if response.status().is_success() => true,
        Ok(response) => {
            info!(
                url = %health_check_url,
                status = %response.status(),
                "Kubernetes public endpoint health check is not ready yet"
            );
            false
        }
        Err(err) => {
            info!(
                url = %health_check_url,
                error = %err,
                "Kubernetes public endpoint health check request is not ready yet"
            );
            false
        }
    }
}

fn endpoint_health_check_url(public_url: &str, health_check_path: &str) -> String {
    format!(
        "{}{}",
        public_url.trim_end_matches('/'),
        health_check_path
            .strip_prefix('/')
            .map(|path| format!("/{path}"))
            .unwrap_or_else(|| format!("/{health_check_path}"))
    )
}

fn dns_lookup_target(endpoint: &str) -> (String, u16) {
    let endpoint = endpoint.trim();
    let (without_scheme, default_port) = if let Some(value) = endpoint.strip_prefix("https://") {
        (value, 443)
    } else if let Some(value) = endpoint.strip_prefix("http://") {
        (value, 80)
    } else {
        (endpoint, 80)
    };
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
    if let Some((host, port)) = host_port.rsplit_once(':') {
        if let Ok(port) = port.parse::<u16>() {
            return (host.trim_end_matches('.').to_string(), port);
        }
    }
    (host_port.trim_end_matches('.').to_string(), default_port)
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

fn get_endpoint_object_or_none<T>(
    result: alien_client_core::Result<T>,
    name: &str,
) -> Result<Option<T>> {
    match result {
        Ok(object) => Ok(Some(object)),
        Err(error)
            if matches!(
                error.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to inspect Kubernetes public endpoint object '{name}' before deletion"
            ),
            resource_id: None,
        })),
    }
}

fn endpoint_json_has_delete_owner(
    object: &Value,
    desired_scope: &BTreeMap<String, String>,
    name: &str,
) -> bool {
    let labels = json_labels(object);
    endpoint_labels_match_delete_owner(labels.as_ref(), desired_scope, name)
}

fn endpoint_json_has_legacy_delete_owner(
    object: &Value,
    name: &str,
    workload_name: &str,
    component: &str,
) -> bool {
    let labels = json_labels(object);
    endpoint_labels_match_legacy_delete_owner(labels.as_ref(), name, workload_name, component)
}

fn endpoint_labels_match_delete_owner(
    existing: Option<&BTreeMap<String, String>>,
    desired_scope: &BTreeMap<String, String>,
    name: &str,
) -> bool {
    existing.is_some_and(|labels| {
        labels.get("managed-by").map(String::as_str) == Some("runtime")
            && labels.get("endpoint").map(String::as_str) == Some(name)
            && desired_scope
                .iter()
                .all(|(key, value)| labels.get(key) == Some(value))
    })
}

fn endpoint_labels_match_legacy_delete_owner(
    existing: Option<&BTreeMap<String, String>>,
    name: &str,
    workload_name: &str,
    component: &str,
) -> bool {
    !workload_name.is_empty()
        && existing.is_some_and(|labels| {
            !labels.keys().any(|key| key.ends_with("/deployment"))
                && labels.get("managed-by").map(String::as_str) == Some("runtime")
                && labels.get("endpoint").map(String::as_str) == Some(name)
                && labels.get("app").map(String::as_str) == Some(workload_name)
                && labels.get("component").map(String::as_str) == Some(component)
        })
}

fn endpoint_delete_refusal(kind: &str, name: &str, resource_id: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: format!(
            "Refusing to delete Kubernetes {kind} '{name}' because it is not owned by public endpoint resource '{resource_id}' in this deployment"
        ),
        resource_id: Some(resource_id.to_string()),
    })
}

fn endpoint_labels(
    target: &KubernetesPublicEndpointTarget<'_>,
    name: &str,
) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::from([
        ("app".to_string(), target.workload_name.to_string()),
        ("component".to_string(), target.component.to_string()),
        ("managed-by".to_string(), "runtime".to_string()),
        ("endpoint".to_string(), name.to_string()),
    ]);
    labels.extend(target.deployment_labels.clone());
    labels
}

fn ensure_endpoint_labels_allow_adoption(
    existing: Option<&BTreeMap<String, String>>,
    desired: Option<&BTreeMap<String, String>>,
    kind: &str,
    name: &str,
    resource_id: &str,
) -> Result<()> {
    let desired = desired.cloned().unwrap_or_default();
    if crate::core::kubernetes_labels_match_identity(
        existing,
        &desired,
        &["managed-by", "component", "app", "endpoint"],
    ) && crate::core::kubernetes_labels_have_compatible_scope(existing, &desired)
    {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::ResourceConfigInvalid {
        message: format!(
            "Refusing to mutate Kubernetes {kind} '{name}' because it is not owned by public endpoint resource '{resource_id}' in this deployment"
        ),
        resource_id: Some(resource_id.to_string()),
    }))
}

fn json_labels(value: &Value) -> Option<BTreeMap<String, String>> {
    value
        .pointer("/metadata/labels")
        .cloned()
        .and_then(|labels| serde_json::from_value(labels).ok())
}

fn ensure_endpoint_json_labels_allow_adoption(
    existing: &Value,
    desired: &Value,
    kind: &str,
    name: &str,
    resource_id: &str,
) -> Result<()> {
    let existing_labels = json_labels(existing);
    let desired_labels = json_labels(desired);
    ensure_endpoint_labels_allow_adoption(
        existing_labels.as_ref(),
        desired_labels.as_ref(),
        kind,
        name,
        resource_id,
    )
}

fn merge_labels(
    base: BTreeMap<String, String>,
    extra: &HashMap<String, String>,
) -> BTreeMap<String, String> {
    let mut labels = btree_from_hash(extra);
    labels.extend(base);
    labels
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

fn aws_hosted_zone_id(profile: &KubernetesIngressRouteProfile) -> Option<String> {
    match &profile.provider {
        Some(KubernetesRouteProviderOptions::AwsAlb { .. }) => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ExposeProtocol, PublicEndpoint};
    use httpmock::prelude::*;

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
            wildcard_subdomains: false,
            deployment_labels: BTreeMap::from([(
                "alien.dev/deployment".to_string(),
                "test-release".to_string(),
            )]),
        }
    }

    #[test]
    fn container_target_is_public_only_for_http_exposed_port() {
        let public_endpoints = [PublicEndpoint {
            name: "default".to_string(),
            port: 8080,
            protocol: ExposeProtocol::Http,
            host_label: None,
            wildcard_subdomains: false,
        }];
        let target = container_public_endpoint_target(
            "api",
            "api",
            "default",
            BTreeMap::new(),
            &public_endpoints,
            None,
        )
        .expect("target");

        assert!(target.public);
        assert_eq!(target.service_port, 8080);
        assert_eq!(target.target_port, 8080);
    }

    #[test]
    fn managed_alias_with_separate_certificate_fails_before_route_creation() {
        let mut domain: alien_core::ResourceDomainInfo = serde_json::from_value(json!({
            "fqdn": "api.example.com", "certificateId": "primary-cert",
            "certificateStatus": "issued", "dnsStatus": "active",
            "aliases": [{"fqdn": "*.api.example.com", "certificateId": "primary-cert",
                "certificateStatus": "issued", "dnsStatus": "active"}]
        }))
        .expect("domain metadata");
        validate_managed_alias_certificates("api", &domain).expect("shared certificate");
        domain.aliases[0].certificate_id = "another-cert".to_string();
        let error = validate_managed_alias_certificates("api", &domain)
            .expect_err("separate certificate must not be served with primary material");
        assert!(error.message.contains("requires a separate certificate"));
        assert!(error.message.contains("*.api.example.com"));
    }

    #[test]
    fn wildcard_hosts_preserve_custom_wildcards_and_managed_aliases() {
        let endpoints = [
            PublicEndpoint {
                name: "web".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: Some("web".to_string()),
                wildcard_subdomains: false,
            },
            PublicEndpoint {
                name: "apps".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: Some("apps".to_string()),
                wildcard_subdomains: true,
            },
        ];
        let target = container_public_endpoint_target(
            "api",
            "api",
            "app",
            BTreeMap::new(),
            &endpoints,
            None,
        )
        .expect("target");
        let daemon =
            daemon_public_endpoint_target("api", "api", "app", BTreeMap::new(), &endpoints, None)
                .expect("daemon");
        assert!(target.wildcard_subdomains);
        assert!(daemon.wildcard_subdomains);
        let mut plan = EndpointPlan {
            managed_hostnames: None,
            hostname: Some("*.example.com".to_string()),
            public_url: None,
            route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile::default()),
            certificate: EndpointCertificate::None,
        };
        assert_eq!(endpoint_hostnames(&target, &plan), vec!["*.example.com"]);
        plan.hostname = Some("web.example.com".to_string());
        let managed = vec![
            "web.example.com".to_string(),
            "apps.example.com".to_string(),
            "*.apps.example.com".to_string(),
        ];
        plan.managed_hostnames = Some(managed.clone());
        assert_eq!(endpoint_hostnames(&target, &plan), managed);
        let gateway_profile = KubernetesGatewayRouteProfile {
            gateway_class_name: "shared-gateway".to_string(),
            listener_port: 80,
            ..Default::default()
        };
        let gateway =
            build_gateway(&target, &plan, &gateway_profile, "api-gateway", None).expect("gateway");
        let listeners = gateway["spec"]["listeners"].as_array().expect("listeners");
        assert_eq!(
            listeners
                .iter()
                .map(|listener| listener["hostname"].as_str().expect("hostname"))
                .collect::<Vec<_>>(),
            managed
        );
        assert_eq!(
            listeners
                .iter()
                .map(|listener| listener["name"].as_str().expect("name"))
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            3
        );
        assert_eq!(
            build_http_route(&target, &plan, "api-public", "api-gateway", "api-route")["spec"]
                ["hostnames"],
            json!(managed)
        );
    }

    #[test]
    fn wildcard_endpoint_keeps_ingress_tls_and_gateway_routes_aligned() {
        for wildcard_subdomains in [false, true] {
            let endpoints = [PublicEndpoint {
                name: "web".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains,
            }];
            let target = container_public_endpoint_target(
                "api",
                "api-v1",
                "app",
                BTreeMap::new(),
                &endpoints,
                None,
            )
            .expect("container target");
            let daemon_target = daemon_public_endpoint_target(
                "api",
                "api-v1",
                "app",
                BTreeMap::new(),
                &endpoints,
                None,
            )
            .expect("daemon target");
            assert_eq!(daemon_target.wildcard_subdomains, wildcard_subdomains);
            let mut hosts = vec!["api.example.com".to_string()];
            if wildcard_subdomains {
                hosts.push("*.api.example.com".to_string());
            }
            let secret = KubernetesTlsSecretRef {
                secret_name: "api-tls".to_string(),
                namespace: None,
            };
            let ingress_profile = KubernetesIngressRouteProfile {
                ingress_class_name: "nginx".to_string(),
                ..Default::default()
            };
            let plan = EndpointPlan {
                managed_hostnames: None,
                hostname: Some("api.example.com".to_string()),
                public_url: Some("https://api.example.com".to_string()),
                route: KubernetesRouteProfile::Ingress(ingress_profile.clone()),
                certificate: EndpointCertificate::TlsSecretRef(secret.clone()),
            };
            let ingress = build_ingress(
                &target,
                &plan,
                &ingress_profile,
                "api-public",
                "api-ingress",
                Some(&secret),
            )
            .expect("ingress");
            let spec = ingress.spec.expect("spec");
            let rules = spec.rules.expect("rules");
            assert_eq!(
                rules
                    .iter()
                    .map(|rule| rule.host.clone().expect("host"))
                    .collect::<Vec<_>>(),
                hosts
            );
            for rule in rules {
                let paths = rule.http.expect("http").paths;
                assert_eq!(paths.len(), 1);
                let backend = paths[0].backend.service.as_ref().expect("service");
                assert_eq!(backend.name, "api-public");
                assert_eq!(backend.port.as_ref().expect("port").number, Some(8080));
            }
            let tls = spec.tls.expect("TLS");
            assert_eq!(tls.len(), 1);
            assert_eq!(tls[0].hosts.as_ref(), Some(&hosts));
            assert_eq!(tls[0].secret_name.as_deref(), Some("api-tls"));

            let gateway_profile = KubernetesGatewayRouteProfile {
                gateway_class_name: "shared-gateway".to_string(),
                listener_port: 443,
                ..Default::default()
            };
            let gateway = build_gateway(
                &target,
                &plan,
                &gateway_profile,
                "api-gateway",
                Some(&secret),
            )
            .expect("gateway");
            let listeners = gateway["spec"]["listeners"].as_array().expect("listeners");
            assert_eq!(listeners.len(), hosts.len());
            for (listener, host) in listeners.iter().zip(&hosts) {
                assert_eq!(listener["hostname"], json!(host));
                assert_eq!(listener["protocol"], "HTTPS");
                assert_eq!(listener["port"], 443);
                assert_eq!(listener["tls"]["certificateRefs"][0]["name"], "api-tls");
            }
            if wildcard_subdomains {
                assert_ne!(listeners[0]["name"], listeners[1]["name"]);
            }
            let route = build_http_route(&target, &plan, "api-public", "api-gateway", "api-route");
            assert_eq!(route["spec"]["hostnames"], json!(hosts));
            assert_eq!(
                route["spec"]["rules"][0]["backendRefs"][0]["name"],
                "api-public"
            );
        }
    }

    #[test]
    fn ingress_with_byo_acm_arn_sets_alb_certificate_annotation() {
        let target = endpoint_target();
        let mut profile_labels = HashMap::new();
        profile_labels.insert(
            "alien.dev/deployment".to_string(),
            "attacker-selected".to_string(),
        );
        profile_labels.insert("custom".to_string(), "kept".to_string());
        let plan = EndpointPlan {
            managed_hostnames: None,
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
                labels: profile_labels,
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
        let labels = ingress.metadata.labels.expect("ingress labels");
        assert_eq!(
            labels.get("alien.dev/deployment").map(String::as_str),
            Some("test-release"),
            "custom route labels must not override cleanup ownership"
        );
        assert_eq!(labels.get("custom").map(String::as_str), Some("kept"));
    }

    #[test]
    fn aws_alb_ingress_uses_declared_health_check_path() {
        let mut target = endpoint_target();
        target.health_check_path = Some("/ready".to_string());
        let plan = EndpointPlan {
            managed_hostnames: None,
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
            managed_hostnames: None,
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
            managed_hostnames: None,
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
            managed_hostnames: None,
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
            managed_hostnames: None,
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
            managed_hostnames: None,
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
            .expect("ALB provider annotations");
        assert_eq!(
            annotations.get("alb.ingress.kubernetes.io/target-type"),
            Some(&"ip".to_string())
        );
        assert!(!annotations.contains_key("alb.ingress.kubernetes.io/healthcheck-path"));
        assert!(!annotations.contains_key("alb.ingress.kubernetes.io/success-codes"));
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
    fn dns_lookup_target_handles_endpoint_url_forms() {
        assert_eq!(
            dns_lookup_target("k8s-api.example.elb.amazonaws.com"),
            ("k8s-api.example.elb.amazonaws.com".to_string(), 80)
        );
        assert_eq!(
            dns_lookup_target("http://k8s-api.example.elb.amazonaws.com:8080/health"),
            ("k8s-api.example.elb.amazonaws.com".to_string(), 8080)
        );
        assert_eq!(
            dns_lookup_target("https://api.example.com."),
            ("api.example.com".to_string(), 443)
        );
    }

    #[test]
    fn endpoint_health_check_url_joins_path_forms() {
        assert_eq!(
            endpoint_health_check_url("http://8.8.8.8", "/health"),
            "http://8.8.8.8/health"
        );
        assert_eq!(
            endpoint_health_check_url("http://8.8.8.8/", "health"),
            "http://8.8.8.8/health"
        );
    }

    #[tokio::test]
    async fn public_endpoint_health_check_waits_for_success_response() {
        let server = MockServer::start_async().await;
        let _not_ready = server
            .mock_async(|when, then| {
                when.method(GET).path("/not-ready");
                then.status(503).body("not ready");
            })
            .await;
        let _ready = server
            .mock_async(|when, then| {
                when.method(GET).path("/health");
                then.status(200).body("ok");
            })
            .await;

        assert!(
            !public_endpoint_health_check_succeeds(&server.base_url(), "/not-ready").await,
            "non-success health responses must keep the endpoint waiting"
        );
        assert!(
            public_endpoint_health_check_succeeds(&server.base_url(), "/health").await,
            "success health response should allow the endpoint to publish the URL"
        );
    }

    #[test]
    fn aws_alb_ingress_keeps_explicit_health_check_annotations() {
        let mut target = endpoint_target();
        target.health_check_path = Some("/ready".to_string());
        let plan = EndpointPlan {
            managed_hostnames: None,
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
            managed_hostnames: None,
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
            managed_hostnames: None,
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

    // ─────────────── MANAGED ACM IMPORTS ────────────────

    #[cfg(feature = "aws")]
    mod managed_acm {
        use super::*;
        use crate::core::kubernetes_manifest_test_support::KubernetesManifestTestHarness;
        use crate::core::MockPlatformServiceProvider;
        use alien_aws_clients::acm::{
            CertificateSummary, ImportCertificateResponse, ListCertificatesResponse, MockAcmApi,
            Tag,
        };
        use alien_core::{
            standard_resource_tags, AwsClientConfig, AwsCredentials, ClientConfig, DnsRecordStatus,
            DomainMetadata, KubernetesSettings, Resource, ResourceDomainInfo, Worker, WorkerCode,
        };
        use alien_k8s_clients::kubernetes::services::MockServiceApi;
        use alien_k8s_clients::{RouteApi, ServiceApi};
        use std::sync::{Arc, Mutex};

        /// ACM as these tests see it: the imported certificates, by ARN, with their tags.
        type CertificateWorld = Arc<Mutex<Vec<(String, Vec<Tag>)>>>;

        fn arn(n: usize) -> String {
            format!("arn:aws:acm:us-east-1:123456789012:certificate/imported-{n}")
        }

        fn access_denied() -> AlienError<CloudClientErrorData> {
            AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                resource_type: "Certificate".to_string(),
                resource_name: "certificate".to_string(),
            })
        }

        fn not_found() -> AlienError<CloudClientErrorData> {
            AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Certificate".to_string(),
                resource_name: "certificate".to_string(),
            })
        }

        #[derive(Clone, Default)]
        struct AcmCalls {
            imports: Arc<Mutex<Vec<Vec<Tag>>>>,
            deletes: Arc<Mutex<Vec<String>>>,
        }

        /// Whether the runtime role's tag-conditioned grant covers a certificate with `tags`.
        fn ours(tags: &[Tag]) -> bool {
            standard_resource_tags("test", "api")
                .into_iter()
                .all(|(key, value)| tags.contains(&Tag { key, value }))
        }

        /// The tags an import by this endpoint carries, plus `token`.
        fn our_tags(token: &str) -> Vec<Tag> {
            let mut tags: Vec<Tag> = standard_resource_tags("test", "api")
                .into_iter()
                .map(|(key, value)| Tag { key, value })
                .collect();
            tags.push(Tag {
                key: "CreateAttempt".to_string(),
                value: token.to_string(),
            });
            tags
        }

        /// An ACM client over `world`, as the runtime role sees it (checked live): a
        /// certificate that does not exist is NotFound for every call, and one that exists
        /// without this resource's tags is AccessDenied for the tag-conditioned delete,
        /// describe and tag read. While `lose_responses` is above zero, an import ACM accepted
        /// answers 503 instead.
        fn world_acm(
            world: CertificateWorld,
            calls: AcmCalls,
            lose_responses: usize,
        ) -> MockAcmApi {
            fn find(
                world: &CertificateWorld,
                certificate_arn: &str,
            ) -> std::result::Result<Vec<Tag>, AlienError<CloudClientErrorData>> {
                let tags = world
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(stored, _)| stored == certificate_arn)
                    .map(|(_, tags)| tags.clone())
                    .ok_or_else(not_found)?;
                if ours(&tags) {
                    Ok(tags)
                } else {
                    Err(access_denied())
                }
            }

            let mut acm = MockAcmApi::new();
            let stored = world.clone();
            let imports = calls.imports.clone();
            let mut lose_responses = lose_responses;
            acm.expect_import_certificate().returning(move |request| {
                let tags = request.tags.clone().unwrap_or_default();
                imports.lock().unwrap().push(tags.clone());
                let mut certificates = stored.lock().unwrap();
                let certificate_arn = arn(certificates.len() + 1);
                certificates.push((certificate_arn.clone(), tags));
                if lose_responses > 0 {
                    lose_responses -= 1;
                    return Err(AlienError::new(
                        CloudClientErrorData::RemoteServiceUnavailable {
                            message: "connection reset".to_string(),
                        },
                    ));
                }
                Ok(ImportCertificateResponse { certificate_arn })
            });
            let listed = world.clone();
            acm.expect_list_certificates().returning(move |_| {
                Ok(ListCertificatesResponse {
                    certificate_summary_list: listed
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|(certificate_arn, _)| CertificateSummary {
                            certificate_arn: Some(certificate_arn.clone()),
                            domain_name: Some("api.example.com".to_string()),
                            status: Some("ISSUED".to_string()),
                            certificate_type: Some("IMPORTED".to_string()),
                            key_algorithm: None,
                            in_use: Some(false),
                            imported_at: None,
                        })
                        .collect(),
                    next_token: None,
                })
            });
            let tagged = world.clone();
            acm.expect_list_tags_for_certificate()
                .returning(move |certificate_arn| find(&tagged, certificate_arn));
            let removed = world.clone();
            let deletes = calls.deletes.clone();
            acm.expect_delete_certificate()
                .returning(move |certificate_arn| {
                    deletes.lock().unwrap().push(certificate_arn.to_string());
                    find(&removed, certificate_arn)?;
                    removed
                        .lock()
                        .unwrap()
                        .retain(|(stored, _)| stored != certificate_arn);
                    Ok(())
                });
            let described = world;
            acm.expect_describe_certificate()
                .returning(move |certificate_arn| {
                    find(&described, certificate_arn)?;
                    Ok(alien_aws_clients::acm::DescribeCertificateResponse { certificate: None })
                });
            acm
        }

        /// The cluster's routes: creating the Ingress fails, as an API server error does.
        #[derive(Debug)]
        struct IngressCreateFails;

        #[async_trait::async_trait]
        impl RouteApi for IngressCreateFails {
            async fn create_ingress(
                &self,
                _namespace: &str,
                _ingress: &K8sIngress,
            ) -> alien_client_core::Result<K8sIngress> {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteServiceUnavailable {
                        message: "the API server is unavailable".to_string(),
                    },
                ))
            }
            async fn get_ingress(&self, _: &str, _: &str) -> alien_client_core::Result<K8sIngress> {
                unreachable!("no Ingress exists")
            }
            async fn update_ingress(
                &self,
                _: &str,
                _: &str,
                _: &K8sIngress,
            ) -> alien_client_core::Result<K8sIngress> {
                unreachable!("no Ingress exists")
            }
            async fn delete_ingress(&self, _: &str, _: &str) -> alien_client_core::Result<()> {
                unreachable!("nothing is deleted")
            }
            async fn create_gateway(&self, _: &str, _: &Value) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn get_gateway(&self, _: &str, _: &str) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn update_gateway(
                &self,
                _: &str,
                _: &str,
                _: &Value,
            ) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn delete_gateway(&self, _: &str, _: &str) -> alien_client_core::Result<()> {
                unreachable!("the route is an Ingress")
            }
            async fn create_http_route(
                &self,
                _: &str,
                _: &Value,
            ) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn get_http_route(&self, _: &str, _: &str) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn update_http_route(
                &self,
                _: &str,
                _: &str,
                _: &Value,
            ) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn delete_http_route(&self, _: &str, _: &str) -> alien_client_core::Result<()> {
                unreachable!("the route is an Ingress")
            }
            async fn create_gke_health_check_policy(
                &self,
                _: &str,
                _: &Value,
            ) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn get_gke_health_check_policy(
                &self,
                _: &str,
                _: &str,
            ) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn update_gke_health_check_policy(
                &self,
                _: &str,
                _: &str,
                _: &Value,
            ) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn delete_gke_health_check_policy(
                &self,
                _: &str,
                _: &str,
            ) -> alien_client_core::Result<()> {
                unreachable!("the route is an Ingress")
            }
            async fn create_azure_health_check_policy(
                &self,
                _: &str,
                _: &Value,
            ) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn get_azure_health_check_policy(
                &self,
                _: &str,
                _: &str,
            ) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn update_azure_health_check_policy(
                &self,
                _: &str,
                _: &str,
                _: &Value,
            ) -> alien_client_core::Result<Value> {
                unreachable!("the route is an Ingress")
            }
            async fn delete_azure_health_check_policy(
                &self,
                _: &str,
                _: &str,
            ) -> alien_client_core::Result<()> {
                unreachable!("the route is an Ingress")
            }
        }

        /// The cluster's own region.
        const CLUSTER_REGION: &str = "us-east-1";

        fn managed_acm_import(region: Option<&str>) -> KubernetesCertificateMode {
            KubernetesCertificateMode::ManagedAcmImport {
                region: region.map(str::to_string),
                tags: HashMap::new(),
            }
        }

        /// An EKS cluster in `CLUSTER_REGION` whose public endpoint imports the issued
        /// certificate into ACM in that region.
        fn harness(acm: MockAcmApi) -> KubernetesManifestTestHarness {
            harness_with(vec![(CLUSTER_REGION, acm)], managed_acm_import(None))
        }

        /// An EKS cluster in `CLUSTER_REGION` with `certificate` as its endpoints' certificate
        /// mode, and one ACM client per region. A call into any other region fails the test.
        fn harness_with(
            acm_by_region: Vec<(&str, MockAcmApi)>,
            certificate: KubernetesCertificateMode,
        ) -> KubernetesManifestTestHarness {
            let acm_by_region: HashMap<String, Arc<dyn alien_aws_clients::acm::AcmApi>> =
                acm_by_region
                    .into_iter()
                    .map(|(region, acm)| {
                        (
                            region.to_string(),
                            Arc::new(acm) as Arc<dyn alien_aws_clients::acm::AcmApi>,
                        )
                    })
                    .collect();
            let mut services = MockServiceApi::new();
            services
                .expect_create_service()
                .returning(|_, service| Ok(service.clone()));
            let services: Arc<dyn ServiceApi> = Arc::new(services);
            let routes: Arc<dyn RouteApi> = Arc::new(IngressCreateFails);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_acm_client()
                .returning(move |config| {
                    Ok(acm_by_region
                        .get(&config.region)
                        .unwrap_or_else(|| panic!("no ACM call expected in {}", config.region))
                        .clone())
                });
            provider
                .expect_get_kubernetes_service_client()
                .returning(move |_| Ok(services.clone()));
            provider
                .expect_get_kubernetes_route_client()
                .returning(move |_| Ok(routes.clone()));

            let worker = Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "api:latest".to_string(),
                })
                .permissions("execution".to_string())
                .build();
            let mut harness = KubernetesManifestTestHarness::new(Resource::new(worker), vec![])
                .with_service_provider(Arc::new(provider))
                .with_cloud(ClientConfig::Aws(Box::new(AwsClientConfig {
                    account_id: "123456789012".to_string(),
                    region: CLUSTER_REGION.to_string(),
                    credentials: AwsCredentials::AccessKeys {
                        access_key_id: "test".to_string(),
                        secret_access_key: "test".to_string(),
                        session_token: None,
                    },
                    service_overrides: None,
                })));
            let config = harness.deployment_config_mut();
            config.stack_settings.kubernetes = Some(KubernetesSettings {
                cluster: None,
                exposure: Some(KubernetesExposureSettings::Generated {
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
                    certificate,
                }),
            });
            config.domain_metadata = Some(DomainMetadata {
                base_domain: "example.com".to_string(),
                public_subdomain: "app".to_string(),
                hosted_zone_id: "Z1234567890ABC".to_string(),
                resources: HashMap::from([(
                    "api".to_string(),
                    ResourceDomainInfo {
                        fqdn: "api.example.com".to_string(),
                        certificate_id: "cert-1".to_string(),
                        certificate_status: CertificateStatus::Issued,
                        dns_status: DnsRecordStatus::Active,
                        dns_error: None,
                        certificate_chain: Some(
                            "-----BEGIN CERTIFICATE-----\nMIIBtest\n-----END CERTIFICATE-----\n"
                                .to_string(),
                        ),
                        private_key: Some(
                            "-----BEGIN PRIVATE KEY-----\nMIIBtest\n-----END PRIVATE KEY-----\n"
                                .to_string(),
                        ),
                        endpoints: HashMap::new(),
                        aliases: Vec::new(),
                        issued_at: Some("2026-01-01T00:00:00Z".to_string()),
                    },
                )]),
            });
            harness
        }

        fn import_token(tags: &[Tag]) -> Option<&str> {
            tags.iter()
                .find(|tag| tag.key == "CreateAttempt")
                .map(|tag| tag.value.as_str())
        }

        /// The first reconcile records the import token and stops before importing, so the
        /// controller saves it. The next one imports with the token, and the import's ARN is
        /// kept although creating the Ingress fails afterwards; the retry does not import again.
        #[tokio::test]
        async fn import_token_is_saved_first_and_the_arn_survives_a_later_failure() {
            let world = CertificateWorld::default();
            let calls = AcmCalls::default();
            let harness = harness(world_acm(world.clone(), calls.clone(), 0));
            let mut state = KubernetesPublicEndpointState::default();

            let action =
                reconcile_kubernetes_public_endpoint(&harness.ctx(), endpoint_target(), &mut state)
                    .await
                    .expect("records the token");
            assert!(matches!(action, KubernetesEndpointAction::Waiting { .. }));
            let token = state.managed_acm_import_token.clone().expect("token saved");
            assert!(calls.imports.lock().unwrap().is_empty(), "no import yet");

            reconcile_kubernetes_public_endpoint(&harness.ctx(), endpoint_target(), &mut state)
                .await
                .expect_err("the Ingress create fails");
            assert_eq!(state.managed_acm_certificate_arn, Some(arn(1)));
            assert_eq!(state.published_certificate_id.as_deref(), Some("cert-1"));
            assert_eq!(
                import_token(&calls.imports.lock().unwrap()[0]),
                Some(token.as_str())
            );

            reconcile_kubernetes_public_endpoint(&harness.ctx(), endpoint_target(), &mut state)
                .await
                .expect_err("the Ingress create fails again");
            assert_eq!(calls.imports.lock().unwrap().len(), 1, "no second import");
            assert_eq!(world.lock().unwrap().len(), 1);
        }

        /// ACM imports the certificate but the process stops before the reconcile that imported
        /// is saved. The controller restarts from the state saved before it, which holds the
        /// token, and adopts the certificate instead of importing a second one.
        #[tokio::test]
        async fn certificate_imported_before_a_crash_is_adopted_from_the_previous_checkpoint() {
            let world = CertificateWorld::default();
            let calls = AcmCalls::default();
            let first = harness(world_acm(world.clone(), calls.clone(), 0));
            let mut state = KubernetesPublicEndpointState::default();
            reconcile_kubernetes_public_endpoint(&first.ctx(), endpoint_target(), &mut state)
                .await
                .expect("records the token");
            let checkpoint = state.clone();
            reconcile_kubernetes_public_endpoint(&first.ctx(), endpoint_target(), &mut state)
                .await
                .expect_err("imports, then the Ingress create fails");
            assert_eq!(calls.imports.lock().unwrap().len(), 1);

            let restarted = harness(world_acm(world.clone(), calls.clone(), 0));
            let mut state = checkpoint;
            reconcile_kubernetes_public_endpoint(&restarted.ctx(), endpoint_target(), &mut state)
                .await
                .expect_err("the Ingress create fails after the adoption");

            assert_eq!(state.managed_acm_certificate_arn, Some(arn(1)));
            assert_eq!(calls.imports.lock().unwrap().len(), 1, "no second import");
            assert_eq!(world.lock().unwrap().len(), 1);
        }

        /// The import reaches ACM but its response is lost; the retry adopts the certificate by
        /// its token instead of importing a second one.
        #[tokio::test]
        async fn lost_import_response_is_adopted_by_its_token() {
            let world = CertificateWorld::default();
            let calls = AcmCalls::default();
            let harness = harness(world_acm(world.clone(), calls.clone(), 1));
            let mut state = KubernetesPublicEndpointState::default();

            reconcile_kubernetes_public_endpoint(&harness.ctx(), endpoint_target(), &mut state)
                .await
                .expect("records the token");
            reconcile_kubernetes_public_endpoint(&harness.ctx(), endpoint_target(), &mut state)
                .await
                .expect_err("the import response is lost");
            assert_eq!(state.managed_acm_certificate_arn, None);
            reconcile_kubernetes_public_endpoint(&harness.ctx(), endpoint_target(), &mut state)
                .await
                .expect_err("the Ingress create fails after the adoption");

            assert_eq!(state.managed_acm_certificate_arn, Some(arn(1)));
            assert_eq!(calls.imports.lock().unwrap().len(), 1);
        }

        /// A lost import followed by a delete: the certificate is found by its token.
        #[tokio::test]
        async fn delete_finds_a_lost_import_by_its_token() {
            let world: CertificateWorld = Arc::new(Mutex::new(vec![(arn(1), our_tags("token-1"))]));
            let calls = AcmCalls::default();
            let harness = harness(world_acm(world.clone(), calls.clone(), 0));
            let mut state = KubernetesPublicEndpointState {
                managed_acm_import_token: Some("token-1".to_string()),
                ..Default::default()
            };

            delete_managed_acm_certificate(&harness.ctx(), "api", &mut state)
                .await
                .expect("deletes the certificate");

            assert_eq!(*calls.deletes.lock().unwrap(), [arn(1)]);
            assert!(world.lock().unwrap().is_empty());
            assert_eq!(state, KubernetesPublicEndpointState::default());
        }

        /// `managedAcmImport` names a region other than the cluster's. The import's response is
        /// lost; teardown finds the certificate by its token in that region and deletes it
        /// there. The cluster region's ACM is never called.
        #[tokio::test]
        async fn lost_import_in_a_configured_region_is_found_and_deleted_there() {
            let world = CertificateWorld::default();
            let calls = AcmCalls::default();
            let creating = harness_with(
                vec![("eu-west-1", world_acm(world.clone(), calls.clone(), 1))],
                managed_acm_import(Some("eu-west-1")),
            );
            let mut state = KubernetesPublicEndpointState::default();
            reconcile_kubernetes_public_endpoint(&creating.ctx(), endpoint_target(), &mut state)
                .await
                .expect("records the token");
            assert_eq!(state.managed_acm_region.as_deref(), Some("eu-west-1"));
            reconcile_kubernetes_public_endpoint(&creating.ctx(), endpoint_target(), &mut state)
                .await
                .expect_err("the import response is lost");
            assert_eq!(state.managed_acm_certificate_arn, None);
            assert_eq!(world.lock().unwrap().len(), 1, "ACM holds the certificate");

            let deleting = harness_with(
                vec![("eu-west-1", world_acm(world.clone(), calls.clone(), 0))],
                managed_acm_import(Some("eu-west-1")),
            );
            delete_managed_acm_certificate(&deleting.ctx(), "api", &mut state)
                .await
                .expect("deletes the certificate");

            assert_eq!(*calls.deletes.lock().unwrap(), [arn(1)]);
            assert!(world.lock().unwrap().is_empty(), "no certificate is left");
            assert_eq!(state, KubernetesPublicEndpointState::default());
        }

        /// An update that switches from `managedAcmImport` in another region to a mode with no
        /// region deletes the certificate in the region it was imported into.
        #[tokio::test]
        async fn switching_away_from_managed_acm_deletes_in_the_import_region() {
            let world = CertificateWorld::default();
            let calls = AcmCalls::default();
            let importing = harness_with(
                vec![("eu-west-1", world_acm(world.clone(), calls.clone(), 0))],
                managed_acm_import(Some("eu-west-1")),
            );
            let mut state = KubernetesPublicEndpointState::default();
            reconcile_kubernetes_public_endpoint(&importing.ctx(), endpoint_target(), &mut state)
                .await
                .expect("records the token");
            reconcile_kubernetes_public_endpoint(&importing.ctx(), endpoint_target(), &mut state)
                .await
                .expect_err("imports, then the Ingress create fails");
            assert_eq!(state.managed_acm_certificate_arn, Some(arn(1)));
            assert_eq!(state.managed_acm_region.as_deref(), Some("eu-west-1"));

            let switched = harness_with(
                vec![("eu-west-1", world_acm(world.clone(), calls.clone(), 0))],
                KubernetesCertificateMode::None,
            );
            clean_up_after_leaving_managed_acm(&switched, &mut state)
                .await
                .expect("deletes the certificate");

            assert_eq!(*calls.deletes.lock().unwrap(), [arn(1)]);
            assert!(world.lock().unwrap().is_empty());
            assert_eq!(state.managed_acm_certificate_arn, None);
            assert_eq!(state.managed_acm_import_token, None);
            assert_eq!(state.managed_acm_region, None);
        }

        /// State saved before the import region was recorded has only the ARN; the delete
        /// uses the region in it.
        #[tokio::test]
        async fn certificate_recorded_without_a_region_is_deleted_in_its_arn_region() {
            let old_arn = "arn:aws:acm:eu-west-1:123456789012:certificate/old".to_string();
            let world: CertificateWorld =
                Arc::new(Mutex::new(vec![(old_arn.clone(), our_tags("unused"))]));
            let calls = AcmCalls::default();
            let harness = harness_with(
                vec![("eu-west-1", world_acm(world.clone(), calls.clone(), 0))],
                KubernetesCertificateMode::None,
            );
            let mut value = serde_json::to_value(KubernetesPublicEndpointState {
                managed_acm_certificate_arn: Some(old_arn.clone()),
                ..Default::default()
            })
            .unwrap();
            value.as_object_mut().unwrap().remove("managedAcmRegion");
            value
                .as_object_mut()
                .unwrap()
                .remove("managedAcmImportToken");
            let mut state: KubernetesPublicEndpointState = serde_json::from_value(value).unwrap();

            delete_managed_acm_certificate(&harness.ctx(), "api", &mut state)
                .await
                .expect("deletes the certificate");

            assert_eq!(*calls.deletes.lock().unwrap(), [old_arn]);
            assert!(world.lock().unwrap().is_empty());
        }

        /// A certificate stays in the region it was imported into: changing the configured
        /// region under it fails loudly instead of reimporting into a region where the ARN
        /// does not exist.
        #[tokio::test]
        async fn changing_the_region_of_an_imported_certificate_is_refused() {
            let harness = harness_with(vec![], managed_acm_import(Some("eu-central-1")));
            let mut state = KubernetesPublicEndpointState {
                managed_acm_certificate_arn: Some(
                    "arn:aws:acm:eu-west-1:123456789012:certificate/imported".to_string(),
                ),
                managed_acm_region: Some("eu-west-1".to_string()),
                ..Default::default()
            };

            let error =
                reconcile_kubernetes_public_endpoint(&harness.ctx(), endpoint_target(), &mut state)
                    .await
                    .expect_err("the region change is refused");

            assert!(error.message.contains("eu-west-1"), "{}", error.message);
            assert!(error.message.contains("eu-central-1"), "{}", error.message);
            assert_eq!(state.managed_acm_region.as_deref(), Some("eu-west-1"));
        }

        /// Runs the stale-object cleanup of an update that switched away from
        /// `managedAcmImport`: no route or Secret changed, so it only removes the certificate.
        async fn clean_up_after_leaving_managed_acm(
            harness: &KubernetesManifestTestHarness,
            state: &mut KubernetesPublicEndpointState,
        ) -> Result<()> {
            let routes: Arc<dyn RouteApi> = Arc::new(IngressCreateFails);
            cleanup_stale_endpoint_objects(
                &harness.ctx(),
                "app",
                "api",
                "api-v1",
                "container",
                &routes,
                PreviousEndpointObjects {
                    ingress_name: None,
                    gateway_name: None,
                    http_route_name: None,
                    gke_health_check_policy_name: None,
                    azure_health_check_policy_name: None,
                    managed_tls_secret_name: None,
                    managed_tls_certificate_id: None,
                },
                ActiveEndpointObjects {
                    ingress_name: None,
                    gateway_name: None,
                    http_route_name: None,
                    gke_health_check_policy_name: None,
                    azure_health_check_policy_name: None,
                    managed_tls_secret_name: None,
                    managed_acm_certificate: false,
                },
                state,
            )
            .await
        }

        /// An update that switches away from `managedAcmImport` deletes the recorded
        /// certificate and every other import under its token.
        #[tokio::test]
        async fn update_away_from_managed_acm_deletes_every_import_under_the_token() {
            let world: CertificateWorld = Arc::new(Mutex::new(vec![
                (arn(1), our_tags("token-1")),
                (arn(2), our_tags("token-1")),
                (arn(3), our_tags("another-endpoint")),
            ]));
            let calls = AcmCalls::default();
            let harness = harness(world_acm(world.clone(), calls.clone(), 0));
            let mut state = KubernetesPublicEndpointState {
                managed_acm_certificate_arn: Some(arn(2)),
                managed_acm_import_token: Some("token-1".to_string()),
                ..Default::default()
            };

            clean_up_after_leaving_managed_acm(&harness, &mut state)
                .await
                .expect("deletes the certificates");

            assert_eq!(*calls.deletes.lock().unwrap(), [arn(2), arn(1)]);
            assert_eq!(
                world
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(certificate_arn, _)| certificate_arn.clone())
                    .collect::<Vec<_>>(),
                [arn(3)]
            );
            assert_eq!(state.managed_acm_certificate_arn, None);
            assert_eq!(state.managed_acm_import_token, None);
        }

        /// The recorded certificate is already gone (deleted out of band, or by an attempt
        /// whose response was lost): the update goes on.
        #[tokio::test]
        async fn update_away_from_managed_acm_finishes_when_the_certificate_is_gone() {
            let calls = AcmCalls::default();
            let harness = harness(world_acm(CertificateWorld::default(), calls.clone(), 0));
            let mut state = KubernetesPublicEndpointState {
                managed_acm_certificate_arn: Some(arn(1)),
                managed_acm_import_token: Some("token-1".to_string()),
                ..Default::default()
            };

            clean_up_after_leaving_managed_acm(&harness, &mut state)
                .await
                .expect("a certificate that is gone needs no delete");

            assert_eq!(*calls.deletes.lock().unwrap(), [arn(1)]);
            assert_eq!(state.managed_acm_certificate_arn, None);
            assert_eq!(state.managed_acm_import_token, None);
        }

        /// The recorded certificate exists but no longer carries this resource's tags, so the
        /// tag-conditioned delete and describe are both denied. It is not this resource's to
        /// delete any more: the update goes on instead of failing with a permission error no
        /// grant can fix, and the certificate is left alone.
        #[tokio::test]
        async fn update_away_from_managed_acm_leaves_a_certificate_that_is_no_longer_ours() {
            let retagged = vec![Tag {
                key: "deployment".to_string(),
                value: "someone-else".to_string(),
            }];
            let world: CertificateWorld = Arc::new(Mutex::new(vec![(arn(1), retagged)]));
            let calls = AcmCalls::default();
            let harness = harness(world_acm(world.clone(), calls.clone(), 0));
            let mut state = KubernetesPublicEndpointState {
                managed_acm_certificate_arn: Some(arn(1)),
                ..Default::default()
            };

            clean_up_after_leaving_managed_acm(&harness, &mut state)
                .await
                .expect("a certificate that is not ours is not deleted");

            assert_eq!(*calls.deletes.lock().unwrap(), [arn(1)]);
            assert_eq!(
                world.lock().unwrap().len(),
                1,
                "the certificate is left alone"
            );
            assert_eq!(state.managed_acm_certificate_arn, None);
        }

        /// When the certificate is still readable, its denied delete is a real denial: the
        /// error is returned and the ARN kept for the retry.
        #[tokio::test]
        async fn denied_delete_of_a_certificate_that_still_exists_is_an_error() {
            let mut acm = MockAcmApi::new();
            acm.expect_delete_certificate()
                .withf(|certificate_arn| certificate_arn == arn(1))
                .times(1)
                .returning(|_| Err(access_denied()));
            acm.expect_describe_certificate().times(1).returning(|_| {
                Ok(alien_aws_clients::acm::DescribeCertificateResponse { certificate: None })
            });
            let harness = harness(acm);
            let mut state = KubernetesPublicEndpointState {
                managed_acm_certificate_arn: Some(arn(1)),
                ..Default::default()
            };

            let error = delete_managed_acm_certificate(&harness.ctx(), "api", &mut state)
                .await
                .expect_err("the denial is real");

            let mut codes = vec![error.code.clone()];
            let mut source = error.source.as_deref();
            while let Some(inner) = source {
                codes.push(inner.code.clone());
                source = inner.source.as_deref();
            }
            assert!(
                codes.contains(&"REMOTE_ACCESS_DENIED".to_string()),
                "{codes:?}"
            );
            assert_eq!(state.managed_acm_certificate_arn, Some(arn(1)));
        }

        /// A delete answered NotFound is finished without a describe.
        #[tokio::test]
        async fn not_found_delete_is_finished() {
            let mut acm = MockAcmApi::new();
            acm.expect_delete_certificate()
                .times(1)
                .returning(|_| Err(not_found()));
            acm.expect_describe_certificate().times(0);
            let harness = harness(acm);
            let mut state = KubernetesPublicEndpointState {
                managed_acm_certificate_arn: Some(arn(1)),
                ..Default::default()
            };

            delete_managed_acm_certificate(&harness.ctx(), "api", &mut state)
                .await
                .expect("a certificate that is gone needs no delete");
            assert_eq!(state.managed_acm_certificate_arn, None);
        }
    }
}
