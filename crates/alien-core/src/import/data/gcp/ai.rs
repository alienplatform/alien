use serde::{Deserialize, Serialize};

/// GCP AI (Vertex AI) ImportData.
///
/// The `project_id` and `location` fields identify the Vertex AI endpoint and
/// are carried directly into the controller's ready state so that heartbeat
/// ticks and binding-param serialization work without a cloud round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpAiImportData {
    /// GCP project ID that owns the Vertex AI endpoint.
    pub project_id: String,
    /// GCP region (location) of the Vertex AI endpoint.
    pub location: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The GCP AI Terraform emitter writes the import ref with key `projectId`
    /// (`crates/alien-terraform/src/emitters/gcp/ai.rs`), matching every other GCP
    /// resource. This pins the serde key so a frozen/Terraform import round-trip does not
    /// fail with `missing field project`.
    #[test]
    fn deserializes_the_emitter_import_ref_key() {
        let json = serde_json::json!({ "projectId": "my-project", "location": "us-central1" });
        let data: GcpAiImportData =
            serde_json::from_value(json).expect("emitter's projectId key must deserialize");
        assert_eq!(data.project_id, "my-project");
        assert_eq!(data.location, "us-central1");
    }
}
