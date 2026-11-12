use alien_extension_sdk::prelude::*;

/// The tags every interceptor adds.
pub const TAGS: &[(&str, &str)] = &[("team", "payments"), ("managed-by", "acme")];

/// Every Kubernetes object has metadata.labels, so this matches every create.
pub struct KubernetesLabels;

impl KubernetesInterceptor for KubernetesLabels {
    fn matches(&self, call: &KubernetesCall) -> bool {
        call.verb() == "create"
    }

    fn modify_request(&self, object: &mut KubernetesObject) -> Result<()> {
        let labels = &mut object.body_mut()["metadata"]["labels"];
        for (key, value) in TAGS {
            labels[*key] = json!(value);
        }
        Ok(())
    }
}
