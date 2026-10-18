//! Worker-facing private ingress, certificate ownership and DNS outputs.

use std::collections::HashMap;

use alien_aws_clients::acm::{ImportCertificateRequest, Tag};
use alien_core::{
    CertificateStatus, DnsRecordStatus, ExposeProtocol, ManagedDomainInfo, Network,
    PublicEndpointOutput, ResourceRef, Worker, standard_resource_tags,
};
use alien_error::AlienError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::aws_gateway_cleanup::{GatewayCleanup, installed_function};
use super::aws_private_ingress::{
    PrivateIngressClients, PrivateIngressPlan, PrivateWorkerIngress, validate_worker_setup,
};
use crate::{
    AwsAcmImports, AwsNetworkController, ErrorData, ResourceControllerContext, Result,
    core::split_certificate_chain,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AwsPrivateWorker {
    graph: Option<PrivateWorkerIngress>,
    certificates: AwsAcmImports,
    pub endpoints: HashMap<String, PublicEndpointOutput>,
    #[serde(default)]
    applied_fingerprint: Option<String>,
}

struct Domains {
    hosts: Vec<String>,
    outputs: HashMap<String, PublicEndpointOutput>,
    certificate: Certificate,
    dns_ready: bool,
}

enum Certificate {
    Existing(String),
    Managed(ImportCertificateRequest),
}

impl Domains {
    fn fingerprint(&self) -> String {
        let certificate = match &self.certificate {
            Certificate::Existing(arn) => serde_json::json!(["existing", arn]),
            Certificate::Managed(request) => {
                serde_json::json!(["managed", request.certificate, request.certificate_chain])
            }
        };
        let mut names = self.outputs.keys().collect::<Vec<_>>();
        names.sort();
        let public_configuration = serde_json::json!([self.hosts, names, certificate]);
        format!(
            "{:x}",
            Sha256::digest(public_configuration.to_string().as_bytes())
        )
    }
}

impl AwsPrivateWorker {
    pub async fn maintain(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        if let Some(graph) = &mut self.graph {
            if graph.has_retired_tasks() {
                let ecs = ctx
                    .service_provider
                    .get_aws_ecs_client(ctx.get_aws_config()?)
                    .await?;
                let function_arn = installed_function(ctx)?;
                let lambda = ctx
                    .service_provider
                    .get_aws_lambda_client(ctx.get_aws_config()?)
                    .await?;
                graph
                    .maintain_retired_tasks(
                        ecs.as_ref(),
                        &GatewayCleanup {
                            lambda: lambda.as_ref(),
                            function_arn: &function_arn,
                        },
                    )
                    .await?;
            }
        }
        Ok(())
    }

    pub fn needs_update(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
    ) -> Result<bool> {
        if worker.public_endpoints.is_empty()
            || !ctx
                .deployment_config
                .stack_settings
                .endpoint_access
                .private_enabled()
        {
            return Ok(self.graph.is_some()
                || !self.certificates.is_empty()
                || !self.endpoints.is_empty());
        }
        let Some(domains) = resolve_domains(ctx, worker)? else {
            return Ok(true);
        };
        Ok(self.graph.is_none()
            || self.applied_fingerprint.as_deref() != Some(&domains.fingerprint()))
    }

    pub async fn prepare(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        function_arn: &str,
    ) -> Result<bool> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        if worker.public_endpoints.is_empty()
            || !ctx
                .deployment_config
                .stack_settings
                .endpoint_access
                .private_enabled()
        {
            return self.delete(ctx).await;
        }
        let cleanup_function_arn = installed_function(ctx)?;
        validate_worker_setup(ctx, &worker).await?;
        let Some(domains) = resolve_domains(ctx, &worker)? else {
            return Ok(false);
        };
        let fingerprint = domains.fingerprint();
        let network = ctx.require_dependency::<AwsNetworkController>(&ResourceRef::new(
            Network::RESOURCE_TYPE,
            "default-network",
        ))?;
        let aws = ctx.get_aws_config()?;
        let plan = PrivateIngressPlan {
            account: aws.account_id.clone(),
            region: aws.region.clone(),
            stack: ctx.resource_prefix.into(),
            resource: worker.id.clone(),
            vpc_id: network
                .vpc_id
                .clone()
                .ok_or_else(|| invalid("Private worker VPC missing"))?,
            private_subnets: network.private_subnet_ids.clone(),
            gateway_subnets: network.public_subnet_ids.clone(),
            domains: domains.hosts,
            function_arn: function_arn.into(),
        };
        if self.graph.is_none() {
            self.graph = Some(PrivateWorkerIngress::new(plan)?);
            return Ok(false);
        }
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| invalid("Private worker graph missing"))?;
        if !graph.configure_plan(&plan)? {
            return Ok(false);
        }
        let acm = ctx.service_provider.get_aws_acm_client(aws).await?;
        let certificate = match domains.certificate {
            Certificate::Existing(arn) => arn,
            Certificate::Managed(request) => {
                let Some(arn) = self.certificates.step_import(acm.as_ref(), request).await? else {
                    return Ok(false);
                };
                arn
            }
        };
        let api = ctx.service_provider.get_aws_apigateway_client(aws).await?;
        let ec2 = ctx.service_provider.get_aws_ec2_client(aws).await?;
        let ecs = ctx.service_provider.get_aws_ecs_client(aws).await?;
        let iam = ctx.service_provider.get_aws_iam_client(aws).await?;
        let elb = ctx.service_provider.get_aws_elbv2_client(aws).await?;
        let lambda = ctx.service_provider.get_aws_lambda_client(aws).await?;
        let clients = PrivateIngressClients {
            api: api.as_ref(),
            ec2: ec2.as_ref(),
            ecs: ecs.as_ref(),
            iam: iam.as_ref(),
            elb: elb.as_ref(),
            lambda: lambda.as_ref(),
            cleanup_function_arn: &cleanup_function_arn,
        };
        let Some(frontend) = graph.step_prepare(&clients, &certificate).await? else {
            return Ok(false);
        };
        let mut endpoints = domains.outputs;
        for output in endpoints.values_mut() {
            output.load_balancer_endpoint = Some(frontend.endpoint.clone());
        }
        if self.endpoints != endpoints {
            self.endpoints = endpoints;
            // Persist the target before consuming DNS readiness. The metadata
            // in this step was read before the DNS reconciler could see it.
            return Ok(false);
        }
        // Publish the healthy target before waiting for DNS. Keep every listener
        // certificate reference until rotation has been observed on the provider.
        if !self
            .certificates
            .step_retire(acm.as_ref(), graph.serving_certificate_arns())
            .await?
        {
            return Ok(false);
        }
        if domains.dns_ready {
            self.applied_fingerprint = Some(fingerprint);
        }
        Ok(domains.dns_ready)
    }

    pub async fn delete(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        if self.graph.is_none() && self.certificates.is_empty() {
            self.endpoints.clear();
            self.applied_fingerprint = None;
            return Ok(true);
        }
        let aws = ctx.get_aws_config()?;
        if let Some(graph) = &mut self.graph {
            let cleanup_function_arn = installed_function(ctx)?;
            let api = ctx.service_provider.get_aws_apigateway_client(aws).await?;
            let ec2 = ctx.service_provider.get_aws_ec2_client(aws).await?;
            let ecs = ctx.service_provider.get_aws_ecs_client(aws).await?;
            let iam = ctx.service_provider.get_aws_iam_client(aws).await?;
            let elb = ctx.service_provider.get_aws_elbv2_client(aws).await?;
            let lambda = ctx.service_provider.get_aws_lambda_client(aws).await?;
            let clients = PrivateIngressClients {
                api: api.as_ref(),
                ec2: ec2.as_ref(),
                ecs: ecs.as_ref(),
                iam: iam.as_ref(),
                elb: elb.as_ref(),
                lambda: lambda.as_ref(),
                cleanup_function_arn: &cleanup_function_arn,
            };
            if !graph.step_delete(&clients).await? {
                return Ok(false);
            }
            self.graph = None;
            self.endpoints.clear();
            self.applied_fingerprint = None;
            return Ok(false);
        }
        let acm = ctx.service_provider.get_aws_acm_client(aws).await?;
        self.certificates.step_retire(acm.as_ref(), &[]).await
    }
}

fn resolve_domains(
    ctx: &ResourceControllerContext<'_>,
    worker: &Worker,
) -> Result<Option<Domains>> {
    let custom = ctx
        .deployment_config
        .stack_settings
        .domains
        .as_ref()
        .and_then(|domains| domains.private_custom_domains.as_ref())
        .and_then(|domains| domains.get(&worker.id));
    let metadata = ctx
        .deployment_config
        .domain_metadata
        .as_ref()
        .and_then(|metadata| metadata.resources.get(&worker.id));
    let mut managed: Option<&ManagedDomainInfo> = None;
    let mut hosts = Vec::new();
    let mut outputs = HashMap::new();
    let mut dns_ready = true;
    for endpoint in &worker.public_endpoints {
        let host = if let Some(custom) = custom {
            custom.domain.clone()
        } else {
            let Some(domain) =
                metadata.and_then(|resource| resource.private_endpoints.get(&endpoint.name))
            else {
                return Ok(None);
            };
            if domain.certificate_status == CertificateStatus::Failed
                || domain.dns_status == DnsRecordStatus::Failed
            {
                return Err(invalid(
                    "Private worker certificate or DNS provisioning failed",
                ));
            }
            dns_ready &= domain.dns_status == DnsRecordStatus::Active;
            if let Some(previous) = managed {
                if previous.certificate_id != domain.certificate_id
                    || previous.certificate_chain != domain.certificate_chain
                {
                    return Err(invalid(
                        "Private worker endpoints must share one covering certificate",
                    ));
                }
            } else {
                managed = Some(domain);
            }
            domain.fqdn.clone()
        };
        let wildcard_host = endpoint.wildcard_subdomains.then(|| format!("*.{host}"));
        hosts.push(host.clone());
        hosts.extend(wildcard_host.iter().cloned());
        if outputs
            .insert(
                endpoint.name.clone(),
                PublicEndpointOutput {
                    url: format!("https://{host}"),
                    host,
                    wildcard_host,
                    protocol: ExposeProtocol::Http,
                    port: 443,
                    load_balancer_endpoint: None,
                },
            )
            .is_some()
        {
            return Err(invalid("Private worker endpoint names must be unique"));
        }
    }
    hosts.sort();
    hosts.dedup();
    let certificate = if let Some(custom) = custom {
        Certificate::Existing(
            custom
                .certificate
                .aws
                .as_ref()
                .map(|certificate| certificate.certificate_arn.clone())
                .filter(|arn| !arn.is_empty())
                .ok_or_else(|| invalid("Private custom domain requires an AWS certificate ARN"))?,
        )
    } else {
        let domain = managed.ok_or_else(|| invalid("Private worker requires endpoint domains"))?;
        if !matches!(
            domain.certificate_status,
            CertificateStatus::Issued
                | CertificateStatus::Renewing
                | CertificateStatus::RenewalFailed
        ) {
            return Ok(None);
        }
        let (leaf, chain) = split_certificate_chain(
            domain
                .certificate_chain
                .as_deref()
                .ok_or_else(|| invalid("Issued private certificate chain missing"))?,
        );
        Certificate::Managed(
            ImportCertificateRequest::builder()
                .certificate(leaf)
                .private_key(
                    domain
                        .private_key
                        .clone()
                        .ok_or_else(|| invalid("Issued private certificate key missing"))?,
                )
                .maybe_certificate_chain(chain)
                .tags(
                    standard_resource_tags(ctx.resource_prefix, &worker.id)
                        .into_iter()
                        .map(|(key, value)| Tag { key, value })
                        .collect(),
                )
                .build(),
        )
    };
    Ok(Some(Domains {
        hosts,
        outputs,
        certificate,
        dns_ready,
    }))
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.into(),
        resource_id: None,
    })
}
