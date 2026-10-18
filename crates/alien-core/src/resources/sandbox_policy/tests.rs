use super::*;
use proptest::prelude::*;
use serde_json::json;

fn allow(targets: &[&str]) -> SandboxEgress {
    SandboxEgress::Allow {
        allow: targets.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn legacy_policies_write_only_the_current_format() {
    for (input, output) in [
        (json!({"mode":"deny"}), json!("none")),
        (json!({"mode":"allow"}), json!("internet")),
        (
            json!({"mode":"allowDomains", "domains":["api.example.com"]}),
            json!({"allow":["api.example.com"]}),
        ),
    ] {
        let policy: SandboxEgress = serde_json::from_value(input).unwrap();
        assert_eq!(serde_json::to_value(&policy).unwrap(), output);
        assert_eq!(
            serde_json::from_value::<SandboxEgress>(output).unwrap(),
            policy
        );
    }
    for invalid in [
        json!({"allow":[],"mode":"allow"}),
        json!({"mode":"deny","domains":[]}),
        json!("allow"),
    ] {
        assert!(serde_json::from_value::<SandboxEgress>(invalid).is_err());
    }
}

#[test]
fn names_ports_and_networks_are_normalized() {
    assert_eq!(
        allow(&["API.Example.COM.:443", "*.BÜCHER.de", "8.8.8.7/24:80"])
            .normalized()
            .unwrap(),
        allow(&["api.example.com", "*.xn--bcher-kva.de", "8.8.8.0/24:80"])
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
    assert!(allow(&[]).validate().is_err());
    assert!(SandboxEgress::Allow {
        allow: vec!["example.com".to_string(); 4097]
    }
    .validate()
    .is_err());
}

#[test]
fn subset_rule_checks_ports_apex_wildcards_and_cidr_containment() {
    for (child, parent, expected) in [
        ("a.example.com", "*.example.com", true),
        ("a.b.example.com", "*.example.com", true),
        ("*.a.example.com", "*.example.com", true),
        ("*.example.com", "*.example.com", true),
        ("example.com", "*.example.com", false),
        ("badexample.com", "*.example.com", false),
        ("*.example.com", "a.example.com", false),
        ("a.example.com:80", "*.example.com", false),
        ("8.8.8.8", "8.8.8.0/24", true),
        ("8.8.8.0/25", "8.8.8.0/24", true),
        ("8.8.8.0/24", "8.8.8.8", false),
        ("8.8.8.0/24:80", "8.8.8.0/24", false),
    ] {
        assert_eq!(
            allow(&[child]).ensure_subset_of(&allow(&[parent])).is_ok(),
            expected,
            "{child} <= {parent}"
        );
    }
    assert!(SandboxEgress::None
        .ensure_subset_of(&allow(&["example.com"]))
        .is_ok());
    assert!(allow(&["example.com"])
        .ensure_subset_of(&SandboxEgress::Internet)
        .is_ok());
    let error = allow(&["example.com", "other.com"])
        .ensure_subset_of(&allow(&["example.com"]))
        .unwrap_err();
    assert_eq!(error.code, "SANDBOX_EGRESS_WIDENS");
    assert!(!error.retryable);
    assert!(error.message.contains("other.com"));
    assert!(SandboxEgress::Internet
        .ensure_subset_of(&allow(&["example.com"]))
        .is_err());
}

proptest! {
    #[test]
    fn subset_is_reflexive_and_transitive(label in "[a-z]{1,20}", sub in "[a-z]{1,20}", port in 1u16..=65535) {
        let parent = allow(&[&format!("*.example.com:{port}")]);
        let middle = allow(&[&format!("*.{label}.example.com:{port}")]);
        let child = allow(&[&format!("{sub}.{label}.example.com:{port}")]);
        prop_assert!(child.ensure_subset_of(&child).is_ok());
        prop_assert!(middle.ensure_subset_of(&parent).is_ok());
        prop_assert!(child.ensure_subset_of(&middle).is_ok());
        prop_assert!(child.ensure_subset_of(&parent).is_ok());
        prop_assert!(parent.ensure_subset_of(&child).is_err());
    }

    #[test]
    fn cidr_narrowing_agrees_with_address_membership(a in any::<u8>(), b in any::<u8>(), prefix in 16u8..=32) {
        let ip = Ipv4Addr::new(8, 8, a, b);
        let network = Ipv4Net::new(ip, prefix).unwrap().trunc();
        let child = allow(&[&ip.to_string()]);
        let parent = allow(&[&network.to_string()]);
        prop_assert!(child.ensure_subset_of(&parent).is_ok());
        prop_assert_eq!(parent.ensure_subset_of(&child).is_ok(), prefix == 32);
    }
}

#[test]
fn canonical_policy_sorts_normalizes_and_hashes_the_exact_json() {
    let policy = allow(&["Z.Example.COM.:443", "a.example.com"]);
    let canonical = br#"{"allow":["a.example.com","z.example.com"]}"#;
    assert_eq!(policy.canonical_json().unwrap(), canonical);
    assert_eq!(
        policy.policy_digest().unwrap(),
        allow(&["a.example.com", "z.example.com"])
            .policy_digest()
            .unwrap()
    );
    assert_eq!(SandboxEgress::None.canonical_json().unwrap(), br#""none""#);
}

#[test]
fn policy_limits_preserve_4096_targets_and_refuse_oversized_canonical_json() {
    let accepted = SandboxEgress::Allow {
        allow: (0..4096).map(|i| format!("api-{i}.example.com")).collect(),
    };
    accepted.validate().unwrap();
    assert!(accepted.canonical_json().unwrap().len() < 256 * 1024);
    let too_many = SandboxEgress::Allow {
        allow: vec!["example.com".to_string(); 4097],
    };
    assert_eq!(
        too_many.validate().unwrap_err().code,
        "SANDBOX_EGRESS_POLICY_TOO_LARGE"
    );
    let too_large = SandboxEgress::Allow {
        allow: (0..4096)
            .map(|i| format!("{i}.{}.{}.example.com", "a".repeat(63), "b".repeat(63)))
            .collect(),
    };
    assert_eq!(
        too_large.validate().unwrap_err().code,
        "SANDBOX_EGRESS_POLICY_TOO_LARGE"
    );
}
