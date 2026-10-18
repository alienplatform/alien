//! Translation and readback checks for Azure's native HTTP policy.
use crate::error::{ErrorData, Result};
use alien_azure_clients::azure::sandbox_data_plane::{
    EgressHostRule, EgressPolicy, EgressRule, EgressRuleAction, EgressRuleMatch,
};
use alien_core::SandboxEgress;
use alien_error::{AlienError, ContextError};

fn invalid(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxEgressInvalid {
        reason: reason.into(),
    })
}

fn rule(host: &str, scheme: &str, action: &str) -> EgressRule {
    EgressRule {
        name: None,
        r#match: Some(EgressRuleMatch {
            host: host.to_string(),
            scheme: Some(scheme.to_string()),
            path: None,
            methods: None,
        }),
        action: Some(EgressRuleAction {
            action_type: action.to_string(),
            host: None,
            path: None,
            scheme: None,
            headers: None,
        }),
    }
}

/// Only HTTP(S) ports are expressible in the native policy. Refuse other targets before create.
pub(super) fn policy(egress: &SandboxEgress) -> Result<EgressPolicy> {
    let egress = egress.normalized().map_err(|e| {
        e.context(ErrorData::SandboxEgressInvalid {
            reason: "invalid Azure egress target".to_string(),
        })
    })?;
    let mut policy = EgressPolicy {
        default_action: if matches!(egress, SandboxEgress::Internet) {
            "Allow"
        } else {
            "Deny"
        }
        .to_string(),
        host_rules: Vec::new(),
        rules: Vec::new(),
        traffic_inspection: Some("Full".to_string()),
        unmodelled: Default::default(),
    };
    match egress {
        SandboxEgress::None => policy.host_rules.push(EgressHostRule {
            pattern: "*".to_string(),
            action: "Deny".to_string(),
        }),
        SandboxEgress::Internet => {}
        SandboxEgress::Allow { allow } => {
            let mut targets = Vec::new();
            for target in &allow {
                let (host, port) = target.rsplit_once(':').unwrap_or((target, "443"));
                let scheme = match port { "80" => "http", "443" => "https", _ => return Err(invalid(format!("Azure native egress cannot enforce target '{target}'; only HTTP port 80 and HTTPS port 443 are supported"))) };
                if host.contains('/') {
                    return Err(invalid(format!(
                        "Azure native egress cannot enforce CIDR target '{target}'"
                    )));
                }
                targets.push((host, scheme));
            }
            // Azure wildcards include the apex. Deny it before the wildcard unless another
            // target explicitly allows that apex (possibly through a broader wildcard).
            for &(host, scheme) in &targets {
                if let Some(apex) = host.strip_prefix("*.") {
                    let apex_policy = SandboxEgress::Allow {
                        allow: vec![format!(
                            "{apex}:{}",
                            if scheme == "http" { 80 } else { 443 }
                        )],
                    };
                    if apex_policy
                        .ensure_subset_of(&SandboxEgress::Allow {
                            allow: allow.clone(),
                        })
                        .is_err()
                    {
                        policy.rules.push(rule(apex, scheme, "Deny"));
                    }
                }
            }
            policy.rules.extend(
                targets
                    .into_iter()
                    .map(|(host, scheme)| rule(host, scheme, "Allow")),
            );
        }
    }
    Ok(policy)
}

/// Read the effective policy without discarding unrecognised permissions. The service repeats
/// its HTTP policy in `http` and adds `enforcementMode`; both must agree with the outer object.
pub(super) fn effective(policy: &EgressPolicy) -> Result<SandboxEgress> {
    if !policy
        .traffic_inspection
        .as_deref()
        .is_some_and(|mode| mode.eq_ignore_ascii_case("Full"))
    {
        return Err(invalid("Azure egress inspection is not Full"));
    }
    for (key, value) in &policy.unmodelled {
        match key.as_str() {
            "enforcementMode" if value.as_str() == Some("Enforced") => {}
            "http" => {
                let nested: EgressPolicy = serde_json::from_value(value.clone())
                    .map_err(|_| invalid("unreadable native HTTP policy"))?;
                let mut outer = policy.clone();
                outer.unmodelled.remove("http");
                if nested != outer {
                    return Err(invalid("native HTTP policy differs from its outer policy"));
                }
            }
            _ => {
                return Err(invalid(format!(
                    "unknown Azure egress policy field '{key}'"
                )))
            }
        }
    }
    if policy.default_action.eq_ignore_ascii_case("Allow") {
        if policy.host_rules.is_empty() && policy.rules.is_empty() {
            return Ok(SandboxEgress::Internet);
        }
        return Err(invalid("unrecognised rules on an open native policy"));
    }
    if !policy.default_action.eq_ignore_ascii_case("Deny") {
        return Err(invalid("unknown native default action"));
    }
    let mut allow = Vec::new();
    for host in &policy.host_rules {
        match host.action.to_ascii_lowercase().as_str() {
            "deny" => {}
            "allow" if !host.pattern.contains('*') => allow.push(host.pattern.clone()),
            _ => return Err(invalid("unrecognised native host rule")),
        }
    }
    for (index, entry) in policy.rules.iter().enumerate() {
        let action = entry
            .action
            .as_ref()
            .ok_or_else(|| invalid("native rule has no action"))?;
        if action.host.is_some()
            || action.path.is_some()
            || action.scheme.is_some()
            || action.headers.is_some()
        {
            return Err(invalid("native rule transforms traffic"));
        }
        if action.action_type.eq_ignore_ascii_case("Deny") {
            continue;
        }
        if !action.action_type.eq_ignore_ascii_case("Allow") {
            return Err(invalid("unknown native rule action"));
        }
        let target = entry
            .r#match
            .as_ref()
            .ok_or_else(|| invalid("native allow rule has no match"))?;
        if target.path.is_some() || target.methods.is_some() {
            return Err(invalid("native rule has unrequested request restrictions"));
        }
        let port = match target.scheme.as_deref() {
            None | Some("https") => 443,
            Some("http") => 80,
            _ => return Err(invalid("unknown native match scheme")),
        };
        allow.push(format!("{}:{port}", target.host));
        if let Some(apex) = target.host.strip_prefix("*.") {
            // Without the earlier deny Azure also grants the apex. Include that permission in
            // the readback so the normal subset check catches a missing or reordered guard.
            let denied = policy.rules[..index].iter().any(|r| {
                r.action
                    .as_ref()
                    .is_some_and(|a| a.action_type.eq_ignore_ascii_case("Deny"))
                    && r.r#match.as_ref().is_some_and(|m| {
                        m.host == apex
                            && m.scheme == target.scheme
                            && m.path.is_none()
                            && m.methods.is_none()
                    })
            });
            if !denied {
                allow.push(format!("{apex}:{port}"));
            }
        }
    }
    let result = if allow.is_empty() {
        SandboxEgress::None
    } else {
        SandboxEgress::Allow { allow }
    };
    result.normalized().map_err(|e| {
        e.context(ErrorData::SandboxEgressInvalid {
            reason: "unrecognised native egress targets".to_string(),
        })
    })
}

pub(super) fn equivalent(asked: &EgressPolicy, actual: Option<&EgressPolicy>) -> bool {
    let (Ok(asked), Some(actual)) = (effective(asked), actual) else {
        return false;
    };
    let Ok(actual) = effective(actual) else {
        return false;
    };
    actual.ensure_subset_of(&asked).is_ok() && asked.ensure_subset_of(&actual).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow(targets: &[&str]) -> SandboxEgress {
        SandboxEgress::Allow {
            allow: targets.iter().map(|v| (*v).to_string()).collect(),
        }
    }

    #[test]
    fn live_native_http_mirror_is_checked_and_unknown_permissions_are_refused() {
        let asked = policy(&allow(&["example.com:80"])).unwrap();
        let mut actual = asked.clone();
        actual
            .unmodelled
            .insert("enforcementMode".to_string(), "Enforced".into());
        actual
            .unmodelled
            .insert("http".to_string(), serde_json::to_value(&actual).unwrap());
        assert!(equivalent(&asked, Some(&actual)));
        actual.unmodelled.get_mut("http").unwrap()["defaultAction"] = "Allow".into();
        assert!(effective(&actual).is_err());
        actual.unmodelled.remove("http");
        actual.unmodelled.insert(
            "tcp".to_string(),
            serde_json::json!({"defaultAction":"Allow"}),
        );
        assert!(effective(&actual).is_err());
    }

    #[test]
    fn a_missing_or_late_apex_deny_is_a_wider_policy() {
        let bound = allow(&["*.example.org"]);
        let mut actual = policy(&bound).unwrap();
        assert!(effective(&actual).unwrap().ensure_subset_of(&bound).is_ok());
        actual.rules.swap(0, 1);
        assert!(effective(&actual)
            .unwrap()
            .ensure_subset_of(&bound)
            .is_err());
        actual.rules.pop();
        assert!(effective(&actual)
            .unwrap()
            .ensure_subset_of(&bound)
            .is_err());
    }

    #[test]
    fn overlapping_wildcards_preserve_explicitly_allowed_apices() {
        for bound in [
            allow(&["*.example.org", "*.nested.example.org"]),
            allow(&["*.example.org", "example.org"]),
        ] {
            let native = policy(&bound).unwrap();
            let actual = effective(&native).unwrap();
            assert!(actual.ensure_subset_of(&bound).is_ok());
            assert!(bound.ensure_subset_of(&actual).is_ok());
        }
    }

    #[test]
    fn http_and_https_have_distinct_permissions() {
        let http = policy(&allow(&["example.com:80"])).unwrap();
        let https = policy(&allow(&["example.com"])).unwrap();
        assert!(!equivalent(&http, Some(&https)));
        assert!(!equivalent(&https, Some(&http)));
        assert!(policy(&allow(&["example.com:8443"])).is_err());
        assert!(policy(&allow(&["8.8.8.0/24"])).is_err());
    }

    #[test]
    fn transform_rules_and_audit_only_enforcement_are_refused() {
        let mut actual = policy(&allow(&["example.com"])).unwrap();
        actual.rules[0].action.as_mut().unwrap().headers = Some(vec![]);
        assert!(effective(&actual).is_err());
        actual.rules[0].action.as_mut().unwrap().headers = None;
        actual
            .unmodelled
            .insert("enforcementMode".to_string(), "Audit".into());
        assert!(effective(&actual).is_err());
    }
}
