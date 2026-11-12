use alien_extension_sdk::prelude::*;
use std::collections::BTreeMap;

/// Lets the chart's pods send traffic only inside the cluster.
pub struct NetworkPolicy;

impl HelmModifier for NetworkPolicy {
    fn templates(&self, _chart: &HelmChart) -> Result<BTreeMap<String, String>> {
        let policy = r#"apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: acme-deny-egress
  namespace: {{ .Release.Namespace }}
spec:
  podSelector: {}
  policyTypes: [Egress]
  egress:
    - to:
        - namespaceSelector: {}
"#;
        Ok(BTreeMap::from([(
            "acme-deny-egress.yaml".into(),
            policy.into(),
        )]))
    }
}
