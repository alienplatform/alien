#![cfg(feature = "local")]
use alien_bindings::Bindings;
use alien_egress_gateway::{config::CredentialRule, credentials::Credentials, policy::Target};
use base64::{engine::general_purpose::STANDARD, Engine};
use rustls::{
    crypto::ring,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
    ClientConfig, RootCertStore, ServerConfig,
};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn client(root: CertificateDer<'static>) -> TlsConnector {
    let mut roots = RootCertStore::empty();
    roots.add(root).unwrap();
    TlsConnector::from(Arc::new(
        ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ))
}

#[tokio::test]
async fn unrelated_missing_credentials_do_not_break_other_methods_or_paths() {
    let directory = tempfile::tempdir().unwrap();
    let bindings = Arc::new(Bindings::from_env_map(HashMap::from([
        ("ALIEN_DEPLOYMENT_TYPE".into(), "local".into()),
        ("ALIEN_TEST_VAULT_BINDING".into(), serde_json::json!({"service":"local-vault","dataDir":directory.path(),"vaultName":"test"}).to_string()),
    ])).unwrap());
    bindings
        .vault("test-vault")
        .await
        .unwrap()
        .set_secret("present", "fake-scoped-value")
        .await
        .unwrap();
    let rules: Vec<CredentialRule> = serde_json::from_value(serde_json::json!([
        {"aud":"p/r","host":"inject.example.com","port":443,"method":"GET","path":"/private/","header":"X-Test-Credential","vaultBinding":"test-vault","secret":"present"},
        {"aud":"p/r","host":"inject.example.com","port":443,"method":"POST","path":"/private/","header":"X-Test-Credential","vaultBinding":"test-vault","secret":"missing"}
    ])).unwrap();
    let origin_key = rcgen::KeyPair::generate().unwrap();
    let origin_certificate = rcgen::CertificateParams::new(vec!["inject.example.com".into()])
        .unwrap()
        .self_signed(&origin_key)
        .unwrap();
    let server = Arc::new(
        ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![origin_certificate.der().clone()],
                PrivatePkcs8KeyDer::from(origin_key.serialize_der()).into(),
            )
            .unwrap(),
    );
    let credentials = Arc::new(
        Credentials::new(
            bindings,
            &[STANDARD.encode(origin_certificate.der())],
            &rules,
            Some([7; 32]),
        )
        .unwrap(),
    );
    let pem = credentials.issue_ca("p/r", "box").await.unwrap();
    let der = STANDARD
        .decode(
            pem.lines()
                .filter(|line| !line.starts_with("---"))
                .collect::<String>(),
        )
        .unwrap();
    let guest_tls = client(CertificateDer::from(der));

    for (method, path, expected_status, expected_secret) in [
        ("GET", "/public", 200, false),
        ("GET", "/private/action", 200, true),
        ("POST", "/public", 200, false),
        ("POST", "/private/action", 502, false),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = server.clone();
        let origin = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let socket = TlsAcceptor::from(server).accept(socket).await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut head = String::new();
            loop {
                let mut line = String::new();
                match socket.read_line(&mut line).await {
                    Ok(0) | Err(_) if head.is_empty() => return head,
                    result => {
                        assert!(result.unwrap() > 0);
                    }
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            socket
                .get_mut()
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
            socket.get_mut().shutdown().await.unwrap();
            head
        });
        let upstream = TcpStream::connect(address).await.unwrap();
        let (guest, proxy) = tokio::io::duplex(32 * 1024);
        let credentials = credentials.clone();
        let rules = rules.clone();
        let forwarding = tokio::spawn(async move {
            credentials
                .forward(
                    proxy,
                    upstream,
                    ("p/r", "box"),
                    Target::parse("inject.example.com:443", None).unwrap(),
                    &rules,
                )
                .await
        });
        let socket = guest_tls
            .connect(ServerName::try_from("inject.example.com").unwrap(), guest)
            .await
            .unwrap();
        let mut socket = BufReader::new(socket);
        socket.get_mut().write_all(format!("{method} {path} HTTP/1.1\r\nHost: inject.example.com\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut status = String::new();
        socket
            .read_line(&mut status)
            .await
            .expect("scoped request must receive an HTTP response");
        assert_eq!(
            status
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<u16>()
                .unwrap(),
            expected_status,
            "{method} {path}"
        );
        loop {
            let mut line = String::new();
            assert!(socket.read_line(&mut line).await.unwrap() > 0);
            if line == "\r\n" {
                break;
            }
        }
        if expected_status == 200 {
            let mut body = [0; 2];
            socket.read_exact(&mut body).await.unwrap();
            assert_eq!(&body, b"ok");
        }
        drop(socket);
        assert_eq!(forwarding.await.unwrap().is_ok(), expected_status == 200);
        let head = origin.await.unwrap();
        assert_eq!(head.is_empty(), expected_status != 200);
        assert_eq!(
            head.to_ascii_lowercase()
                .contains("x-test-credential: fake-scoped-value"),
            expected_secret
        );
    }
}
