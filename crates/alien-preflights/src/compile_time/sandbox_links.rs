//! Refuses a sandbox link the deployment cannot deliver through its egress gateway.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Sandbox, Stack};

/// Ensures every `Sandbox.link` names a linkable service the platform's gateway can reach.
pub struct SandboxLinksCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for SandboxLinksCheck {
    fn description(&self) -> &'static str {
        "A Sandbox may link only services its deployment's egress gateway can reach"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Sandbox>()
                .is_some_and(|sandbox| !sandbox.gateway_links.is_empty())
        })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let errors: Vec<String> = stack
            .resources()
            .filter_map(|(_, entry)| entry.config.downcast_ref::<Sandbox>())
            .filter_map(|sandbox| {
                alien_core::sandbox_links::validate_sandbox_links(stack, sandbox, platform)
                    .err()
                    .map(|reason| {
                        format!("Sandbox '{}': SANDBOX_LINK_INVALID: {reason}", sandbox.id())
                    })
            })
            .collect();
        Ok(if errors.is_empty() {
            CheckResult::success()
        } else {
            CheckResult::failed(errors)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Container, ContainerCode, ResourceLifecycle, ResourceSpec, SandboxCode, SandboxEgress,
        SandboxLifecyclePolicy, Storage,
    };

    /// A gateway image newer than every pin that predates links.
    const LINKS_GATEWAY: &str = "ghcr.io/alienplatform/alien-egress-gateway@sha256:0000000000000000000000000000000000000000000000000000000000000001";

    fn container(id: &str, ports: &[u16], image: &str) -> Container {
        let mut builder = Container::new(id.into())
            .code(ContainerCode::Image {
                image: image.into(),
            })
            .permissions("execution".into())
            .cpu(ResourceSpec {
                min: "1".into(),
                desired: "1".into(),
            })
            .memory(ResourceSpec {
                min: "1Gi".into(),
                desired: "1Gi".into(),
            });
        for &port in ports {
            builder = builder.port(port);
        }
        builder.build()
    }

    fn sandbox(egress: SandboxEgress, links: &[&dyn Fn() -> alien_core::ResourceRef]) -> Sandbox {
        let mut sandbox = Sandbox::new("agents".into())
            .code(SandboxCode::Image {
                image: "ubuntu".into(),
                bundled_from: None,
            })
            .egress(egress)
            .lifecycle(SandboxLifecyclePolicy::default())
            .build();
        sandbox.gateway_links = links.iter().map(|link| link()).collect();
        sandbox
    }

    fn api() -> alien_core::ResourceRef {
        alien_core::ResourceRef::new(Container::RESOURCE_TYPE, "api")
    }

    fn stack(sandbox: Sandbox, gateway_image: Option<&str>) -> Stack {
        let mut stack = Stack::new("links".into())
            .add(
                container("api", &[8080], "example.com/api:1"),
                ResourceLifecycle::Live,
            )
            .add(
                Storage::new("data".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                container("silent", &[], "example.com/silent:1"),
                ResourceLifecycle::Live,
            )
            .add(sandbox, ResourceLifecycle::Live);
        if let Some(image) = gateway_image {
            stack = stack.add(
                container(
                    alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID,
                    &[3128, 3129],
                    image,
                ),
                ResourceLifecycle::Live,
            );
        }
        stack.build()
    }

    async fn errors(stack: &Stack, platform: Platform) -> Vec<String> {
        SandboxLinksCheck
            .check(stack, platform)
            .await
            .unwrap()
            .errors
    }

    #[tokio::test]
    async fn a_linked_container_plans_where_the_gateway_reaches_it() {
        let stack = stack(
            sandbox(SandboxEgress::Internet, &[&api]),
            Some(LINKS_GATEWAY),
        );
        for platform in [Platform::Aws, Platform::Kubernetes, Platform::Local] {
            assert!(errors(&stack, platform).await.is_empty(), "{platform}");
        }
        assert_eq!(
            alien_core::sandbox_links::gateway_link_targets(&stack)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![("api".to_string(), vec![8080])]
        );
    }

    #[tokio::test]
    async fn platforms_without_a_route_refuse_with_the_reason() {
        let stack = stack(
            sandbox(SandboxEgress::Internet, &[&api]),
            Some(LINKS_GATEWAY),
        );
        let gcp = errors(&stack, Platform::Gcp).await;
        assert!(
            gcp[0].contains("not available on Google Cloud yet"),
            "{gcp:?}"
        );
    }

    #[tokio::test]
    async fn azure_links_plan_once_the_gateway_image_serves_them() {
        // A single-port container without a public endpoint links on Azure.
        let linked = stack(
            sandbox(SandboxEgress::Internet, &[&api]),
            Some(LINKS_GATEWAY),
        );
        assert!(errors(&linked, Platform::Azure).await.is_empty());
        // The released gateway image refuses Azure links as it does everywhere.
        let released = stack(sandbox(SandboxEgress::Internet, &[&api]), None);
        let refused = errors(&released, Platform::Azure).await;
        assert!(refused[0].contains("cannot enforce links"), "{refused:?}");
    }

    #[tokio::test]
    async fn the_released_gateway_image_refuses_links() {
        // No gateway Container yet: planning reads the pinned image, which predates links.
        let stack = stack(sandbox(SandboxEgress::Internet, &[&api]), None);
        let refused = errors(&stack, Platform::Kubernetes).await;
        assert!(refused[0].contains("cannot enforce links"), "{refused:?}");
    }

    #[tokio::test]
    async fn only_internet_sandboxes_link_and_only_containers_with_ports() {
        let data = || alien_core::ResourceRef::new(Storage::RESOURCE_TYPE, "data");
        let silent = || alien_core::ResourceRef::new(Container::RESOURCE_TYPE, "silent");
        let missing = || alien_core::ResourceRef::new(Container::RESOURCE_TYPE, "missing");
        let gateway = || {
            alien_core::ResourceRef::new(
                Container::RESOURCE_TYPE,
                alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID,
            )
        };
        let cases: [(SandboxEgress, &dyn Fn() -> alien_core::ResourceRef, &str); 5] = [
            (SandboxEgress::None, &api, "need egress `internet`"),
            (SandboxEgress::Internet, &data, "not a linkable service"),
            (SandboxEgress::Internet, &silent, "declares no port"),
            (SandboxEgress::Internet, &missing, "no such resource"),
            (SandboxEgress::Internet, &gateway, "egress gateway"),
        ];
        for (egress, link, reason) in cases {
            let stack = stack(sandbox(egress, &[link]), Some(LINKS_GATEWAY));
            let refused = errors(&stack, Platform::Kubernetes).await;
            assert!(
                refused.len() == 1 && refused[0].contains(reason),
                "{reason}: {refused:?}"
            );
        }
    }

    /// Planning refuses every link the gateway would refuse, so a published configuration never
    /// carries one that stops the gateway from loading it, and the union stays within what the
    /// compute cluster's runtime takes.
    #[tokio::test]
    async fn planning_refuses_what_the_gateway_and_the_runtime_refuse() {
        let to = |id: &str| alien_core::ResourceRef::new(Container::RESOURCE_TYPE, id);
        let linking = |id: &str, links: Vec<alien_core::ResourceRef>| {
            let mut linking = sandbox(SandboxEgress::Internet, &[]);
            linking.id = id.into();
            linking.gateway_links = links;
            linking
        };
        let planned = |services: Vec<Container>, sandboxes: Vec<Sandbox>| {
            let mut stack = Stack::new("links".into()).add(
                container(
                    alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID,
                    &[3128, 3129],
                    LINKS_GATEWAY,
                ),
                ResourceLifecycle::Live,
            );
            for service in services {
                stack = stack.add(service, ResourceLifecycle::Live);
            }
            for sandbox in sandboxes {
                stack = stack.add(sandbox, ResourceLifecycle::Live);
            }
            stack.build()
        };

        let dns = planned(
            vec![container("dns", &[53], "example.com/dns:1")],
            vec![linking("agents", vec![to("dns")])],
        );
        let refused = errors(&dns, Platform::Kubernetes).await;
        assert!(
            refused[0].contains("port 53 cannot be linked"),
            "{refused:?}"
        );

        let wide = |count: u16| {
            let ports: Vec<u16> = (8000..8000 + count).collect();
            planned(
                vec![container("wide", &ports, "example.com/wide:1")],
                vec![linking("agents", vec![to("wide")])],
            )
        };
        let refused = errors(&wide(17), Platform::Kubernetes).await;
        assert!(refused[0].contains("at most 16 ports"), "{refused:?}");
        assert!(errors(&wide(16), Platform::Kubernetes).await.is_empty());

        // 16 services across the deployment plan; a 17th, linked by a second sandbox, does not.
        let services = || -> Vec<Container> {
            (0..17)
                .map(|n| container(&format!("svc{n}"), &[8080], "example.com/svc:1"))
                .collect()
        };
        let sixteen = linking("agents", (0..16).map(|n| to(&format!("svc{n}"))).collect());
        let fits = planned(services(), vec![sixteen.clone()]);
        assert!(errors(&fits, Platform::Kubernetes).await.is_empty());
        let over = planned(
            services(),
            vec![sixteen, linking("workers", vec![to("svc16")])],
        );
        let refused = errors(&over, Platform::Kubernetes).await;
        assert!(
            refused
                .iter()
                .any(|error| error.contains("link 17 services; at most 16")),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_stack_without_links_is_not_checked() {
        let stack = stack(sandbox(SandboxEgress::Internet, &[]), None);
        assert!(!SandboxLinksCheck.should_run(&stack, Platform::Aws));
    }
}
