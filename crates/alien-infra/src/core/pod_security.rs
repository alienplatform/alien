//! Pod Security `restricted` profile for the pods the Operator creates.
//!
//! Every container, daemon and worker pod is restricted unless its stack sets
//! `security.profile` to `unrestricted`. The profile is non-root, seccomp
//! `RuntimeDefault`, all capabilities dropped, no privilege escalation and a
//! read-only root filesystem, with writable emptyDirs for `/tmp` and the
//! paths the stack declares.

use std::collections::BTreeMap;

use alien_core::ContainerSecurity;
use alien_error::{AlienError, Context};
use alien_k8s_clients::label_selector;
use k8s_openapi::api::core::v1::{
    Capabilities, EmptyDirVolumeSource, Pod, PodSecurityContext, PodTemplateSpec, SeccompProfile,
    SecurityContext, Volume, VolumeMount,
};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

/// Name of the opt-out, as stacks spell it, for error messages.
pub const SECURITY_OPT_OUT: &str = "security: { profile: \"unrestricted\" }";

/// Writable path every restricted container gets.
const DEFAULT_WRITABLE_PATH: &str = "/tmp";
/// Group of the non-root user source-built Kubernetes images run as.
const DEFAULT_FS_GROUP: i64 = 65532;

/// Security settings to put on one pod and its single container.
#[derive(Debug, Default)]
pub struct PodSecurity {
    pub pod: Option<PodSecurityContext>,
    pub container: Option<SecurityContext>,
    pub volumes: Vec<Volume>,
    pub mounts: Vec<VolumeMount>,
}

/// Build the security context, volumes and mounts for a workload.
///
/// `existing_mounts` are paths the container already mounts; they are not
/// shadowed by a writable emptyDir.
pub fn pod_security(
    resource_id: &str,
    security: Option<&ContainerSecurity>,
    existing_mounts: &[VolumeMount],
) -> Result<PodSecurity> {
    if !ContainerSecurity::is_restricted(security) {
        return Ok(PodSecurity::default());
    }
    let invalid = |message: String| {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: resource_id.to_string(),
            message,
        })
    };
    let user = security.and_then(|security| security.run_as_user);
    let group = security.and_then(|security| security.run_as_group);
    if user.is_some_and(|id| id <= 0) || group.is_some_and(|id| id <= 0) {
        return Err(invalid(format!(
            "Restricted container security needs positive user and group IDs; to run as root, set {SECURITY_OPT_OUT}"
        )));
    }

    let mut paths = vec![DEFAULT_WRITABLE_PATH.to_string()];
    if let Some(security) = security {
        for path in &security.writable_paths {
            if !path.starts_with('/') || path == "/" {
                return Err(invalid(format!(
                    "security.writablePaths entry '{path}' must be an absolute path below /"
                )));
            }
            if !paths.contains(path) {
                paths.push(path.clone());
            }
        }
    }
    let mut volumes = Vec::new();
    let mut mounts = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if existing_mounts
            .iter()
            .any(|mount| mount.mount_path == *path)
        {
            continue;
        }
        let name = format!("writable-{index}");
        volumes.push(Volume {
            name: name.clone(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        });
        mounts.push(VolumeMount {
            name,
            mount_path: path.clone(),
            ..Default::default()
        });
    }

    let seccomp = SeccompProfile {
        type_: "RuntimeDefault".to_string(),
        ..Default::default()
    };
    Ok(PodSecurity {
        pod: Some(PodSecurityContext {
            run_as_non_root: Some(true),
            run_as_user: user,
            run_as_group: group,
            // Without an explicit user or group the pod runs as the image's own user (source-built
            // images: 65532). Give volumes that group so persistent storage is writable. When the
            // stack names a user, fsGroup stays as declared: a group-writable data directory is
            // rejected by some databases.
            fs_group: group.or_else(|| user.is_none().then_some(DEFAULT_FS_GROUP)),
            seccomp_profile: Some(seccomp.clone()),
            ..Default::default()
        }),
        container: Some(SecurityContext {
            allow_privilege_escalation: Some(false),
            privileged: Some(false),
            run_as_non_root: Some(true),
            read_only_root_filesystem: Some(
                security
                    .and_then(|security| security.read_only_root_filesystem)
                    .unwrap_or(true),
            ),
            capabilities: Some(Capabilities {
                drop: Some(vec!["ALL".to_string()]),
                ..Default::default()
            }),
            seccomp_profile: Some(seccomp),
            ..Default::default()
        }),
        volumes,
        mounts,
    })
}

/// The kubelet refuses to start a container whose image runs as root under
/// `runAsNonRoot`. Returns the message that names the opt-out when one of
/// `pods` is stuck that way.
///
/// Only pods of the desired `template` count: a pod of an earlier template that is still
/// stuck, or being deleted, is a leftover of the previous rollout, and a corrective release
/// must not fail because of it.
pub fn root_image_blocked(
    pods: &[Pod],
    template: &PodTemplateSpec,
    resource_id: &str,
) -> Option<String> {
    let blocked = pods
        .iter()
        .filter(|pod| pod.metadata.deletion_timestamp.is_none())
        .filter(|pod| pod_runs_template(pod, template))
        .any(|pod| {
            pod.status
                .as_ref()
                .and_then(|status| status.container_statuses.as_deref())
                .unwrap_or(&[])
                .iter()
                .filter_map(|status| status.state.as_ref()?.waiting.as_ref())
                .any(|waiting| {
                    waiting.reason.as_deref() == Some("CreateContainerConfigError")
                        && waiting
                            .message
                            .as_deref()
                            .is_some_and(|message| message.contains("runAsNonRoot"))
                })
        });
    blocked.then(|| {
        format!(
            "The image of '{resource_id}' runs as root, which the restricted security profile refuses. \
             Build the image with a non-root USER, set security.runAsUser, or opt out with {SECURITY_OPT_OUT}."
        )
    })
}

/// Whether `pod` was created from `template`: its containers run the template's images, and
/// the user settings the kubelet checks are the template's.
fn pod_runs_template(pod: &Pod, template: &PodTemplateSpec) -> bool {
    let Some(pod_spec) = pod.spec.as_ref() else {
        return false;
    };
    let Some(template_spec) = template.spec.as_ref() else {
        return false;
    };
    let user = |context: Option<&PodSecurityContext>| {
        context.map(|c| (c.run_as_non_root, c.run_as_user, c.run_as_group))
    };
    let pod_user = |spec: &k8s_openapi::api::core::v1::PodSpec| {
        user(spec.security_context.as_ref()).unwrap_or((None, None, None))
    };
    if pod_user(pod_spec) != pod_user(template_spec) {
        return false;
    }
    template_spec.containers.iter().all(|wanted| {
        pod_spec
            .containers
            .iter()
            .find(|container| container.name == wanted.name)
            .is_some_and(|actual| {
                let security = |container: &k8s_openapi::api::core::v1::Container| {
                    container
                        .security_context
                        .as_ref()
                        .map(|c| (c.run_as_non_root, c.run_as_user))
                        .unwrap_or((None, None))
                };
                actual.image == wanted.image && security(actual) == security(wanted)
            })
    })
}

/// Fail the controller step when a pod of the workload cannot start because
/// its image runs as root, so the error names the opt-out instead of the
/// step timing out.
pub async fn fail_if_root_image_blocked(
    ctx: &ResourceControllerContext<'_>,
    namespace: &str,
    selector: &BTreeMap<String, String>,
    template: &PodTemplateSpec,
    resource_id: &str,
) -> Result<()> {
    if selector.is_empty() {
        return Ok(());
    }
    let pods = ctx
        .service_provider
        .get_kubernetes_pod_client(ctx.get_kubernetes_config()?)
        .await?
        .list_pods(namespace, Some(label_selector(selector)), None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to inspect the pods of '{resource_id}'"),
            resource_id: Some(resource_id.to_string()),
        })?;
    match root_image_blocked(&pods.items, template, resource_id) {
        Some(message) => Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: resource_id.to_string(),
            message,
        })),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pod_blocked_by_run_as_non_root_names_the_opt_out() {
        use k8s_openapi::api::core::v1::{
            Container, ContainerState, ContainerStateWaiting, ContainerStatus, PodSpec, PodStatus,
        };
        let template = |image: &str| PodTemplateSpec {
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "api".to_string(),
                    image: Some(image.to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let pod = |image: &str, reason: &str, message: &str| Pod {
            spec: template(image).spec,
            status: Some(PodStatus {
                container_statuses: Some(vec![ContainerStatus {
                    name: "api".to_string(),
                    state: Some(ContainerState {
                        waiting: Some(ContainerStateWaiting {
                            reason: Some(reason.to_string()),
                            message: Some(message.to_string()),
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let root = "container has runAsNonRoot and image will run as root (pod: \"api-1\", container: api)";
        let blocked = pod("app:v2", "CreateContainerConfigError", root);
        let message = root_image_blocked(&[blocked], &template("app:v2"), "api").expect("blocked");
        assert!(message.contains("unrestricted"), "{message}");
        let pulling = pod("app:v2", "ImagePullBackOff", "back-off pulling image");
        assert!(root_image_blocked(&[pulling], &template("app:v2"), "api").is_none());

        // A corrective release: the stuck pod belongs to the old template, or is being deleted.
        let old = pod("app:v1", "CreateContainerConfigError", root);
        assert!(root_image_blocked(&[old], &template("app:v2"), "api").is_none());
        let mut deleting = pod("app:v2", "CreateContainerConfigError", root);
        deleting.metadata.deletion_timestamp = Some(
            k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(k8s_openapi::chrono::Utc::now()),
        );
        assert!(root_image_blocked(&[deleting], &template("app:v2"), "api").is_none());
    }

    #[test]
    fn absent_security_is_restricted() {
        let security = pod_security("api", None, &[]).unwrap();
        let pod = security.pod.unwrap();
        assert_eq!(pod.run_as_non_root, Some(true));
        assert_eq!(pod.fs_group, Some(65532));
        assert_eq!(pod.seccomp_profile.unwrap().type_, "RuntimeDefault");
        let container = security.container.unwrap();
        assert_eq!(container.allow_privilege_escalation, Some(false));
        assert_eq!(container.read_only_root_filesystem, Some(true));
        assert_eq!(
            container.capabilities.unwrap().drop,
            Some(vec!["ALL".to_string()])
        );
        assert_eq!(security.mounts[0].mount_path, "/tmp");
        assert!(security.volumes[0].empty_dir.is_some());
    }

    #[test]
    fn opt_out_applies_nothing() {
        let security = ContainerSecurity {
            profile: alien_core::ContainerSecurityProfile::Unrestricted,
            run_as_user: None,
            run_as_group: None,
            read_only_root_filesystem: None,
            writable_paths: Vec::new(),
        };
        let applied = pod_security("api", Some(&security), &[]).unwrap();
        assert!(applied.pod.is_none() && applied.container.is_none());
        assert!(applied.volumes.is_empty() && applied.mounts.is_empty());
    }

    #[test]
    fn root_uid_names_the_opt_out() {
        let security = ContainerSecurity {
            profile: alien_core::ContainerSecurityProfile::Restricted,
            run_as_user: Some(0),
            run_as_group: None,
            read_only_root_filesystem: None,
            writable_paths: Vec::new(),
        };
        let error = pod_security("api", Some(&security), &[]).unwrap_err();
        assert!(error.message.contains("unrestricted"), "{}", error.message);
    }

    #[test]
    fn declared_paths_are_writable_and_not_duplicated() {
        let security = ContainerSecurity {
            profile: alien_core::ContainerSecurityProfile::Restricted,
            run_as_user: None,
            run_as_group: None,
            read_only_root_filesystem: None,
            writable_paths: vec!["/var/cache".to_string(), "/tmp".to_string()],
        };
        let applied = pod_security("api", Some(&security), &[]).unwrap();
        let paths: Vec<_> = applied
            .mounts
            .iter()
            .map(|m| m.mount_path.as_str())
            .collect();
        assert_eq!(paths, ["/tmp", "/var/cache"]);
    }
}
