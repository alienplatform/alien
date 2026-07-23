use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef};
use crate::ResourceType;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;

/// An Amazon OpenSearch Serverless collection (next generation).
///
/// # Experimental namespace
///
/// This is the first resource under the `experimental/` resource-type
/// namespace. Experimental resources are provider-specific (they do not
/// abstract over clouds), may change or be promoted to a portable resource
/// in a breaking way, and are only registered for the platforms they
/// support. `AwsOpenSearch` registers an AWS CloudFormation emitter only;
/// deploying it to any other platform fails with a typed
/// `ImportRegistrationMissing` error at generation time.
///
/// # What gets provisioned
///
/// The AWS emitter provisions next-generation OpenSearch Serverless:
/// a collection group (`Generation: NEXTGEN`, compute/storage decoupled)
/// plus a collection inside it, an AWS-owned-key encryption configuration,
/// a public network policy, and a data-access policy for service-account
/// roles granted `experimental/aws-opensearch/data-access`. Collection groups
/// scale to zero by default; configure a non-zero minimum capacity when the
/// workload requires predictable interactive latency.
/// The collection endpoint is public but every request must be SigV4-signed
/// and pass both IAM (`aoss:APIAccessAll`) and the data-access policy.
///
/// # Naming
///
/// The physical collection (and collection group) name is
/// `{id}-{stack-suffix}` and must satisfy the AOSS name grammar, so `id`
/// must match `[a-z][a-z0-9-]*` and be at most 23 characters. The emitter
/// rejects ids that don't fit.
///
/// # Runtime access
///
/// Workers reach the collection over HTTPS with SigV4. The SigV4 signing
/// service name for OpenSearch Serverless is `aoss` (not `es`); the runtime
/// binding payload carries `"service": "aoss"` so clients sign correctly.
/// Requests with a body must also send an `x-amz-content-sha256` header
/// (the AOSS gateway rejects body-carrying requests without it with an
/// empty 403); official OpenSearch clients with an `aoss` signer handle
/// this automatically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct AwsOpenSearch {
    /// Identifier for the collection. Becomes part of the physical collection
    /// name, so it must match `[a-z][a-z0-9-]*` and be at most 23 characters.
    #[builder(start_fn)]
    pub id: String,
    /// Workload type of the collection. Immutable once the resource exists
    /// (AWS only allows the type at collection creation). Default `Search`.
    #[builder(default)]
    #[serde(default)]
    pub collection_type: AwsOpenSearchCollectionType,
    /// Optional indexing and search OCU limits for the collection group.
    ///
    /// When omitted, AWS uses zero minimum capacity for both components, so
    /// an idle next-generation collection can scale to zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity: Option<AwsOpenSearchCapacity>,
}

/// Indexing and search capacity limits for an OpenSearch collection group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsOpenSearchCapacity {
    /// Indexing OCU bounds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexing: Option<AwsOpenSearchCapacityRange>,
    /// Search OCU bounds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<AwsOpenSearchCapacityRange>,
}

/// Minimum and maximum OCU bounds for one OpenSearch compute component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsOpenSearchCapacityRange {
    /// Minimum OCUs kept available. Zero enables scale-to-zero.
    #[cfg_attr(feature = "openapi", schema(minimum = 0, maximum = 1696))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ocu: Option<u16>,
    /// Maximum OCUs the component may scale to.
    #[cfg_attr(feature = "openapi", schema(minimum = 1, maximum = 1696))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ocu: Option<u16>,
}

/// Workload type for an OpenSearch Serverless collection.
///
/// `TIMESERIES` is intentionally not exposed: next-generation serverless
/// scale-to-zero targets search and vector workloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum AwsOpenSearchCollectionType {
    /// Full-text search collections (`SEARCH`).
    #[default]
    Search,
    /// Vector similarity search collections (`VECTORSEARCH`).
    VectorSearch,
}

impl AwsOpenSearch {
    /// The resource type identifier for AwsOpenSearch.
    ///
    /// The `experimental/` prefix marks the experimental namespace; see the
    /// struct-level docs for the convention.
    pub const RESOURCE_TYPE: ResourceType =
        ResourceType::from_static("experimental/aws-opensearch");

    /// Returns the collection's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Validates collection-group capacity values against AWS's supported OCU
    /// increments and min/max ordering.
    pub fn validate_capacity(&self) -> Result<()> {
        let Some(capacity) = &self.capacity else {
            return Ok(());
        };
        if capacity.indexing.is_none() && capacity.search.is_none() {
            return Err(invalid_capacity(
                "at least one of 'indexing' or 'search' must be configured",
            ));
        }
        if let Some(range) = capacity.indexing {
            validate_capacity_range("indexing", range)?;
        }
        if let Some(range) = capacity.search {
            validate_capacity_range("search", range)?;
        }
        Ok(())
    }
}

fn validate_capacity_range(component: &str, range: AwsOpenSearchCapacityRange) -> Result<()> {
    if range.min_ocu.is_none() && range.max_ocu.is_none() {
        return Err(invalid_capacity(format!(
            "'{component}' must configure 'minOcu' or 'maxOcu'"
        )));
    }
    if let Some(min) = range.min_ocu {
        if min != 0 && !valid_nonzero_ocu(min) {
            return Err(invalid_capacity(format!(
                "'{component}.minOcu' value {min} is unsupported"
            )));
        }
    }
    if let Some(max) = range.max_ocu {
        if !valid_nonzero_ocu(max) {
            return Err(invalid_capacity(format!(
                "'{component}.maxOcu' value {max} is unsupported"
            )));
        }
    }
    if let (Some(min), Some(max)) = (range.min_ocu, range.max_ocu) {
        if min > max {
            return Err(invalid_capacity(format!(
                "'{component}.minOcu' ({min}) must be less than or equal to \
                 '{component}.maxOcu' ({max})"
            )));
        }
    }
    Ok(())
}

fn valid_nonzero_ocu(value: u16) -> bool {
    matches!(value, 1 | 2 | 4 | 8 | 16) || (value >= 32 && value <= 1696 && value % 16 == 0)
}

fn invalid_capacity(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::GenericError {
        message: format!("AwsOpenSearch capacity is invalid: {}", message.into()),
    })
}

/// Outputs generated by a successfully provisioned AwsOpenSearch collection.
///
/// Next-generation collections expose no OpenSearch Dashboards endpoint
/// (the `DashboardEndpoint` attribute exists only for classic collections),
/// so only the data-plane endpoint and ARN are surfaced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsOpenSearchOutputs {
    /// Collection endpoint (`https://<collectionId>.aoss.<region>.on.aws`).
    /// Requests must be SigV4-signed with service name `aoss`.
    pub endpoint: String,
    /// ARN of the collection.
    pub collection_arn: String,
}

impl ResourceOutputsDefinition for AwsOpenSearchOutputs {
    fn get_resource_type(&self) -> ResourceType {
        AwsOpenSearch::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<AwsOpenSearchOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceDefinition for AwsOpenSearch {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        Vec::new()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_search = new_config
            .as_any()
            .downcast_ref::<AwsOpenSearch>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        if self.id != new_search.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }

        // AWS only accepts the collection type at creation; changing it would
        // silently require replacing the collection and dropping every index.
        if self.collection_type != new_search.collection_type {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'collectionType' field is immutable once the resource exists"
                    .to_string(),
            }));
        }

        new_search.validate_capacity()?;

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<AwsOpenSearch>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_defaults_to_search() {
        let search = AwsOpenSearch::new("search".to_string()).build();
        assert_eq!(search.id, "search");
        assert_eq!(search.collection_type, AwsOpenSearchCollectionType::Search);
        assert!(search.capacity.is_none());
    }

    #[test]
    fn resource_type_uses_experimental_namespace() {
        assert_eq!(
            AwsOpenSearch::RESOURCE_TYPE.as_ref(),
            "experimental/aws-opensearch"
        );
    }

    #[test]
    fn validate_update_rejects_id_change() {
        let original = AwsOpenSearch::new("search".to_string()).build();
        let renamed = AwsOpenSearch::new("other".to_string()).build();
        let err = original
            .validate_update(&renamed)
            .expect_err("changing the id must be rejected");
        assert!(err.to_string().contains("'id' field is immutable"));
    }

    #[test]
    fn validate_update_rejects_collection_type_change() {
        let search = AwsOpenSearch::new("search".to_string()).build();
        let vector = AwsOpenSearch::new("search".to_string())
            .collection_type(AwsOpenSearchCollectionType::VectorSearch)
            .build();

        let err = search
            .validate_update(&vector)
            .expect_err("changing the collection type must be rejected");
        assert!(err
            .to_string()
            .contains("'collectionType' field is immutable"));
        // A no-op update is allowed.
        assert!(vector.validate_update(&vector).is_ok());
    }

    #[test]
    fn serializes_with_camel_case_and_roundtrips() {
        let search = AwsOpenSearch::new("vectors".to_string())
            .collection_type(AwsOpenSearchCollectionType::VectorSearch)
            .build();
        let json = serde_json::to_value(&search).unwrap();
        assert_eq!(json["collectionType"], "vectorSearch");

        let roundtrip: AwsOpenSearch = serde_json::from_value(json).unwrap();
        assert_eq!(search, roundtrip);
    }

    #[test]
    fn capacity_accepts_scale_to_zero_and_supported_nonzero_values() {
        let search = AwsOpenSearch::new("search".to_string())
            .capacity(AwsOpenSearchCapacity {
                indexing: Some(AwsOpenSearchCapacityRange {
                    min_ocu: Some(0),
                    max_ocu: Some(1696),
                }),
                search: Some(AwsOpenSearchCapacityRange {
                    min_ocu: Some(1),
                    max_ocu: Some(32),
                }),
            })
            .build();

        search
            .validate_capacity()
            .expect("capacity should be valid");
        let json = serde_json::to_value(&search).expect("capacity should serialize");
        assert_eq!(json["capacity"]["indexing"]["minOcu"], 0);
        assert_eq!(json["capacity"]["search"]["maxOcu"], 32);
    }

    #[test]
    fn capacity_rejects_empty_unsupported_and_inverted_ranges() {
        let cases = [
            AwsOpenSearchCapacity {
                indexing: None,
                search: None,
            },
            AwsOpenSearchCapacity {
                indexing: Some(AwsOpenSearchCapacityRange {
                    min_ocu: Some(3),
                    max_ocu: None,
                }),
                search: None,
            },
            AwsOpenSearchCapacity {
                indexing: None,
                search: Some(AwsOpenSearchCapacityRange {
                    min_ocu: Some(8),
                    max_ocu: Some(4),
                }),
            },
        ];

        for capacity in cases {
            let search = AwsOpenSearch::new("search".to_string())
                .capacity(capacity)
                .build();
            let error = search
                .validate_capacity()
                .expect_err("invalid capacity must fail");
            assert_eq!(error.code, "GENERIC_ERROR");
            assert!(error.to_string().contains("capacity is invalid"));
        }
    }

    #[test]
    fn outputs_roundtrip() {
        let outputs = AwsOpenSearchOutputs {
            endpoint: "https://abc123.aoss.us-east-1.on.aws".to_string(),
            collection_arn: "arn:aws:aoss:us-east-1:123456789012:collection/abc123".to_string(),
        };
        let json = serde_json::to_string(&outputs).unwrap();
        let deserialized: AwsOpenSearchOutputs = serde_json::from_str(&json).unwrap();
        assert_eq!(outputs, deserialized);
    }
}
