//! What the helper needs to attach an S3 Files file system: the client certificate that carries
//! the caller's IAM identity, the proxy's configuration, and the NFS mount options.
//!
//! This is a port of the AWS efs-utils mount helper (github.com/aws/efs-utils, release 3.4.0,
//! commit 2bca638e6ea0929f59d5f73940c575d548197b72), which is the reference for the handshake.
//! Nothing here is invented:
//!
//! - the certificate extensions and the SigV4 request they sign follow
//!   `src/efs_utils_common/certificate_utils.py` (`create_certificate`, `efs_client_auth_builder`,
//!   `create_canonical_request`, `create_string_to_sign`, `calculate_signature`), with the service
//!   name `s3files` that `src/mount_s3files/__init__.py` sets;
//! - the proxy configuration follows `write_stunnel_config_file` in
//!   `src/efs_utils_common/proxy.py` (and the keys `EfsConfig` reads in
//!   `src/client-core/src/config_parser.rs`);
//! - the NFS options follow `get_nfs_mount_options` in
//!   `src/efs_utils_common/mount_options.py` for an S3 Files mount (NFS 4.2 through the proxy's
//!   loopback port).
//!
//! The upstream helper shells out to `openssl` and Python. The agent must never run a binary from
//! the image, so the certificate is made here instead.

use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use rsa::pkcs1::EncodeRsaPublicKey;
use rsa::pkcs8::EncodePrivateKey;
use sha2::{Digest, Sha256};

use crate::mounts::MountCredentials;

/// `Action=Connect` requests are signed for this service (mount_s3files/__init__.py).
pub const SERVICE: &str = "s3files";
const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const REQUEST_TYPE: &str = "aws4_request";
/// Certificate validity window (constants.py: NOT_BEFORE_MINS, NOT_AFTER_HOURS).
const NOT_BEFORE_MINUTES: i64 = 15;
const NOT_AFTER_HOURS: i64 = 3;
/// Key size the upstream helper generates (certificate_utils.py `generate_key`).
const KEY_BITS: usize = 3072;

/// The key pair the client certificate is made for. One per sandbox boot.
pub struct ClientKey {
    pkcs8_der: Vec<u8>,
    public_key_hash: String,
}

impl ClientKey {
    pub fn generate() -> Result<Self, String> {
        let private = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, KEY_BITS)
            .map_err(|_| "cannot generate the client key".to_string())?;
        Self::from_private(&private)
    }

    pub fn from_pkcs8_der(der: &[u8]) -> Result<Self, String> {
        use rsa::pkcs8::DecodePrivateKey;
        let private = rsa::RsaPrivateKey::from_pkcs8_der(der)
            .map_err(|_| "the client key is unreadable".to_string())?;
        Self::from_private(&private)
    }

    fn from_private(private: &rsa::RsaPrivateKey) -> Result<Self, String> {
        let pkcs8_der = private
            .to_pkcs8_der()
            .map_err(|_| "cannot encode the client key".to_string())?
            .as_bytes()
            .to_vec();
        // `get_public_key_sha1`: SHA-1 of the key material inside the public key's BIT STRING,
        // which for RSA is the DER RSAPublicKey.
        let public = rsa::RsaPublicKey::from(private)
            .to_pkcs1_der()
            .map_err(|_| "cannot encode the public key".to_string())?;
        Ok(Self {
            pkcs8_der,
            public_key_hash: sha1_hex(public.as_bytes()),
        })
    }

    /// The private key, for the file the proxy reads. Mode 0400 root, written by the helper.
    pub fn pkcs8_pem(&self) -> String {
        pem("PRIVATE KEY", &self.pkcs8_der)
    }
}

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest as _;
    hex::encode(sha1::Sha1::digest(bytes))
}

fn pem(label: &str, der: &[u8]) -> String {
    use base64::Engine as _;
    let body = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for chunk in body.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

/// Python's `urllib.parse.quote_plus`: everything but letters, digits and `_.-~` is escaped.
fn quote_plus(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn credential_scope(date: DateTime<Utc>, region: &str) -> String {
    format!("{}/{region}/{SERVICE}/{REQUEST_TYPE}", date.format("%Y%m%d"))
}

/// `create_canonical_request`
fn canonical_request(
    public_key_hash: &str,
    date: DateTime<Utc>,
    access_key_id: &str,
    region: &str,
    fs_id: &str,
    session_token: &str,
) -> String {
    let credential = quote_plus(&format!(
        "{access_key_id}/{}",
        credential_scope(date, region)
    ));
    // `create_canonical_query_string`: parameters sorted by name.
    let mut parameters = vec![
        ("Action", "Connect".to_string()),
        ("PublicKeyHash", quote_plus(public_key_hash)),
        ("X-Amz-Algorithm", ALGORITHM.to_string()),
        ("X-Amz-Credential", credential),
        (
            "X-Amz-Date",
            quote_plus(&date.format("%Y%m%dT%H%M%SZ").to_string()),
        ),
        ("X-Amz-Expires", "86400".to_string()),
        ("X-Amz-SignedHeaders", "host".to_string()),
    ];
    if !session_token.is_empty() {
        parameters.push(("X-Amz-Security-Token", quote_plus(session_token)));
    }
    parameters.sort_by(|a, b| a.0.cmp(b.0));
    let query = parameters
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    // The payload is empty, so its hash is that of the empty string.
    format!(
        "GET\n/\n{query}\nhost:{fs_id}\nhost\n{}",
        hex::encode(Sha256::digest(b""))
    )
}

/// `create_string_to_sign`
fn string_to_sign(canonical_request: &str, date: DateTime<Utc>, region: &str) -> String {
    format!(
        "{ALGORITHM}\n{}\n{}\n{}",
        date.format("%Y%m%dT%H%M%SZ"),
        credential_scope(date, region),
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    )
}

fn hmac_sha256(key: &[u8], message: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(message.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// `calculate_signature`: returns the signature as lower-case hex.
fn signature(string_to_sign: &str, date: DateTime<Utc>, secret: &str, region: &str) -> String {
    let key_date = hmac_sha256(
        format!("AWS4{secret}").as_bytes(),
        &date.format("%Y%m%d").to_string(),
    );
    let key_region = hmac_sha256(&key_date, region);
    let key_service = hmac_sha256(&key_region, SERVICE);
    let signing_key = hmac_sha256(&key_service, REQUEST_TYPE);
    hex::encode(hmac_sha256(&signing_key, string_to_sign))
}

/// The signature the certificate carries for `key` and these credentials.
pub fn connect_signature(
    key: &ClientKey,
    date: DateTime<Utc>,
    credentials: &MountCredentials,
    region: &str,
    fs_id: &str,
) -> String {
    let request = canonical_request(
        &key.public_key_hash,
        date,
        &credentials.access_key_id,
        region,
        fs_id,
        &credentials.session_token,
    );
    signature(
        &string_to_sign(&request, date, region),
        date,
        &credentials.secret_access_key,
        region,
    )
}

// ---- DER pieces for the certificate extensions (what openssl's `ASN1:` config lines produce) ----

fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let length = content.len();
    if length < 0x80 {
        out.push(length as u8);
    } else {
        let bytes: Vec<u8> = length
            .to_be_bytes()
            .into_iter()
            .skip_while(|b| *b == 0)
            .collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
    out.extend_from_slice(content);
    out
}

fn utf8(text: &str) -> Vec<u8> {
    der(0x0c, text.as_bytes())
}

/// `efs_client_auth_builder`: accessKeyId, signature, sigv4DateTime and, explicitly tagged `[0]`,
/// the session token.
fn client_auth_extension(
    access_key_id: &str,
    signature_hex: &str,
    date: DateTime<Utc>,
    session_token: &str,
) -> Result<Vec<u8>, String> {
    // Reject a malformed credential before use (the same checks as upstream).
    let key_ok = (16..=128).contains(&access_key_id.len())
        && access_key_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if !key_ok {
        return Err("accessKeyId is malformed".into());
    }
    let token_ok = session_token.is_empty()
        || session_token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'_' | b'-'));
    if !token_ok {
        return Err("sessionToken is malformed".into());
    }
    let mut body = utf8(access_key_id);
    // openssl's `OCTETSTRING:<text>` (what upstream writes) takes the text itself, not decoded
    // bytes: the 64 hex characters of the signature are the 64 bytes of the OCTET STRING.
    if signature_hex.len() != 64 || !signature_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("bad signature".into());
    }
    body.extend(der(0x04, signature_hex.as_bytes()));
    body.extend(der(0x17, date.format("%y%m%d%H%M%SZ").to_string().as_bytes()));
    if !session_token.is_empty() {
        body.extend(der(0xa0, &utf8(session_token)));
    }
    Ok(der(0x30, &body))
}

/// The client certificate, self-signed with `key`, valid from 15 minutes before `now` for three
/// hours. A renewal calls this again with fresh credentials and rewrites the file.
pub fn client_certificate_pem(
    key: &ClientKey,
    credentials: &MountCredentials,
    region: &str,
    fs_id: &str,
    access_point_id: &str,
    common_name: &str,
    now: DateTime<Utc>,
) -> Result<String, String> {
    use rcgen::{CertificateParams, CustomExtension, DistinguishedName, DnType, KeyIdMethod};
    // SigV4 times carry whole seconds.
    let date = DateTime::<Utc>::from_timestamp(now.timestamp(), 0).ok_or("bad time")?;
    let sig = connect_signature(key, date, credentials, region, fs_id);
    let auth = client_auth_extension(
        &credentials.access_key_id,
        &sig,
        date,
        &credentials.session_token,
    )?;

    let key_pair = rcgen::KeyPair::try_from(key.pkcs8_der.as_slice())
        .map_err(|_| "the client key cannot sign".to_string())?;
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, common_name);
    params.distinguished_name = name;
    let to_time = |t: DateTime<Utc>| {
        time::OffsetDateTime::from_unix_timestamp(t.timestamp()).map_err(|_| "bad time")
    };
    params.not_before = to_time(date - Duration::minutes(NOT_BEFORE_MINUTES))?;
    params.not_after = to_time(date + Duration::hours(NOT_AFTER_HOURS))?;
    // `subjectKeyIdentifier = hash` is the SHA-1 of the key material.
    params.key_identifier_method = KeyIdMethod::PreSpecified(
        hex::decode(&key.public_key_hash).map_err(|_| "bad key hash".to_string())?,
    );
    // `subjectKeyIdentifier = hash`, as upstream's v3_ca section asks (written as a plain
    // extension so the SHA-1 key hash is exactly openssl's).
    params.custom_extensions.push(CustomExtension::from_oid_content(
        &[2, 5, 29, 14],
        der(
            0x04,
            &hex::decode(&key.public_key_hash).map_err(|_| "bad key hash".to_string())?,
        ),
    ));
    params.serial_number = Some(rcgen::SerialNumber::from_slice(
        &Sha256::digest(format!("{}{}", date.timestamp(), credentials.access_key_id))[..16],
    ));
    let extension = |last: u64, value: Vec<u8>| {
        CustomExtension::from_oid_content(&[1, 3, 6, 1, 4, 1, 4843, 7, last], value)
    };
    // Order as upstream writes them: access point, client auth, file system id.
    if !access_point_id.is_empty() {
        params.custom_extensions.push(extension(1, utf8(access_point_id)));
    }
    params.custom_extensions.push(extension(2, auth));
    params.custom_extensions.push(extension(3, utf8(fs_id)));
    let certificate = params
        .self_signed(&key_pair)
        .map_err(|_| "cannot sign the client certificate".to_string())?;
    Ok(certificate.pem())
}

/// The proxy's configuration file (`write_stunnel_config_file`, efs-proxy mode, TLS, verify 2).
/// Everything is a path or an identifier; the credentials are only inside the certificate file.
pub struct ProxyFiles<'a> {
    pub listen_port: u16,
    pub dns_name: &'a str,
    pub fs_id: &'a str,
    pub region: &'a str,
    pub ca_file: &'a str,
    pub certificate: &'a str,
    pub private_key: &'a str,
    pub pid_file: &'a str,
}

pub fn proxy_config(files: &ProxyFiles<'_>) -> String {
    let lines = [
        "fips = no".to_string(),
        "foreground = yes".to_string(),
        "socket = l:SO_REUSEADDR=yes".to_string(),
        "socket = a:SO_BINDTODEVICE=lo".to_string(),
        format!("pid = {}", files.pid_file),
        "[efs]".to_string(),
        "client = yes".to_string(),
        format!("accept = 127.0.0.1:{}", files.listen_port),
        format!("connect = {}:2049", files.dns_name),
        "sslVersion = TLSv1.2".to_string(),
        "renegotiation = no".to_string(),
        "TIMEOUTbusy = 20".to_string(),
        "TIMEOUTclose = 0".to_string(),
        "TIMEOUTidle = 70".to_string(),
        "delay = yes".to_string(),
        "verify = 2".to_string(),
        format!("CAfile = {}", files.ca_file),
        format!("cert = {}", files.certificate),
        format!("key = {}", files.private_key),
        format!("checkHost = {}", files.dns_name),
        "retry_nfs_mount_command_timeout_sec = 30".to_string(),
        format!("fs_id = {}", files.fs_id),
        format!("region = {}", files.region),
    ];
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The arguments the proxy is started with (`bootstrap_proxy`): config file, TLS, and no direct
/// S3 reads (the proxy's S3 read path needs its own credentials, which this mount does not have).
pub fn proxy_arguments(config_file: &str) -> Vec<String> {
    vec![
        config_file.to_string(),
        "--tls".to_string(),
        "--no-direct-s3-read".to_string(),
    ]
}

/// Name the proxy connects to and checks the server certificate against.
pub fn mount_target_name(fs_id: &str, region: &str) -> String {
    format!("{fs_id}.s3files.{region}.on.aws")
}

/// NFS options of an S3 Files mount through the proxy (`get_nfs_mount_options`), as `(name,
/// value)` pairs for `mount(2)`.
pub fn nfs_options(proxy_port: u16) -> String {
    format!(
        "nfsvers=4.2,rsize=1048576,wsize=1048576,hard,timeo=600,retrans=2,noresvport,tls,port={proxy_port},nosuid,nodev"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 18, 12, 0, 0).unwrap()
    }

    // The expected strings below were produced by running the upstream functions
    // (certificate_utils.create_canonical_request / create_string_to_sign / calculate_signature,
    // efs-utils 2bca638e6ea0929f59d5f73940c575d548197b72, service "s3files") on these inputs.
    const KEY_HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn the_signed_request_matches_the_upstream_helper() {
        let request = canonical_request(
            KEY_HASH,
            at(),
            "ASIAEXAMPLEKEYID123",
            "us-east-1",
            "fs-0abc123",
            "TOKEN/with+chars==",
        );
        assert_eq!(
            request,
            "GET\n/\nAction=Connect&PublicKeyHash=0123456789abcdef0123456789abcdef01234567&X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=ASIAEXAMPLEKEYID123%2F20261118%2Fus-east-1%2Fs3files%2Faws4_request&X-Amz-Date=20261118T120000Z&X-Amz-Expires=86400&X-Amz-Security-Token=TOKEN%2Fwith%2Bchars%3D%3D&X-Amz-SignedHeaders=host\nhost:fs-0abc123\nhost\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let to_sign = string_to_sign(&request, at(), "us-east-1");
        assert_eq!(
            to_sign,
            "AWS4-HMAC-SHA256\n20261118T120000Z\n20261118/us-east-1/s3files/aws4_request\n58b1fd7871f68628c496b1f676bf55facaaaa722e6d660377ef928c43b7a7455"
        );
        assert_eq!(
            signature(&to_sign, at(), "secret/KEY+example", "us-east-1"),
            "8a7f2d6e956c86a29b5e26e4261658fa79a225b9dda0b6906ec43ad17e58e402"
        );
    }

    #[test]
    fn the_client_auth_extension_has_the_upstream_layout() {
        // Byte for byte what upstream's openssl step produces (efs_client_auth_builder's config
        // run through `openssl req -x509` with these inputs; efs-utils 2bca638e6ea0).
        let upstream = "306F0C13415349414558414D504C454B45594944313233044032366166643637343132616439343438633632316566636539646131366466623038626665303833616336316535663232663039366635393833643233336430170D3236313131383132303030305AA0070C05544F4B454E";
        let signature = "26afd67412ad9448c621efce9da16dfb08bfe083ac61e5f22f096f5983d233d0";
        let ours = client_auth_extension("ASIAEXAMPLEKEYID123", signature, at(), "TOKEN").unwrap();
        assert_eq!(hex::encode_upper(ours), upstream);
        client_auth_extension("short", &"ab".repeat(32), at(), "").expect_err("key id too short");
        client_auth_extension("ASIAEXAMPLEKEYID123", &"ab".repeat(32), at(), "bad token!")
            .expect_err("token alphabet");
    }

    #[test]
    fn quoting_matches_python_quote_plus() {
        assert_eq!(quote_plus("a/b+c=d e~f-g_h.i"), "a%2Fb%2Bc%3Dd+e~f-g_h.i");
    }

    #[test]
    fn the_proxy_config_matches_the_upstream_layout() {
        let text = proxy_config(&ProxyFiles {
            listen_port: 20049,
            dns_name: "fs-0abc123.s3files.us-east-1.on.aws",
            fs_id: "fs-0abc123",
            region: "us-east-1",
            ca_file: "/opt/alien/efs-utils.crt",
            certificate: "/run/alien/mounts/x/certificate.pem",
            private_key: "/run/alien/mounts/x/key.pem",
            pid_file: "/run/alien/mounts/x/proxy.pid",
        });
        assert!(text.contains("[efs]\nclient = yes\naccept = 127.0.0.1:20049\nconnect = fs-0abc123.s3files.us-east-1.on.aws:2049\n"));
        assert!(text.contains("verify = 2\nCAfile = /opt/alien/efs-utils.crt\ncert = "));
        assert!(text.contains("checkHost = fs-0abc123.s3files.us-east-1.on.aws\n"));
        assert!(!text.contains("secret") && !text.contains("sessionToken"));
    }

    #[test]
    fn a_certificate_carries_the_access_point_signature_and_file_system() {
        // A small key keeps the test quick; the size is not what is under test.
        let private = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
        let key = ClientKey::from_private(&private).unwrap();
        let credentials = MountCredentials {
            access_key_id: "ASIAEXAMPLEKEYID123".into(),
            secret_access_key: "secret/KEY+example".into(),
            session_token: "TOKEN".into(),
            expiration: "2030-01-01T00:00:00Z".into(),
        };
        let pem = client_certificate_pem(
            &key,
            &credentials,
            "us-east-1",
            "fs-0abc123",
            "fsap-0123456789abcdef0",
            "sandbox",
            at(),
        )
        .unwrap();
        assert!(pem.starts_with("-----BEGIN CERTIFICATE-----"));
        // The secret never reaches the file.
        assert!(!pem.contains("secret"));
        // Decode and look for the three identifiers.
        use base64::Engine as _;
        let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
        let der = base64::engine::general_purpose::STANDARD.decode(body).unwrap();
        let text = String::from_utf8_lossy(&der).to_string();
        for needle in ["fsap-0123456789abcdef0", "fs-0abc123", "ASIAEXAMPLEKEYID123", "TOKEN"] {
            assert!(text.contains(needle), "{needle}");
        }
        // The signature in the certificate is the one for this key and time.
        let expected = connect_signature(&key, at(), &credentials, "us-east-1", "fs-0abc123");
        assert!(der
            .windows(expected.len())
            .any(|w| w == expected.as_bytes()));
    }

    /// Writes a certificate and its key to the directory in `S3FILES_DUMP`, for comparing with
    /// what the upstream helper's `openssl` steps produce.
    #[test]
    #[ignore]
    fn dump_a_certificate_for_comparison() {
        let dir = std::env::var("S3FILES_DUMP").expect("S3FILES_DUMP");
        let key = ClientKey::generate().unwrap();
        let credentials = MountCredentials {
            access_key_id: "ASIAEXAMPLEKEYID123".into(),
            secret_access_key: "secret/KEY+example".into(),
            session_token: "TOKEN".into(),
            expiration: "2030-01-01T00:00:00Z".into(),
        };
        let pem = client_certificate_pem(
            &key, &credentials, "us-east-1", "fs-0abc123", "fsap-0123456789abcdef0", "sandbox", at(),
        )
        .unwrap();
        std::fs::write(format!("{dir}/cert.pem"), pem).unwrap();
        std::fs::write(format!("{dir}/key.pem"), key.pkcs8_pem()).unwrap();
        std::fs::write(
            format!("{dir}/sig.txt"),
            connect_signature(&key, at(), &credentials, "us-east-1", "fs-0abc123"),
        )
        .unwrap();
    }

    #[test]
    fn nfs_options_use_version_4_2_through_the_proxy_port() {
        let options = nfs_options(20049);
        assert!(options.starts_with("nfsvers=4.2,"));
        assert!(options.contains("port=20049") && options.contains("noresvport"));
    }
}
