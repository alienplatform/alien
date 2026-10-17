#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output, Stdio};

use alien_core::{deployer_secret_location, DeployerSecretLocationContext, VaultBinding};
use tempfile::TempDir;

const VALUE: &str = "  fake-test-value-1566 '$()\\\"\t ";

struct Shell {
    directory: TempDir,
    executable: &'static str,
}

impl Shell {
    fn new(shell: &'static str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        // The executables enforce each CLI's input transport and capture what it receives.
        let executable = directory.path().join("cli");
        fs::write(
            &executable,
            r#"#!/bin/bash
printf '%s\0' "$@" >> "$CAPTURE/args"
case "${0##*/} $1 $2" in
  'aws sts get-caller-identity') printf '%s\n' "$ACCOUNT"; exit 0;;
  'gcloud secrets describe') exit 0;;
esac
for ((i=1; i<=$#; i++)); do
  arg="${!i}"
  if [[ "$arg" == --value ]]; then
    ((i++)); value="${!i}"
    if [[ "$value" != file:///dev/stdin ]]; then printf '%s' "$value" > "$CAPTURE/value"; exit 0; fi
  elif [[ "$arg" == --from-literal=value=* ]]; then
    printf '%s' "${arg#--from-literal=value=}"; exit 0
  fi
done
if [[ "${0##*/}" == alien && "${!#}" != --stdin ]]; then printf '%s' "${!#}" > "$CAPTURE/value"; exit 0; fi
case "${0##*/}" in
  aws) [[ " $* " == *' --value file:///dev/stdin '* ]] || exit 40;;
  az) [[ " $* " == *' --file /dev/stdin '* ]] || exit 41;;
  gcloud) [[ " $* " == *' --data-file=- '* ]] || exit 42;;
  kubectl)
    if [[ "$1" == create ]]; then
      [[ " $* " == *' --from-file=value=/dev/stdin '* ]] || exit 43
      cat
      exit 0
    fi;;
  alien) [[ " $* " == *' --stdin '* ]] || exit 44;;
esac
cat > "$CAPTURE/value"
"#,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        for name in ["aws", "gcloud", "az", "kubectl", "alien"] {
            std::os::unix::fs::symlink(&executable, directory.path().join(name)).unwrap();
        }
        Self {
            directory,
            executable: shell,
        }
    }

    fn run(&self, command: &str, account: &str, input: &str) -> Output {
        let mut child = Command::new(self.executable)
            .args(["-x", "-c", command])
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.directory.path().display()),
            )
            .env("CAPTURE", self.directory.path())
            .env("ACCOUNT", account)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

fn context() -> DeployerSecretLocationContext {
    DeployerSecretLocationContext {
        secret_label: Some("Key's $(printf injected)".to_string()),
        aws_account_id: Some("123456789012".to_string()),
        ..Default::default()
    }
}

#[test]
fn hidden_prompts_deliver_exact_bytes_without_exposing_values_in_arguments_or_output() {
    for executable in ["bash", "zsh"] {
        for value in [VALUE, "fake-test café 漢字"] {
            for binding in [
                VaultBinding::parameter_store("test-secrets"),
                VaultBinding::secret_manager("test-secrets"),
                VaultBinding::key_vault("testvault"),
                VaultBinding::kubernetes_secret("default", "test-secrets"),
                VaultBinding::local("secrets", "/tmp/unused"),
            ] {
                let shell = Shell::new(executable);
                let location = deployer_secret_location(&binding, "input-key", &context()).unwrap();
                let output =
                    shell.run(&location.cli_command, "123456789012", &format!("{value}\n"));
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    fs::read(shell.directory.path().join("value")).unwrap(),
                    value.as_bytes()
                );
                let args = fs::read(shell.directory.path().join("args")).unwrap();
                for bytes in [args, output.stdout, output.stderr.clone()] {
                    assert!(!String::from_utf8_lossy(&bytes).contains(value));
                }
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("Key's $(printf injected): ")
                );
            }
        }
    }
}

#[test]
fn cancelled_prompt_does_not_write_an_empty_secret() {
    for executable in ["bash", "zsh"] {
        let shell = Shell::new(executable);
        let location = deployer_secret_location(
            &VaultBinding::local("secrets", "/tmp/unused"),
            "input-key",
            &context(),
        )
        .unwrap();
        let output = shell.run(&location.cli_command, "123456789012", "");
        assert!(!output.status.success());
        assert!(!shell.directory.path().join("value").exists());
    }
}

#[test]
fn inline_variants_preserve_shell_quoted_values() {
    for executable in ["bash", "zsh"] {
        for binding in [
            VaultBinding::parameter_store("test-secrets"),
            VaultBinding::secret_manager("test-secrets"),
            VaultBinding::key_vault("testvault"),
            VaultBinding::kubernetes_secret("default", "test-secrets"),
            VaultBinding::local("secrets", "/tmp/unused"),
        ] {
            let shell = Shell::new(executable);
            let location = deployer_secret_location(&binding, "input-key", &context()).unwrap();
            let value = "fake-test '$ \" café\n";
            let quoted = format!("'{}'", value.replace('\'', "'\"'\"'"));
            let command = location
                .commands
                .unwrap()
                .unix
                .inline
                .replace("'<VALUE>'", &quoted);
            let output = shell.run(&command, "123456789012", "");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                fs::read(shell.directory.path().join("value")).unwrap(),
                value.as_bytes()
            );
        }
    }
}

#[test]
fn inline_local_vault_keeps_flag_like_names() {
    for executable in ["bash", "zsh"] {
        let shell = Shell::new(executable);
        let location = deployer_secret_location(
            &VaultBinding::local("app--stdin", "/tmp/unused"),
            "key--stdin",
            &DeployerSecretLocationContext {
                deployment_name: Some("deployment--stdin".to_string()),
                ..context()
            },
        )
        .unwrap();
        let command = location
            .commands
            .unwrap()
            .unix
            .inline
            .replace("'<VALUE>'", "'fake-value'");
        assert!(shell.run(&command, "123456789012", "").status.success());
        let arguments = fs::read(shell.directory.path().join("args")).unwrap();
        let arguments: Vec<&[u8]> = arguments.split(|byte| *byte == 0).collect();
        for expected in ["app--stdin", "key--stdin", "deployment--stdin"] {
            assert!(arguments.contains(&expected.as_bytes()));
        }
        assert_eq!(
            fs::read(shell.directory.path().join("value")).unwrap(),
            b"fake-value"
        );
    }
}
