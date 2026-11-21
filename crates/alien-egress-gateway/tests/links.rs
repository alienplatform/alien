//! Sandbox links: a declared service is reached by its exact internal hostname on its declared
//! ports, and only while the sandbox's policy keeps it.
#![cfg(feature = "local")]
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{atomic::AtomicU64, Arc},
};

use alien_bindings::Bindings;
use alien_egress_gateway::{
    policy::{check_link_addresses, Policy, Target},
    record::Record,
    runtime,
    server::Gateway,
    token::Claims,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ed25519_compact::{KeyPair, Seed};
use serde_json::{json, Value};

const AUD: &str = "prefix/agents";

fn config(key: &KeyPair) -> Value {
    json!({
        "listen":"127.0.0.1:3128", "admin":"127.0.0.1:3129",
        "audiences":[AUD, "prefix/other"],
        "keys":[{"kid":"test","publicKey":STANDARD.encode(key.pk.as_ref())}],
        "replicaSecret":STANDARD.encode([21;32]), "deniedRanges":["10.0.0.0/8"],
        "policyMemoryBytes":1048576,
        "links":{AUD:[{"id":"api","host":"api.svc","ports":[8080],"ranges":["10.244.0.0/16"]}]}
    })
}

async fn gateway(
    config: &Value,
) -> alien_error::Result<Gateway, alien_egress_gateway::error::ErrorData> {
    let directory = tempfile::tempdir().unwrap();
    let bindings = Arc::new(
        Bindings::from_env_map(HashMap::from([
            ("ALIEN_DEPLOYMENT_TYPE".into(), "local".into()),
            (
                "ALIEN_SECRETS_BINDING".into(),
                json!({"service":"local-vault","dataDir":directory.path(),"vaultName":"secrets"})
                    .to_string(),
            ),
        ]))
        .unwrap(),
    );
    runtime::build(
        &config.to_string(),
        bindings,
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    )
    .await
}

fn apply(gateway: &Gateway, key: &KeyPair, aud: &str, sub: &str, policy: &Policy) {
    let now = chrono::Utc::now().timestamp_millis() as u64;
    let payload = json!({"v":1,"aud":aud,"sub":sub,"ver":[now,"00000000000000000000000000000001"],"policy":policy});
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"test","typ":"alien-egress-record"}"#),
        URL_SAFE_NO_PAD.encode(payload.to_string())
    );
    let signed = format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(key.sk.sign(input.as_bytes(), None).as_ref())
    );
    gateway
        .policies
        .apply(
            Record::verify(signed, &gateway.keys, &gateway.config.audiences, now).unwrap(),
            false,
        )
        .unwrap();
}

fn claims(aud: &str, sub: &str) -> Claims {
    let now = chrono::Utc::now().timestamp() as u64;
    Claims {
        iss: "alien-sandbox".into(),
        aud: aud.into(),
        sub: sub.into(),
        iat: now,
        exp: now + 600,
    }
}

fn target(value: &str) -> Target {
    Target::parse(value, None).unwrap()
}

#[tokio::test]
async fn a_link_is_reached_only_by_its_name_and_port_and_only_while_the_policy_keeps_it() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let gateway = gateway(&config(&key)).await.unwrap();
    let policies = [
        ("open", Policy::Internet),
        (
            "named",
            Policy::Allow {
                allow: vec!["link:api".into()],
            },
        ),
        ("deny-all", Policy::Allow { allow: vec![] }),
        (
            "other-link",
            Policy::Allow {
                allow: vec!["link:billing".into(), "api.svc".into()],
            },
        ),
    ];
    for (sub, policy) in &policies {
        apply(&gateway, &key, AUD, sub, policy);
    }
    apply(&gateway, &key, "prefix/other", "open", &Policy::Internet);

    // `internet` keeps the declared link, and `link:api` names it.
    for sub in ["open", "named"] {
        let link = gateway
            .authorize(&claims(AUD, sub), &target("api.svc:8080"))
            .await
            .unwrap()
            .expect("declared link");
        assert_eq!(link.id, "api");
    }
    // An empty allowlist still means nothing, links included; so does a list that names a
    // different link or the link's hostname as a destination.
    for sub in ["deny-all", "other-link"] {
        let error = gateway
            .authorize(&claims(AUD, sub), &target("api.svc:8080"))
            .await
            .unwrap_err();
        assert_eq!(error.code, "EGRESS_POLICY_DENIED", "{sub}");
    }
    // Another port, another audience, or the address instead of the name is not the link:
    // those take the ordinary path, which never dials a private answer.
    for (aud, value) in [
        (AUD, "api.svc:8081"),
        ("prefix/other", "api.svc:8080"),
        (AUD, "10.244.0.5:8080"),
        (AUD, "other.svc:8080"),
    ] {
        let result = gateway
            .authorize(&claims(aud, "open"), &target(value))
            .await;
        assert!(
            !matches!(result, Ok(Some(_))),
            "{aud} {value} must not match a link"
        );
    }
}

#[tokio::test]
async fn linked_answers_must_all_fall_in_the_link_ranges() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let gateway = gateway(&config(&key)).await.unwrap();
    let link = gateway.config.link(AUD, "api.svc", 8080).unwrap();
    let inside: IpAddr = "10.244.1.9".parse().unwrap();
    check_link_addresses(&[inside], link).unwrap();
    assert!(check_link_addresses(&[], link).is_err());
    // A rebinding answer, metadata, loopback or IPv6 next to a good answer refuses them all.
    for other in [
        "10.245.0.1",
        "169.254.169.254",
        "127.0.0.1",
        "8.8.8.8",
        "fd00:ec2::254",
        "fe80::1",
    ] {
        assert!(
            check_link_addresses(&[inside, other.parse().unwrap()], link).is_err(),
            "{other}"
        );
    }
}

#[tokio::test]
async fn invalid_links_refuse_the_whole_configuration() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let link = |range: &str, ports: Value| json!({"id":"api","host":"api.svc","ports":ports,"ranges":[range]});
    for links in [
        json!({"prefix/unknown":[link("10.244.0.0/16", json!([8080]))]}),
        json!({AUD:[link("169.254.0.0/16", json!([80]))]}),
        json!({AUD:[link("0.0.0.0/0", json!([80]))]}),
        json!({AUD:[link("10.0.0.0/7", json!([80]))]}),
        json!({AUD:[link("10.244.0.0/16", json!([53]))]}),
        json!({AUD:[link("10.244.0.0/16", json!([]))]}),
        json!({AUD:[link("10.244.0.0/16", json!([80])), link("10.245.0.0/16", json!([81]))]}),
        json!({AUD:[{"id":"api","host":"api.svc","ports":[80],"ranges":["10.0.0.0/8"],"extra":1}]}),
    ] {
        let mut config = config(&key);
        config["links"] = links.clone();
        assert!(gateway(&config).await.is_err(), "{links}");
    }
    // A configuration without links is read exactly as before.
    let mut config = config(&key);
    config.as_object_mut().unwrap().remove("links");
    assert!(gateway(&config).await.unwrap().config.links.is_empty());
}

#[tokio::test]
async fn fixed_link_addresses_must_lie_in_the_link_ranges() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let link = |addresses: Value| {
        json!({AUD:[{"id":"api","host":"api.svc","ports":[8080],
        "ranges":["10.244.0.0/16"],"addresses":addresses}]})
    };
    for addresses in [
        json!(["127.0.0.1"]),
        json!(["169.254.169.254"]),
        json!(["10.245.0.4"]),
        json!(["10.244.0.4", "8.8.8.8"]),
        json!(["fd00:ec2::254"]),
    ] {
        let mut config = config(&key);
        config["links"] = link(addresses.clone());
        assert!(gateway(&config).await.is_err(), "{addresses}");
    }
    let mut config = config(&key);
    config["links"] = link(json!(["10.244.7.9"]));
    let gateway = gateway(&config).await.unwrap();
    let link = gateway.config.link(AUD, "api.svc", 8080).unwrap();
    assert_eq!(
        link.addresses,
        ["10.244.7.9".parse::<std::net::Ipv4Addr>().unwrap()]
    );
}

/// The 30-second sweep closes an open linked tunnel once the configuration stops declaring that
/// link, and once the link's ranges no longer admit the address the tunnel is connected to. A
/// tunnel whose link and address are still declared stays open.
#[tokio::test]
async fn the_sweep_closes_a_linked_tunnel_after_an_unlink_or_a_range_change() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let directory = tempfile::tempdir().unwrap();
    let bindings = Arc::new(
        Bindings::from_env_map(HashMap::from([
            ("ALIEN_DEPLOYMENT_TYPE".into(), "local".into()),
            (
                "ALIEN_SECRETS_BINDING".into(),
                json!({"service":"local-vault","dataDir":directory.path(),"vaultName":"secrets"})
                    .to_string(),
            ),
        ]))
        .unwrap(),
    );
    let path = directory.path().join("config.json");
    let write = |config: &Value| std::fs::write(&path, config.to_string()).unwrap();
    let linked = config(&key);
    write(&linked);
    let gateway = runtime::build(
        &linked.to_string(),
        bindings.clone(),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    )
    .await
    .unwrap();
    apply(&gateway, &key, AUD, "open", &Policy::Internet);
    let live = runtime::LiveGateway::refreshing(gateway, runtime::ConfigSource::File(path.clone()));
    let registry = live.snapshot().unwrap().connections.clone();
    let open = |peer: &str| {
        let (permit, cancelled) = registry
            .register(
                claims(AUD, "open"),
                Some(target("api.svc:8080")),
                Arc::new(alien_egress_gateway::io::Counters::default()),
            )
            .unwrap();
        permit
            .record_peer(peer.parse().unwrap(), Some("api".into()))
            .unwrap();
        (permit, cancelled)
    };

    // Declared link, address inside its range: the sweep keeps it.
    let (_kept, kept) = open("10.244.0.5");
    registry.revalidate(&live).await;
    assert_eq!(*kept.borrow(), None);

    // The link's ranges move away from the connected address: closed.
    let mut moved = linked.clone();
    moved["links"][AUD][0]["ranges"] = json!(["10.245.0.0/16"]);
    write(&moved);
    live.refresh(&bindings).await.unwrap();
    registry.revalidate(&live).await;
    assert_eq!(*kept.borrow(), Some("policy revoked or unavailable"));

    // The link is removed from the configuration: a tunnel opened before is closed.
    write(&linked);
    live.refresh(&bindings).await.unwrap();
    let (_unlinked, unlinked) = open("10.244.0.6");
    registry.revalidate(&live).await;
    assert_eq!(*unlinked.borrow(), None);
    let mut removed = linked.clone();
    removed.as_object_mut().unwrap().remove("links");
    write(&removed);
    live.refresh(&bindings).await.unwrap();
    registry.revalidate(&live).await;
    assert_eq!(*unlinked.borrow(), Some("policy revoked or unavailable"));
}
