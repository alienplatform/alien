use super::*;
use proptest::prelude::*;
use serde_json::json;
fn allow(targets: &[&str]) -> EgressPolicy {
    EgressPolicy::Allow {
        allow: targets.iter().map(|s| s.to_string()).collect(),
    }
}
#[test]
fn runtime_policy_is_separate_from_declaration_and_legacy_aliases() {
    for value in [
        json!("none"),
        json!({"mode":"deny"}),
        json!({"mode":"allowDomains","domains":["example.com"]}),
        json!({"allow":[],"extra":true}),
    ] {
        assert!(serde_json::from_value::<EgressPolicy>(value).is_err());
    }
    assert_eq!(
        serde_json::from_value::<EgressPolicy>(json!({"allow":[]})).unwrap(),
        allow(&[])
    );
    assert!(!allow(&[]).allows("example.com", 443).unwrap());
}
#[test]
fn names_ports_and_networks_are_normalized() {
    assert_eq!(
        allow(&["API.Example.COM.:443", "*.BÜCHER.de", "8.8.8.7/24:80"])
            .normalized()
            .unwrap(),
        allow(&["api.example.com:443", "*.xn--bcher-kva.de", "8.8.8.0/24:80"])
    );
}
#[test]
fn unsafe_or_ambiguous_targets_are_refused() {
    for target in [
        "*",
        "*.com",
        "*.co.uk",
        "*.github.io",
        "localhost",
        "example.com:0",
        "example.com:65536",
        "example.com:+80",
        "example.com:",
        "https://example.com",
        "foo.*.com",
        "10.0.0.1",
        "127.0.0.1",
        "100.64.0.1",
        "169.254.169.254",
        "168.63.129.16",
        "192.168.1.0/24",
        "8.0.0.0/6",
        "0.0.0.0/0",
        "::1",
        "[2606:4700::1111]",
        "127.1",
        "0x7f000001",
        "a.example.com..",
    ] {
        assert!(allow(&[target]).validate().is_err(), "accepted {target}");
    }
}
#[test]
fn wildcards_include_the_apex_and_unspecified_ports_match_all_ports() {
    let policy = allow(&["*.example.com", "8.8.8.0/24:53"]);
    for host in ["example.com", "a.example.com", "a.b.example.com"] {
        for port in [22, 80, 443, 8443] {
            assert!(policy.allows(host, port).unwrap());
        }
    }
    assert!(!policy.allows("badexample.com", 443).unwrap());
    assert!(!policy.allows("example.com.evil.org", 443).unwrap());
    assert!(policy.allows("8.8.8.8", 53).unwrap());
    assert!(!policy.allows("8.8.8.8", 443).unwrap());
    assert!(!allow(&["example.com:443"])
        .allows("example.com", 8443)
        .unwrap());
    assert!(EgressPolicy::Internet
        .allows("169.254.169.254", 80)
        .is_err());
}
#[test]
fn canonical_encoding_and_limits_cover_the_full_policy() {
    assert_eq!(
        allow(&["B.example.com", "a.example.com."])
            .canonical_json()
            .unwrap(),
        br#"{"allow":["a.example.com","b.example.com"]}"#
    );
    let accepted = EgressPolicy::Allow {
        allow: (0..4096).map(|i| format!("host{i}.example.com")).collect(),
    };
    accepted.validate().unwrap();
    let too_many = EgressPolicy::Allow {
        allow: vec!["example.com".into(); 4097],
    };
    assert_eq!(
        too_many.validate().unwrap_err().code,
        "SANDBOX_EGRESS_POLICY_TOO_LARGE"
    );
    let long = format!(
        "{}.{}.{}.example.com",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63)
    );
    assert_eq!(
        EgressPolicy::Allow {
            allow: vec![long; 4096]
        }
        .validate()
        .unwrap_err()
        .code,
        "SANDBOX_EGRESS_POLICY_TOO_LARGE"
    );
}
proptest! {
    #[test]
    fn canonicalization_is_idempotent(label in "[a-z]{1,30}", port in 1u16..=65535) {
        let policy=allow(&[&format!("{}.EXAMPLE.COM.:{port}",label.to_uppercase())]);
        let normalized=policy.normalized().unwrap();
        prop_assert_eq!(normalized.normalized().unwrap(),normalized.clone());
        let host = format!("{label}.example.com");
        prop_assert!(normalized.allows(&host,port).unwrap());
        let other = format!("other{label}.example.com");
        prop_assert!(!normalized.allows(&other,port).unwrap());
    }
}
