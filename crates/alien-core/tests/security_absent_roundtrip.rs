//! A release without a `security` field keeps it absent when it is read and
//! written again: the manager and the Operator both re-serialize releases, and
//! a field they added would restrict workloads the release never asked to.

use alien_core::{
    Container, ContainerCode, ContainerSecurity, ContainerSecurityProfile, Daemon, DaemonCode,
    Resource, ResourceLifecycle, ResourceSpec, Stack, Worker, WorkerCode,
};

fn stack(security: Option<ContainerSecurity>) -> Stack {
    let mut container = Container::new("api".to_string())
        .code(ContainerCode::Image {
            image: "example/api:1".to_string(),
        })
        .permissions("execution".to_string())
        .cpu(ResourceSpec {
            min: "0.5".into(),
            desired: "0.5".into(),
        })
        .memory(ResourceSpec {
            min: "256Mi".into(),
            desired: "256Mi".into(),
        })
        .build();
    let mut daemon = Daemon::new("agent".to_string())
        .code(DaemonCode::Image {
            image: "example/agent:1".to_string(),
        })
        .permissions("execution".to_string())
        .build();
    let mut worker = Worker::new("fn".to_string())
        .code(WorkerCode::Image {
            image: "example/fn:1".to_string(),
        })
        .permissions("execution".to_string())
        .build();
    container.security = security.clone();
    daemon.security = security.clone();
    worker.security = security;
    Stack::new("app".to_string())
        .add(container, ResourceLifecycle::Live)
        .add(daemon, ResourceLifecycle::Live)
        .add(worker, ResourceLifecycle::Live)
        .build()
}

fn security_of(stack: &Stack) -> Vec<Option<ContainerSecurity>> {
    stack
        .resources()
        .map(|(_, entry)| {
            let config: &Resource = &entry.config;
            if let Some(c) = config.downcast_ref::<Container>() {
                c.security.clone()
            } else if let Some(d) = config.downcast_ref::<Daemon>() {
                d.security.clone()
            } else {
                config.downcast_ref::<Worker>().unwrap().security.clone()
            }
        })
        .collect()
}

#[test]
fn an_absent_security_field_stays_absent_through_a_round_trip() {
    let written = serde_json::to_value(stack(None)).unwrap();
    assert!(!written.to_string().contains("security"), "{written}");
    let read: Stack = serde_json::from_value(written.clone()).unwrap();
    assert_eq!(security_of(&read), vec![None, None, None]);
    assert_eq!(serde_json::to_value(&read).unwrap(), written);
}

#[test]
fn a_declared_profile_survives_a_round_trip() {
    for profile in [
        ContainerSecurityProfile::Restricted,
        ContainerSecurityProfile::Unrestricted,
    ] {
        let declared = ContainerSecurity {
            profile,
            run_as_user: Some(1000),
            run_as_group: Some(1000),
            read_only_root_filesystem: None,
            writable_paths: vec!["/cache".to_string()],
        };
        let written = serde_json::to_value(stack(Some(declared.clone()))).unwrap();
        let read: Stack = serde_json::from_value(written).unwrap();
        assert_eq!(
            security_of(&read),
            vec![
                Some(declared.clone()),
                Some(declared.clone()),
                Some(declared)
            ]
        );
    }
}
