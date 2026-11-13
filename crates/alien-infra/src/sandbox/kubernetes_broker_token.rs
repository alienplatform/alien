//! The token a workload presents to the session broker.
//!
//! A ServiceAccount token the kubelet mounts by default is issued for the API server, so whoever
//! receives one holds a credential for the API server as well. The broker reviews every token for
//! its own audience instead, and a workload that links a sandbox gets a token projected for that
//! audience alone: short lived, rotated by the kubelet, and useless anywhere else.

use alien_core::{ResourceRef, Sandbox};
use k8s_openapi::api::core::v1::{
    PodSpec, ProjectedVolumeSource, ServiceAccountTokenProjection, Volume, VolumeMount,
    VolumeProjection,
};

/// Audience the broker reviews tokens for.
pub const BROKER_AUDIENCE: &str = "alien-sandbox-broker";

/// Volume name in a linking workload's pod.
const VOLUME: &str = "alien-sandbox-broker-token";

/// Where the token is mounted in every container of a linking pod.
const DIRECTORY: &str = "/var/run/secrets/alien.dev/sandbox-broker";

/// The file the kubelet keeps current, which the sandbox binding names as the token path.
pub const TOKEN_PATH: &str = "/var/run/secrets/alien.dev/sandbox-broker/token";

/// Seconds a projected token lives before the kubelet replaces it. The client reads the file on
/// every broker call, so a rotated token is picked up without a restart.
const EXPIRATION_SECONDS: i64 = 3600;

/// Whether any of `links` is a sandbox.
pub fn links_a_sandbox(links: &[ResourceRef]) -> bool {
    links
        .iter()
        .any(|link| link.resource_type == Sandbox::RESOURCE_TYPE)
}

/// Adds the projected token to a pod that links a sandbox: the volume once, and its mount in
/// every container.
pub fn project_into(spec: &mut PodSpec, links: &[ResourceRef]) {
    if !links_a_sandbox(links) {
        return;
    }
    let volumes = spec.volumes.get_or_insert_default();
    if volumes.iter().any(|volume| volume.name == VOLUME) {
        return;
    }
    volumes.push(Volume {
        name: VOLUME.to_string(),
        projected: Some(ProjectedVolumeSource {
            sources: Some(vec![VolumeProjection {
                service_account_token: Some(ServiceAccountTokenProjection {
                    audience: Some(BROKER_AUDIENCE.to_string()),
                    expiration_seconds: Some(EXPIRATION_SECONDS),
                    path: "token".to_string(),
                }),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    });
    for container in spec
        .containers
        .iter_mut()
        .chain(spec.init_containers.iter_mut().flatten())
    {
        container
            .volume_mounts
            .get_or_insert_default()
            .push(VolumeMount {
                name: VOLUME.to_string(),
                mount_path: DIRECTORY.to_string(),
                read_only: Some(true),
                ..Default::default()
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::Container;

    fn pod() -> PodSpec {
        PodSpec {
            containers: vec![Container {
                name: "app".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_pod_that_links_a_sandbox_gets_a_token_projected_for_the_broker_alone() {
        let mut spec = pod();
        project_into(
            &mut spec,
            &[ResourceRef::new(Sandbox::RESOURCE_TYPE, "agent")],
        );
        let projection = spec.volumes.as_ref().unwrap()[0]
            .projected
            .as_ref()
            .unwrap()
            .sources
            .as_ref()
            .unwrap()[0]
            .service_account_token
            .as_ref()
            .unwrap();
        assert_eq!(projection.audience.as_deref(), Some(BROKER_AUDIENCE));
        assert_eq!(projection.expiration_seconds, Some(EXPIRATION_SECONDS));
        let mount = &spec.containers[0].volume_mounts.as_ref().unwrap()[0];
        assert_eq!(
            format!("{}/{}", mount.mount_path, projection.path),
            TOKEN_PATH
        );
        // Applying it twice does not duplicate the volume or the mount.
        project_into(
            &mut spec,
            &[ResourceRef::new(Sandbox::RESOURCE_TYPE, "agent")],
        );
        assert_eq!(spec.volumes.as_ref().unwrap().len(), 1);
        assert_eq!(spec.containers[0].volume_mounts.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn a_pod_that_links_no_sandbox_gets_nothing() {
        let mut spec = pod();
        project_into(&mut spec, &[]);
        assert!(spec.volumes.is_none());
        assert!(spec.containers[0].volume_mounts.is_none());
    }
}
