//! Device keys: one signing key per laptop, created inside its secure hardware.
//!
//! Each backend creates a P-256 key that can't leave the hardware and needs
//! the person at the keyboard for every signature: Touch ID or the login
//! password on macOS, a PIN on a TPM, a touch on a YubiKey. A machine with
//! none of them gets a software key in a private file, listed on the signer
//! list as `software` so admins can see it and refuse it.
//!
//! The hardware backends drive the platform's own tools (`swift` with
//! CryptoKit, `tpm2-tools`, `yubico-piv-tool`), so the CLI links no hardware
//! libraries and builds the same everywhere.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::{PublicKey, SignerKind, SoftwareKey};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

/// Environment variable holding the PIN for TPM and YubiKey backends, for
/// unattended use such as tests. Interactive use prompts instead.
pub const DEVICE_PIN_ENV: &str = "ALIEN_DEVICE_KEY_PIN";

/// Which secure hardware holds a device key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    /// The Secure Enclave of a Mac.
    SecureEnclave,
    /// A TPM 2.0, through `tpm2-tools`.
    Tpm,
    /// A YubiKey's PIV applet, slot 9c.
    Yubikey,
    /// A private file, for machines without secure hardware.
    Software,
}

impl Backend {
    /// Every backend, in the order `auto` tries them.
    pub const ALL: [Backend; 4] = [
        Backend::SecureEnclave,
        Backend::Tpm,
        Backend::Yubikey,
        Backend::Software,
    ];

    /// The kind the signer list records for a key on this backend.
    pub fn signer_kind(self) -> SignerKind {
        match self {
            Backend::Software => SignerKind::Software,
            _ => SignerKind::Device,
        }
    }

    /// Name for display and for the `--backend` flag.
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::SecureEnclave => "secure-enclave",
            Backend::Tpm => "tpm",
            Backend::Yubikey => "yubikey",
            Backend::Software => "software",
        }
    }

    /// Whether this machine has what the backend needs.
    pub fn available(self) -> bool {
        match self {
            Backend::SecureEnclave => cfg!(target_os = "macos") && which::which("swift").is_ok(),
            Backend::Tpm => {
                which::which("tpm2_create").is_ok()
                    && (std::env::var_os("TPM2TOOLS_TCTI").is_some()
                        || Path::new("/dev/tpmrm0").exists())
            }
            Backend::Yubikey => {
                which::which("yubico-piv-tool").is_ok()
                    && Command::new("yubico-piv-tool")
                        .args(["-a", "status"])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .is_ok_and(|status| status.success())
            }
            Backend::Software => true,
        }
    }
}

/// A device key, by where its handle lives on disk.
#[derive(Debug, Clone)]
pub struct DeviceKey {
    /// Backend holding the key.
    pub backend: Backend,
    /// Directory with the backend's handle files.
    pub dir: PathBuf,
    /// The public key.
    pub public_key: PublicKey,
}

impl DeviceKey {
    /// Create a key on `backend`, keeping its handle in `dir`.
    pub fn create(backend: Backend, dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .into_alien_error()
            .context(file_failed(dir, "create the device key directory"))?;
        restrict_dir(dir)?;
        let public_key = match backend {
            Backend::Software => {
                let key =
                    SoftwareKey::generate().context(signing_failed("generate a software key"))?;
                write_private(
                    &dir.join("key.pem"),
                    &key.to_pkcs8_pem()
                        .context(signing_failed("encode the key"))?,
                )?;
                key.public_key()
            }
            Backend::Tpm => tpm::create(dir)?,
            Backend::Yubikey => yubikey::create(dir)?,
            Backend::SecureEnclave => secure_enclave::create(dir)?,
        };
        std::fs::write(dir.join("public.txt"), public_key.as_str())
            .into_alien_error()
            .context(file_failed(dir, "write the public key"))?;
        Ok(Self {
            backend,
            dir: dir.to_path_buf(),
            public_key,
        })
    }

    /// Open the key created in `dir`.
    pub fn open(backend: Backend, dir: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(dir.join("public.txt"))
            .into_alien_error()
            .context(file_failed(dir, "read the device public key"))?;
        let public_key =
            PublicKey::parse(text.trim()).context(signing_failed("read the device public key"))?;
        Ok(Self {
            backend,
            dir: dir.to_path_buf(),
            public_key,
        })
    }

    /// Sign `message`; the hardware asks for the person's presence. The
    /// signature is checked against the public key before it is returned, so
    /// a backend fault never produces a signature enforcers will refuse.
    pub fn sign(&self, message: &[u8]) -> Result<String> {
        let signature = match self.backend {
            Backend::Software => {
                let pem = std::fs::read_to_string(self.dir.join("key.pem"))
                    .into_alien_error()
                    .context(file_failed(&self.dir, "read the software key"))?;
                SoftwareKey::from_pkcs8_pem(&pem)
                    .context(signing_failed("read the software key"))?
                    .sign(message)
            }
            Backend::Tpm => tpm::sign(&self.dir, message)?,
            Backend::Yubikey => yubikey::sign(&self.dir, message)?,
            Backend::SecureEnclave => secure_enclave::sign(&self.dir, message)?,
        };
        self.public_key
            .verify(message, &signature)
            .context(signing_failed(&format!(
                "check the {} signature",
                self.backend.as_str()
            )))?;
        Ok(signature)
    }
}

/// Fingerprint code people compare when an admin approves a device: the
/// first four hex digits of the key's SHA-256, upper case, such as `4F2A`.
pub fn approval_code(key: &PublicKey) -> String {
    key.fingerprint()
        .trim_start_matches("SHA256:")
        .chars()
        .take(4)
        .collect::<String>()
        .to_uppercase()
}

mod tpm {
    //! A TPM 2.0 key under the owner hierarchy's default primary, with a PIN.
    //! The primary is re-derived from the same template each time, so only
    //! the wrapped key blobs are kept on disk; they are useless without this
    //! machine's TPM.

    use super::*;

    const ATTRIBUTES: &str = "fixedtpm|fixedparent|sensitivedataorigin|userwithauth|sign";

    pub(super) fn create(dir: &Path) -> Result<PublicKey> {
        let pin = super::pin("Choose a PIN for the TPM signing key")?;
        primary(dir)?;
        tpm(
            Command::new("tpm2_create").args([
                "-Q",
                "-C",
                "primary.ctx",
                "-G",
                "ecc256:ecdsa-sha256",
                "-u",
                "key.pub",
                "-r",
                "key.priv",
                "-a",
                ATTRIBUTES,
                "-p",
                "file:-",
            ]),
            dir,
            Some(pin.as_bytes()),
            "create the TPM key",
        )?;
        load(dir)?;
        tpm(
            Command::new("tpm2_readpublic")
                .args(["-Q", "-c", "key.ctx", "-f", "der", "-o", "key.der"]),
            dir,
            None,
            "read the TPM public key",
        )?;
        cleanup(dir);
        public_from_der(&dir.join("key.der"))
    }

    pub(super) fn sign(dir: &Path, message: &[u8]) -> Result<String> {
        let pin = super::pin("TPM signing key PIN")?;
        primary(dir)?;
        load(dir)?;
        std::fs::write(dir.join("message.bin"), message)
            .into_alien_error()
            .context(file_failed(dir, "write the message to sign"))?;
        let result = tpm(
            Command::new("tpm2_sign").args([
                "-Q",
                "-c",
                "key.ctx",
                "-p",
                "file:-",
                "-g",
                "sha256",
                "-f",
                "plain",
                "-o",
                "signature.der",
                "message.bin",
            ]),
            dir,
            Some(pin.as_bytes()),
            "sign with the TPM key",
        );
        let signature = result.and_then(|()| {
            std::fs::read(dir.join("signature.der"))
                .into_alien_error()
                .context(file_failed(dir, "read the TPM signature"))
        });
        cleanup(dir);
        let _ = std::fs::remove_file(dir.join("message.bin"));
        let _ = std::fs::remove_file(dir.join("signature.der"));
        Ok(STANDARD.encode(signature?))
    }

    fn primary(dir: &Path) -> Result<()> {
        tpm(
            Command::new("tpm2_createprimary").args([
                "-Q",
                "-C",
                "o",
                "-g",
                "sha256",
                "-G",
                "ecc256",
                "-c",
                "primary.ctx",
            ]),
            dir,
            None,
            "derive the TPM primary key",
        )
    }

    fn load(dir: &Path) -> Result<()> {
        tpm(
            Command::new("tpm2_load").args([
                "-Q",
                "-C",
                "primary.ctx",
                "-u",
                "key.pub",
                "-r",
                "key.priv",
                "-c",
                "key.ctx",
            ]),
            dir,
            None,
            "load the TPM key",
        )
    }

    fn cleanup(dir: &Path) {
        // Transient contexts only; the wrapped key stays.
        let _ = std::fs::remove_file(dir.join("primary.ctx"));
        let _ = std::fs::remove_file(dir.join("key.ctx"));
    }

    /// Runs one tpm2-tools command, then flushes the transient objects it
    /// loaded. Without a resource manager (a TPM simulator, or `/dev/tpm0`)
    /// they stay loaded, and a TPM has only a few slots for them. The
    /// context files stay usable: later commands load them again.
    fn tpm(command: &mut Command, dir: &Path, input: Option<&[u8]>, action: &str) -> Result<()> {
        let result = run(command, dir, input, action);
        let flushed = run(
            Command::new("tpm2_flushcontext").arg("-t"),
            dir,
            None,
            "flush the TPM's transient objects",
        );
        result.and(flushed)
    }
}

mod yubikey {
    //! A key generated on a YubiKey in PIV slot 9c (digital signature), with
    //! touch required for every use.

    use super::*;

    pub(super) fn create(dir: &Path) -> Result<PublicKey> {
        run(
            Command::new("yubico-piv-tool").args([
                "-a",
                "generate",
                "-s",
                "9c",
                "-A",
                "ECCP256",
                "--touch-policy=always",
                "--pin-policy=once",
                "-o",
                "public.pem",
            ]),
            dir,
            None,
            "generate the YubiKey key",
        )?;
        let pem = std::fs::read_to_string(dir.join("public.pem"))
            .into_alien_error()
            .context(file_failed(dir, "read the YubiKey public key"))?;
        let der = pem
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect::<String>();
        PublicKey::parse(&format!("ecdsa-p256:{der}"))
            .context(signing_failed("read the YubiKey public key"))
    }

    pub(super) fn sign(dir: &Path, message: &[u8]) -> Result<String> {
        let pin = super::pin("YubiKey PIV PIN")?;
        std::fs::write(dir.join("message.bin"), message)
            .into_alien_error()
            .context(file_failed(dir, "write the message to sign"))?;
        eprintln!("Touch your YubiKey to sign.");
        let result = run(
            Command::new("yubico-piv-tool").args([
                "-a",
                "verify-pin",
                "--stdin-input",
                "-a",
                "sign",
                "-s",
                "9c",
                "-A",
                "ECCP256",
                "-H",
                "SHA256",
                "-i",
                "message.bin",
                "-o",
                "signature.der",
            ]),
            dir,
            Some(pin.as_bytes()),
            "sign with the YubiKey",
        );
        let signature = result.and_then(|()| {
            std::fs::read(dir.join("signature.der"))
                .into_alien_error()
                .context(file_failed(dir, "read the YubiKey signature"))
        });
        let _ = std::fs::remove_file(dir.join("message.bin"));
        let _ = std::fs::remove_file(dir.join("signature.der"));
        Ok(STANDARD.encode(signature?))
    }
}

mod secure_enclave {
    //! A CryptoKit Secure Enclave key that requires user presence (Touch ID
    //! or the login password) for every signature. CryptoKit returns the key
    //! as an opaque blob that only this Mac's Secure Enclave can use, so no
    //! keychain entitlement is needed.

    use super::*;

    const CREATE: &str = r#"
import CryptoKit
import Foundation
var error: Unmanaged<CFError>?
guard let access = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, [.privateKeyUsage, .userPresence], &error) else { exit(2) }
let key = try SecureEnclave.P256.Signing.PrivateKey(accessControl: access)
try key.dataRepresentation.write(to: URL(fileURLWithPath: "key.blob"))
FileHandle.standardOutput.write(key.publicKey.derRepresentation.base64EncodedData())
"#;

    const SIGN: &str = r#"
import CryptoKit
import Foundation
let blob = try Data(contentsOf: URL(fileURLWithPath: "key.blob"))
let key = try SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: blob)
let message = FileHandle.standardInput.readDataToEndOfFile()
let signature = try key.signature(for: message)
FileHandle.standardOutput.write(signature.derRepresentation.base64EncodedData())
"#;

    pub(super) fn create(dir: &Path) -> Result<PublicKey> {
        let output = swift(dir, CREATE, &[], "create the Secure Enclave key")?;
        PublicKey::parse(&format!("ecdsa-p256:{}", output.trim()))
            .context(signing_failed("read the Secure Enclave public key"))
    }

    pub(super) fn sign(dir: &Path, message: &[u8]) -> Result<String> {
        eprintln!("Confirm with Touch ID to sign.");
        Ok(
            swift(dir, SIGN, message, "sign with the Secure Enclave key")?
                .trim()
                .to_string(),
        )
    }

    fn swift(dir: &Path, script: &str, input: &[u8], action: &str) -> Result<String> {
        let path = dir.join("helper.swift");
        std::fs::write(&path, script)
            .into_alien_error()
            .context(file_failed(dir, "write the Secure Enclave helper"))?;
        let output = output(
            Command::new("swift").arg("helper.swift"),
            dir,
            Some(input),
            action,
        );
        let _ = std::fs::remove_file(path);
        output
    }
}

fn pin(prompt: &str) -> Result<String> {
    if let Ok(pin) = std::env::var(DEVICE_PIN_ENV) {
        return Ok(pin);
    }
    if !crate::output::can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("{prompt} is needed; run in a terminal or set {DEVICE_PIN_ENV}"),
        }));
    }
    eprint!("{prompt}: ");
    console::Term::stderr()
        .read_secure_line()
        .into_alien_error()
        .context(ErrorData::CliInteractionFailed {
            message: "Failed to read the PIN".to_string(),
        })
}

fn run(command: &mut Command, dir: &Path, input: Option<&[u8]>, action: &str) -> Result<()> {
    output(command, dir, input, action).map(|_| ())
}

fn output(command: &mut Command, dir: &Path, input: Option<&[u8]>, action: &str) -> Result<String> {
    let program = command.get_program().to_string_lossy().to_string();
    let mut child = command
        .current_dir(dir)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .into_alien_error()
        .context(signing_failed(&format!(
            "{action}: '{program}' could not start"
        )))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin
            .write_all(input)
            .into_alien_error()
            .context(signing_failed(&format!("{action}: write to '{program}'")))?;
    }
    let output = child
        .wait_with_output()
        .into_alien_error()
        .context(signing_failed(&format!(
            "{action}: '{program}' did not finish"
        )))?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "{action}: '{program}' failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn public_from_der(path: &Path) -> Result<PublicKey> {
    let der = std::fs::read(path)
        .into_alien_error()
        .context(file_failed(path, "read the public key"))?;
    PublicKey::parse(&format!("ecdsa-p256:{}", STANDARD.encode(der)))
        .context(signing_failed("read the public key"))
}

/// Write a file only its owner can read, refusing to replace one.
pub fn write_private(path: &Path, contents: &str) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).into_alien_error().context(file_failed(
        path,
        "create the private file (it must not exist yet)",
    ))?;
    file.write_all(contents.as_bytes())
        .into_alien_error()
        .context(file_failed(path, "write the private file"))
}

fn restrict_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .into_alien_error()
            .context(file_failed(dir, "restrict the device key directory"))?;
    }
    Ok(())
}

fn file_failed(path: &Path, reason: &str) -> ErrorData {
    ErrorData::FileOperationFailed {
        operation: reason.to_string(),
        file_path: path.display().to_string(),
        reason: reason.to_string(),
    }
}

fn signing_failed(action: &str) -> ErrorData {
    ErrorData::SigningFailed {
        message: format!("Could not {action}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_software_device_key_signs_and_survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let key = DeviceKey::create(Backend::Software, dir.path()).unwrap();
        let reopened = DeviceKey::open(Backend::Software, dir.path()).unwrap();
        assert_eq!(reopened.public_key, key.public_key);
        let signature = reopened.sign(b"statement bytes").unwrap();
        key.public_key
            .verify(b"statement bytes", &signature)
            .unwrap();
        assert_eq!(Backend::Software.signer_kind(), SignerKind::Software);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(dir.path().join("key.pem"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        DeviceKey::create(Backend::Software, dir.path())
            .expect_err("an existing key is never replaced");
    }

    #[test]
    fn approval_codes_are_four_upper_hex_digits_of_the_key() {
        let key = SoftwareKey::from_secret_bytes(&[7; 32])
            .unwrap()
            .public_key();
        let code = approval_code(&key);
        assert_eq!(code.len(), 4);
        assert!(code
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
        assert!(key.fingerprint().to_uppercase().contains(&code));
    }

    /// Runs against a real TPM: the machine's, or a simulator named by
    /// `TPM2TOOLS_TCTI` (for example `swtpm:port=2321`).
    #[test]
    #[ignore = "needs a TPM or a TPM simulator and tpm2-tools"]
    fn a_tpm_key_signs_with_its_pin_and_refuses_a_wrong_one() {
        std::env::set_var(DEVICE_PIN_ENV, "4321");
        let dir = tempfile::tempdir().unwrap();
        let key = DeviceKey::create(Backend::Tpm, dir.path()).unwrap();
        let signature = DeviceKey::open(Backend::Tpm, dir.path())
            .unwrap()
            .sign(b"statement")
            .unwrap();
        key.public_key.verify(b"statement", &signature).unwrap();
        std::env::set_var(DEVICE_PIN_ENV, "0000");
        key.sign(b"statement").expect_err("a wrong PIN is refused");
        assert!(
            !dir.path().join("key.pem").exists(),
            "nothing exportable is written"
        );
    }
}
