//! Signed statements for the gate's end-to-end test.
//!
//! `gate-fixtures <out-dir> <project> <deployment> <inputs> <image-a> <image-b>`
//! writes an anchor, a signer list and releases of one function workload,
//! `api`, signed by fresh keys. See `scripts/gate-e2e/aws.py`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use alien_release_trust::envelope::Envelope;
use alien_release_trust::release::{
    Bounds, Configurable, FunctionSettings, ProvidedEnv, Runtime, WorkloadKind,
};
use alien_release_trust::{
    fingerprint, Anchor, Kind, ReleaseBody, Role, SessionApproval, SignedStatement, Signer,
    SignerKind, SignerListBody, SoftwareKey, Workload,
};
use serde::Serialize;

fn statement<B: Serialize>(project: &str, kind: Kind, sequence: u64, body: B) -> SignedStatement {
    SignedStatement::new(
        &Envelope {
            kind: kind.as_str().to_string(),
            project: Some(project.to_string()),
            deployment: None,
            sequence,
            not_after: None,
            signer_list_version: 1,
            body,
        }
        .to_payload()
        .expect("the envelope serializes"),
    )
}

fn release(image: &str, marker: &str, deployer_env: BTreeMap<String, String>) -> ReleaseBody {
    ReleaseBody {
        platform: "aws".to_string(),
        workloads: vec![Workload {
            name: "api".to_string(),
            kind: WorkloadKind::Function,
            image: image.to_string(),
            command: None,
            env: BTreeMap::from([("WORKLOAD_MARKER".to_string(), marker.to_string())]),
            provided_env: ProvidedEnv {
                names: Vec::new(),
                prefixes: vec!["ALIEN_".to_string()],
            },
            configurable_env: Vec::new(),
            deployer_env,
            secrets: Vec::new(),
            runtime: Runtime::default(),
            volume_mount_path: None,
            identity: Some("execution".to_string()),
            bounds: Bounds::default(),
            function: Some(FunctionSettings {
                memory_mb: 256,
                timeout_seconds: 20,
                architecture: None,
            }),
        }],
        configurable: Configurable::default(),
        components: BTreeMap::new(),
        remote_access: Vec::new(),
        dynamic: None,
        plugins: BTreeMap::new(),
        signed_commands: Vec::new(),
        stack: None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, out, project, deployment, inputs, image_a, image_b] = args.as_slice() else {
        return Err(
            "usage: gate-fixtures <out-dir> <project> <deployment> <inputs> <image-a> <image-b>"
                .into(),
        );
    };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out)?;
    let write = |name: &str, value: &dyn erased::Json| -> Result<(), Box<dyn std::error::Error>> {
        std::fs::write(out.join(name), value.json()?)?;
        Ok(())
    };

    let ci = SoftwareKey::generate()?;
    let attacker = SoftwareKey::generate()?;
    let components = SoftwareKey::generate()?;
    let list = statement(
        project,
        Kind::SignerList,
        1,
        SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers: vec![Signer {
                id: "ci".to_string(),
                key: ci.public_key().as_str().to_string(),
                kind: SignerKind::Ci,
                roles: vec![Role::Admin, Role::Release],
                label: None,
            }],
            deny: Vec::new(),
        },
    )
    .sign_with("ci", &ci)?;
    let anchor = Anchor {
        project: project.clone(),
        deployment: deployment.clone(),
        protected: true,
        signing: fingerprint(&list.open()?.digest),
        components: components.public_key().as_str().to_string(),
        session_approval: SessionApproval::default(),
        inputs: Some(inputs.clone()),
        resource_prefix: None,
    };
    let none = BTreeMap::new;
    write("anchor.json", &anchor)?;
    write("signer-list.json", &list)?;
    for (name, sequence, image, signer, key, deployer_env) in [
        ("release-10", 10, image_a, "ci", &ci, none()),
        ("release-20", 20, image_b, "ci", &ci, none()),
        ("release-30-attacker", 30, image_a, "ci", &attacker, none()),
        (
            "release-40-deployer-input",
            40,
            image_b,
            "ci",
            &ci,
            BTreeMap::from([("DEPLOYER_REGION".to_string(), "region".to_string())]),
        ),
    ] {
        let marker = name.to_string();
        let signed = statement(
            project,
            Kind::Release,
            sequence,
            release(image, &marker, deployer_env),
        )
        .sign_with(signer, key)?;
        write(&format!("{name}.json"), &signed)?;
    }
    println!("{}", out.display());
    Ok(())
}

mod erased {
    pub trait Json {
        fn json(&self) -> Result<Vec<u8>, serde_json::Error>;
    }
    impl<T: serde::Serialize> Json for T {
        fn json(&self) -> Result<Vec<u8>, serde_json::Error> {
            serde_json::to_vec_pretty(self)
        }
    }
}
