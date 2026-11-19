//! The suspend and resume hooks of a sandbox with mounts, over a real socket.
//!
//! The hooks take the file systems down and bring them back, so the code the agent runs (which
//! shares the guest's loopback) must not be able to call them.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use alien_sandbox_agent::exec::ExecIdentity;
use alien_sandbox_agent::jobs::JobRegistry;
use alien_sandbox_agent::mounts::{
    HelperRequest, MountBackend, MountCredentials, MountSource, Mounts,
};
use alien_sandbox_agent::server::{hook_path, router, AgentAuthorization, AgentState};
use tokio::net::TcpListener;

#[derive(Default)]
struct Recorder(Mutex<Vec<String>>);

struct Shared(Arc<Recorder>);

impl MountBackend for Shared {
    fn request(&self, request: &HelperRequest) -> Result<(), String> {
        self.0 .0.lock().unwrap().push(format!("{request:?}"));
        Ok(())
    }
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn the_code_the_agent_runs_cannot_suspend_or_resume_the_mounts() {
    let dir = tempfile::tempdir().unwrap();
    let recorder = Arc::new(Recorder::default());
    let declared: Vec<alien_core::SandboxMount> =
        serde_json::from_str(r#"[{"type":"s3files","path":"/workspace"}]"#).unwrap();
    alien_sandbox_agent::mounts::install(Mounts::new(
        declared,
        60000,
        60000,
        dir.path().join("credentials"),
        Box::new(Shared(recorder.clone())),
    ));
    let mounts = alien_sandbox_agent::mounts::global().expect("installed");
    mounts.advance(
        &[(
            "/workspace".to_string(),
            MountSource {
                file_system_id: "fs-1".into(),
                access_point_id: "fsap-1".into(),
                region: "us-east-1".into(),
                availability_zone_id: Some("use1-az1".into()),
                mount_target_ip: None,
                credentials: MountCredentials {
                    access_key_id: "a".into(),
                    secret_access_key: "b".into(),
                    session_token: "c".into(),
                    expiration: "2030-01-01T00:00:00Z".into(),
                },
            },
        )]
        .into(),
    );
    assert!(mounts.ready());
    let before = recorder.0.lock().unwrap().len();

    let root = dir.path().canonicalize().unwrap();
    let state = Arc::new(AgentState {
        image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
        gateway_egress: None,
        session_root: root,
        authorization: AgentAuthorization::Transport,
        // This process stands in for the code the agent runs: its own uid is the exec uid.
        exec_identity: ExecIdentity {
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
        },
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
        interactive: Default::default(),
        preview: Default::default(),
        agent_binary: std::path::PathBuf::from(env!("CARGO_BIN_EXE_alien-sandbox-agent")),
    });
    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            router(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    for hook in ["suspend", "resume"] {
        let response = reqwest::Client::new()
            .post(format!("http://{address}{}", hook_path(hook)))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403, "{hook}");
    }
    assert!(mounts.ready(), "the mounts are still in place");
    assert_eq!(
        recorder.0.lock().unwrap().len(),
        before,
        "the helper was not asked for anything"
    );
}
