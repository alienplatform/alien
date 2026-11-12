//! The release check the deploy CLI runs as Helm's post-renderer, on charts the real Helm
//! modifier pass makes: each template the `steered` guest returns goes through the pass, the
//! chart renders with the local Helm as an install and as an upgrade, and the check reads what
//! Helm hands a post-renderer. An object that repeats one of Alien's is refused in a mode it
//! renders in; an object of the extension's own passes both.

use alien_core::{ResourceLifecycle, Stack, StackSettings, Storage};
use alien_extension_sdk::testing::{build_module, load_module, HookModule};
use alien_helm::{generate_helm_chart, HelmOptions};
use alien_infra::{DeploymentRegistries, HostRegistrations, LoadedExtensions};
use indexmap::IndexMap;
use serde_json::json;
use std::path::{Path, PathBuf};

fn steered() -> HookModule {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("extension-sdk-guests");
    let wasm = build_module(&workspace, "steered-hooks", &target_dir)
        .unwrap_or_else(|error| panic!("{error}"));
    load_module(&wasm).unwrap_or_else(|error| panic!("{error}"))
}

/// The chart the pass makes when the `steered` extension's Helm modifier returns `templates`.
fn chart(module: &HookModule, templates: &[(&str, &str)]) -> IndexMap<String, String> {
    let mut stack = Stack::new("my-app".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.extensions = vec![serde_json::from_value(json!({
        "name": "steered", "version": "1.0.0", "digest": "sha256:0", "resourceTypes": {}
    }))
    .unwrap()];
    let loaded = LoadedExtensions::default().with_hooks("steered", module.clone());
    let registries = DeploymentRegistries::for_stack(&stack, &HostRegistrations::NONE, &loaded);
    let mut chart = generate_helm_chart(
        &stack,
        HelmOptions {
            registry: &registries.helm,
            stack_settings: StackSettings::default(),
            chart_name: stack.id().to_string(),
        },
    )
    .unwrap();
    let templates: IndexMap<&str, &str> = templates.iter().copied().collect();
    chart.files.insert(
        "steer.json".into(),
        json!({ "templates": templates }).to_string(),
    );
    registries.modifiers.modify_helm(&mut chart).unwrap();
    chart.files.shift_remove("steer.json");
    chart.files
}

/// A new, empty directory for one render.
fn work_dir() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("release-check")
        .join(
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .to_string(),
        );
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// A post-renderer plugin that keeps what Helm hands it in `seen.yaml`.
fn keep_plugin(directory: &Path) -> PathBuf {
    let script = directory.join("keep.sh");
    let seen = directory.join("seen.yaml");
    std::fs::write(&script, format!("#!/bin/sh\ntee {}\n", seen.display())).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(directory.join("plugins/keep")).unwrap();
    std::fs::write(
        directory.join("plugins/keep/plugin.yaml"),
        format!(
            "apiVersion: v1\ntype: postrenderer/v1\nname: keep\nversion: 0.1.0\nruntime: subprocess\nruntimeConfig:\n  platformCommand:\n    - command: {}\n",
            script.display()
        ),
    )
    .unwrap();
    seen
}

/// The release check's verdict on `files` rendered in namespace `ns`, as an upgrade or not.
fn verdict(files: &IndexMap<String, String>, upgrade: bool) -> Result<(), String> {
    let work = work_dir();
    let chart = work.as_path().join("chart");
    for (path, contents) in files {
        let path = chart.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let seen = keep_plugin(work.as_path());
    let mut helm = std::process::Command::new("helm");
    helm.args(["template", "rel"])
        .arg(&chart)
        .args(["--namespace", "ns", "--post-renderer", "keep"])
        .env("HELM_PLUGINS", work.as_path().join("plugins"));
    if upgrade {
        // What an upgrade reads from the installed release with `lookup`.
        helm.args([
            "--is-upgrade",
            "--set-string",
            "logCollector.scope.deploymentLabelValue=dep_1",
            "--set-string",
            "runtime.encryption.existingSecret.name=enc",
        ]);
    }
    let output = helm.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = std::fs::read_to_string(seen).unwrap();
    let objects = alien_helm::post_rendered_objects(&manifest).map_err(|error| error.message)?;
    alien_helm::refuse_repeated_objects(&objects, &["steered"]).map_err(|error| error.message)
}

#[cfg(unix)]
#[test]
fn an_object_that_repeats_one_of_alien_s_is_refused_however_the_template_gates_it() {
    let module = steered();
    let alien = chart(&module, &[]);
    assert_eq!(verdict(&alien, false), Ok(()));
    assert_eq!(verdict(&alien, true), Ok(()));
    let rendered = {
        let work = work_dir();
        let chart = work.as_path().join("chart");
        for (path, contents) in &alien {
            let path = chart.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        let output = std::process::Command::new("helm")
            .args(["template", "rel"])
            .arg(&chart)
            .args(["--namespace", "ns"])
            .output()
            .unwrap();
        alien_helm::rendered_objects(&String::from_utf8(output.stdout).unwrap()).unwrap()
    };
    let name = |kind: &str| {
        rendered
            .iter()
            .find(|object| object.kind == kind)
            .unwrap_or_else(|| panic!("Alien's chart renders no {kind}"))
            .name
            .clone()
    };
    let (secret, cluster_role) = (name("Secret"), name("ClusterRole"));
    let with_names = |body: &str| {
        body.replace("SECRET", &secret)
            .replace("CROLE", &cluster_role)
    };
    let gated = |gate: &str| {
        with_names(&format!(
            "{{{{- {gate} }}}}\napiVersion: v1\nkind: Secret\nmetadata:\n  name: SECRET\n{{{{- end }}}}\n"
        ))
    };

    let own = chart(
        &module,
        &[(
            "zz-own.yaml",
            "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: steered-config\ndata:\n  op: <<\n  ops: [<<]\n",
        )],
    );
    assert_eq!(
        verdict(&own, false),
        Ok(()),
        "an object of its own passes, with << as a value"
    );
    assert_eq!(
        verdict(&own, true),
        Ok(()),
        "an object of its own passes, with << as a value"
    );

    for (case, template) in [
        ("a repeat", with_names("apiVersion: v1\nkind: Secret\nmetadata:\n  name: SECRET\n")),
        ("the release's namespace", with_names("apiVersion: v1\nkind: Secret\nmetadata:\n  name: SECRET\n  namespace: ns\n")),
        ("an empty namespace", with_names("apiVersion: v1\nkind: Secret\nmetadata:\n  name: SECRET\n  namespace: \"\"\n")),
        ("another namespace", with_names("apiVersion: v1\nkind: Secret\nmetadata:\n  name: SECRET\n  namespace: steered\n")),
        ("a quoted name", with_names("apiVersion: v1\nkind: Secret\nmetadata:\n  name: \"SECRET\"\n")),
        ("IsUpgrade", gated("if .Release.IsUpgrade")),
        ("index", gated("if index .Release \"IsUpgrade\"")),
        ("get", gated("if get .Release \"IsUpgrade\"")),
        ("toJson", gated("if (fromJson (toJson .Release)).IsUpgrade")),
        ("lookup", gated("if not (lookup \"v1\" \"Namespace\" \"\" \"kube-system\")")),
        ("a cluster-scoped kind in a namespace", with_names("apiVersion: rbac.authorization.k8s.io/v1\nkind: ClusterRole\nmetadata:\n  name: CROLE\n  namespace: steered\nrules: []\n")),
        ("a cluster-scoped kind", with_names("apiVersion: rbac.authorization.k8s.io/v1\nkind: ClusterRole\nmetadata:\n  name: CROLE\nrules: []\n")),
        ("a List item", with_names("apiVersion: v1\nkind: List\nitems:\n- apiVersion: v1\n  kind: Secret\n  metadata:\n    name: SECRET\n")),
        ("a hook", with_names("apiVersion: v1\nkind: Secret\nmetadata:\n  name: SECRET\n  annotations:\n    helm.sh/hook: pre-install,pre-upgrade\n")),
        ("a spoofed source comment", with_names("# Source: my-app/templates/secret.yaml\napiVersion: v1\nkind: Secret\nmetadata:\n  name: SECRET\n")),
    ] {
        let files = chart(&module, &[("zz-case.yaml", template.as_str())]);
        let refusals: Vec<String> = [false, true]
            .into_iter()
            .filter_map(|upgrade| verdict(&files, upgrade).err())
            .collect();
        assert!(
            refusals
                .iter()
                .any(|refusal| refusal.starts_with("extension 'steered' renders ")),
            "{case}: {refusals:?}"
        );
    }

    // Kubernetes' decoder reads a tagged or merged node as Alien's object, so any tag or merge
    // key is refused.
    for (case, template) in [
        ("a merge key", with_names("apiVersion: v1\nkind: Secret\nmetadata:\n  labels: &m\n    name: SECRET\n  <<: *m\n")),
        ("a binary name", with_names("apiVersion: v1\nkind: Secret\nmetadata:\n  name: !!binary {{ \"SECRET\" | b64enc }}\n")),
        ("a tagged metadata key", with_names("apiVersion: v1\nkind: Secret\n!x metadata:\n  name: SECRET\n")),
        ("a tagged kind key", with_names("apiVersion: v1\n!x kind: Secret\nmetadata:\n  name: SECRET\n")),
    ] {
        let files = chart(&module, &[("zz-case.yaml", template.as_str())]);
        for upgrade in [false, true] {
            let refusal = verdict(&files, upgrade).unwrap_err();
            assert!(
                refusal.starts_with("Helm rendered a document as YAML with ")
                    && refusal.ends_with(", which a chart may not use"),
                "{case}: {refusal}"
            );
        }
    }
}
