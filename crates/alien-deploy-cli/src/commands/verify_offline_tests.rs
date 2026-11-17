//! `verify --offline` on local files: a rendered template passes, one changed byte fails, a
//! renderer below the signed minimum is refused, and nothing reads a cloud.

use super::*;
use crate::commands::verify::tests::{
    fingerprint, genesis, release_with_minimum, render_fixture,
};
use alien_cli_common::setup_templates::RENDERER_VERSION;

struct Files {
    dir: tempfile::TempDir,
}

impl Files {
    /// The signed statements and a CloudFormation template rendered with `renderer`.
    fn new(template: &str, minimum: &str) -> (Self, OfflineInputs) {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, content: String| {
            let path = dir.path().join(name);
            std::fs::write(&path, content).unwrap();
            path
        };
        let template = write("template.yaml", template.to_string());
        let list = write("list-1.json", serde_json::to_string(&genesis()).unwrap());
        let release = write(
            "2026-11-04.3.json",
            serde_json::to_string(&release_with_minimum(
                "production",
                20,
                minimum,
                ("github-acme-app", 2),
            ))
            .unwrap(),
        );
        let inputs = OfflineInputs {
            setup: OfflineSetup::Cloudformation,
            template,
            fingerprint: fingerprint(),
            channel: "production".to_string(),
            signer_lists: vec![list],
            releases: vec![release],
        };
        (Self { dir }, inputs)
    }
}

fn find<'a>(lines: &'a [Line], label: &str) -> &'a Line {
    lines
        .iter()
        .find(|line| line.label == label)
        .unwrap_or_else(|| panic!("no {label} line in\n{}", render(lines)))
}

fn rendered() -> String {
    String::from_utf8(render_fixture("cloudformation", "production", None).bytes).unwrap()
}

#[tokio::test]
async fn a_rendered_template_passes_and_shows_what_it_was_rendered_with() {
    let (_files, inputs) = Files::new(&rendered(), RENDERER_VERSION);
    let lines = check_offline(&inputs).await.unwrap();
    assert!(
        lines.iter().all(|line| line.mark != Mark::Fail),
        "{}",
        render(&lines)
    );
    assert_eq!(find(&lines, "Template").mark, Mark::Pass);
    assert!(find(&lines, "Renderer").text.contains(RENDERER_VERSION));
    assert!(find(&lines, "Managing principals")
        .text
        .contains("vendor-manager"));
    assert_eq!(find(&lines, "External id").text, "required");
}

#[tokio::test]
async fn a_template_with_one_changed_byte_fails_offline() {
    let original = rendered();
    let verifier = format!("verifier@sha256:{}", "5d".repeat(32));
    for (what, changed) in [
        ("a widened statement", original.replacen("sts:AssumeRole", "sts:*", 1)),
        (
            "another verifier image digest",
            original.replace(&verifier, &format!("verifier@sha256:{}", "ab".repeat(32))),
        ),
        (
            "another fingerprint",
            original.replace(&fingerprint(), &alien_signing::fingerprint(&"c".repeat(64))),
        ),
    ] {
        assert_ne!(changed, original, "{what}");
        let (_files, inputs) = Files::new(&changed, RENDERER_VERSION);
        let lines = check_offline(&inputs).await.unwrap();
        assert_eq!(find(&lines, "Template").mark, Mark::Fail, "{what}");
    }
}

#[tokio::test]
async fn a_renderer_below_the_signed_minimum_is_refused_offline() {
    let old = String::from_utf8(render_fixture("cloudformation", "production", Some("0.0.1")).bytes)
        .unwrap();
    let (_files, inputs) = Files::new(&old, RENDERER_VERSION);
    let lines = check_offline(&inputs).await.unwrap();
    assert_eq!(find(&lines, "Renderer").mark, Mark::Fail, "{}", render(&lines));
}

#[tokio::test]
async fn a_release_signed_by_nobody_on_the_list_is_not_believed_offline() {
    let (_files, mut inputs) = Files::new(&rendered(), RENDERER_VERSION);
    // A fingerprint that isn't the list's: the list doesn't verify, so nothing is checked.
    inputs.fingerprint = alien_signing::fingerprint(&"d".repeat(64));
    assert!(check_offline(&inputs).await.is_err());
}
