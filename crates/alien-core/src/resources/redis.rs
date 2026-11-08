use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef};
use crate::ResourceType;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;

/// What a Redis server does when it reaches its memory limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RedisEvictionPolicy {
    /// Reject writes when memory is full.
    #[serde(rename = "noeviction")]
    NoEviction,
    /// Evict the least recently used key, whether or not it has a TTL.
    #[serde(rename = "allkeys-lru")]
    AllKeysLru,
    /// Evict the least frequently used key, whether or not it has a TTL.
    #[serde(rename = "allkeys-lfu")]
    AllKeysLfu,
    /// Evict a random key, whether or not it has a TTL.
    #[serde(rename = "allkeys-random")]
    AllKeysRandom,
    /// Evict the least recently used key among keys with a TTL.
    #[default]
    #[serde(rename = "volatile-lru")]
    VolatileLru,
    /// Evict the least frequently used key among keys with a TTL.
    #[serde(rename = "volatile-lfu")]
    VolatileLfu,
    /// Evict a random key among keys with a TTL.
    #[serde(rename = "volatile-random")]
    VolatileRandom,
    /// Evict the key with the shortest remaining TTL.
    #[serde(rename = "volatile-ttl")]
    VolatileTtl,
}

impl RedisEvictionPolicy {
    /// The value of the `maxmemory-policy` setting on every platform.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoEviction => "noeviction",
            Self::AllKeysLru => "allkeys-lru",
            Self::AllKeysLfu => "allkeys-lfu",
            Self::AllKeysRandom => "allkeys-random",
            Self::VolatileLru => "volatile-lru",
            Self::VolatileLfu => "volatile-lfu",
            Self::VolatileRandom => "volatile-random",
            Self::VolatileTtl => "volatile-ttl",
        }
    }
}

/// A managed, Redis-compatible in-memory store on a private network. The target
/// platform decides the backend (AWS ElastiCache for Valkey, GCP Memorystore for
/// Valkey, Azure Managed Redis, or a native Valkey process on Local). It is
/// never publicly reachable, and it is a single endpoint (not cluster mode), so
/// plain clients and multi-key scripts work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Redis {
    #[builder(start_fn)]
    pub id: String,
    /// Memory available for data (e.g. "512Mi", "4Gi"). None = the smallest
    /// production tier. The smallest tier that fits is used; a request larger
    /// than the biggest single-shard tier on the platform is rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    /// What happens at the memory limit. Default `volatile-lru`: keys without
    /// a TTL are never evicted.
    #[builder(default)]
    #[serde(default)]
    pub eviction_policy: RedisEvictionPolicy,
    /// Keep data across a server restart or maintenance. Default false (a
    /// cache). On AWS, GCP and Azure this needs `high_availability`, because the
    /// clouds only persist data from a replicated deployment. Local always
    /// persists with an append-only file when this is on.
    #[builder(default)]
    #[serde(default)]
    pub persistence: bool,
    /// One replica in another zone with automatic failover. Default false.
    #[builder(default)]
    #[serde(default)]
    pub high_availability: bool,
    /// Customer-managed key for encryption at rest. Cannot be changed after
    /// creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<ResourceRef>,
}

/// ElastiCache node types in ascending order with their published node memory in GiB.
/// One table for the controller, the CloudFormation emitter and the Terraform emitter.
pub const ELASTICACHE_NODE_TYPES: &[(&str, f64)] = &[
    ("cache.t4g.small", 1.37),
    ("cache.t4g.medium", 3.09),
    ("cache.m7g.large", 6.38),
    ("cache.m7g.xlarge", 12.93),
    ("cache.r7g.xlarge", 26.32),
    ("cache.r7g.2xlarge", 52.82),
    ("cache.r7g.4xlarge", 105.81),
    ("cache.r7g.8xlarge", 209.55),
    ("cache.r7g.16xlarge", 419.09),
];

/// Memorystore for Valkey node types in ascending order with their default usable memory in
/// GB (decimal), from the published node specification. The CUSTOM_* types are left out: they
/// exist only for cluster-disabled instances and have not been exercised against a real project.
pub const MEMORYSTORE_NODE_TYPES: &[(&str, f64)] = &[
    ("SHARED_CORE_NANO", 1.12),
    ("STANDARD_SMALL", 5.2),
    ("HIGHMEM_MEDIUM", 10.4),
    ("STANDARD_LARGE", 20.8),
    ("HIGHMEM_XLARGE", 46.4),
    ("HIGHMEM_2XLARGE", 88.0),
];

impl Redis {
    /// The resource type identifier for Redis.
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("redis");

    /// Requested memory in bytes, or None for the default tier. Uses the same
    /// parser as every other sized resource, so all controllers and emitters
    /// read a quantity the same way. A bare number is GiB (as for Postgres).
    pub fn requested_memory_bytes(&self) -> Result<Option<u64>> {
        self.memory
            .as_deref()
            .map(|value| {
                let quantity = value.trim();
                if let Ok(gib) = quantity.parse::<f64>() {
                    if gib.is_finite() && gib > 0.0 {
                        return Ok((gib * 1024_f64.powi(3)).ceil() as u64);
                    }
                    return Err(self.invalid_memory(value));
                }
                // Accept the GiB/GB spellings as aliases of Gi/G.
                let quantity = quantity.strip_suffix('B').unwrap_or(quantity);
                crate::instance_catalog::parse_memory_bytes(quantity)
                    .ok()
                    .filter(|bytes| *bytes > 0)
                    .ok_or_else(|| self.invalid_memory(value))
            })
            .transpose()
    }

    /// The smallest ElastiCache node type whose usable data memory fits the request, and
    /// that usable memory in bytes. Usable memory is 75% of the node's memory because
    /// ElastiCache reserves 25% by default (`reserved-memory-percent`). With no request the
    /// default is 1 GiB. Requests above the largest single-shard node are rejected, never capped.
    pub fn elasticache_node_type(&self) -> Result<(&'static str, u64)> {
        let wanted = self.requested_memory_bytes()?.unwrap_or(1024_u64.pow(3));
        ELASTICACHE_NODE_TYPES
            .iter()
            .map(|(name, gib)| (*name, (gib * 0.75 * 1024_f64.powi(3)) as u64))
            .find(|(_, usable)| *usable >= wanted)
            .ok_or_else(|| {
                AlienError::new(ErrorData::OperationNotSupported {
                    operation: "Redis memory sizing".to_string(),
                    reason: format!(
                        "{} bytes exceeds the largest single-shard ElastiCache node ({})",
                        wanted,
                        ELASTICACHE_NODE_TYPES[ELASTICACHE_NODE_TYPES.len() - 1].0
                    ),
                })
            })
    }

    /// The smallest Memorystore node type whose default usable memory fits the request, and
    /// that usable memory in bytes. With no request the default is 1 GiB. Requests above the
    /// largest node are rejected, never capped.
    pub fn memorystore_node_type(&self) -> Result<(&'static str, u64)> {
        let wanted = self.requested_memory_bytes()?.unwrap_or(1024_u64.pow(3));
        MEMORYSTORE_NODE_TYPES
            .iter()
            .map(|(name, gb)| (*name, (gb * 1e9) as u64))
            .find(|(_, usable)| *usable >= wanted)
            .ok_or_else(|| {
                AlienError::new(ErrorData::OperationNotSupported {
                    operation: "Redis memory sizing".to_string(),
                    reason: format!(
                        "{} bytes exceeds the largest Memorystore node ({})",
                        wanted,
                        MEMORYSTORE_NODE_TYPES[MEMORYSTORE_NODE_TYPES.len() - 1].0
                    ),
                })
            })
    }

    fn invalid_memory(&self, value: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: "Redis memory sizing".to_string(),
            reason: format!(
                "'{value}' must be a positive quantity with a unit, such as 512Mi or 4Gi"
            ),
        })
    }

    /// Returns the Redis resource's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Outputs generated by a successfully provisioned Redis server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RedisOutputs {
    /// Host the server listens on; never publicly reachable.
    pub endpoint: String,
    /// Listening port.
    pub port: u16,
}

impl ResourceOutputsDefinition for RedisOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Redis::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<RedisOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceDefinition for Redis {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn replace_after_failed_create_is_safe(&self) -> bool {
        // Replacing it deletes the data it holds.
        false
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        // The network dependency is added by infrastructure preflights on cloud platforms and
        // is irrelevant on Local; cloud controllers reach the network via
        // require_dependency() at runtime.
        self.encryption_key.iter().cloned().collect()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_redis = new_config.as_any().downcast_ref::<Redis>().ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResourceType {
                resource_id: self.id.clone(),
                expected: Self::RESOURCE_TYPE,
                actual: new_config.get_resource_type(),
            })
        })?;

        if self.id != new_redis.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }

        if self.encryption_key != new_redis.encryption_key {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'encryptionKey' field is immutable once the resource exists"
                    .to_string(),
            }));
        }

        // A malformed size must fail here instead of at the cloud API.
        new_redis.requested_memory_bytes().map_err(|e| {
            AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: e.to_string(),
            })
        })?;

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
        other.as_any().downcast_ref::<Redis>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_applies_defaults() {
        let redis = Redis::new("cache".to_string()).build();
        assert_eq!(redis.eviction_policy, RedisEvictionPolicy::VolatileLru);
        assert!(!redis.persistence);
        assert!(!redis.high_availability);
        assert_eq!(redis.memory, None);
    }

    #[test]
    fn json_defaults_match_builder_defaults() {
        let redis: Redis = serde_json::from_str(r#"{"id":"cache"}"#).unwrap();
        assert_eq!(redis, Redis::new("cache".to_string()).build());
    }

    #[test]
    fn eviction_policy_serializes_to_the_server_setting() {
        for policy in [
            RedisEvictionPolicy::NoEviction,
            RedisEvictionPolicy::AllKeysLru,
            RedisEvictionPolicy::AllKeysLfu,
            RedisEvictionPolicy::AllKeysRandom,
            RedisEvictionPolicy::VolatileLru,
            RedisEvictionPolicy::VolatileLfu,
            RedisEvictionPolicy::VolatileRandom,
            RedisEvictionPolicy::VolatileTtl,
        ] {
            assert_eq!(
                serde_json::to_value(policy).unwrap(),
                serde_json::Value::String(policy.as_str().to_string())
            );
        }
        assert!(serde_json::from_str::<RedisEvictionPolicy>(r#""fifo""#).is_err());
    }

    #[test]
    fn memory_units_agree() {
        for (quantity, bytes) in [
            ("1", 1u64 << 30),
            ("1.5", 3 << 29),
            ("512Mi", 512 << 20),
            ("4Gi", 4 << 30),
            ("4GiB", 4 << 30),
            ("1GB", 1_000_000_000),
        ] {
            let redis = Redis::new("c".into()).memory(quantity.into()).build();
            assert_eq!(
                redis.requested_memory_bytes().unwrap(),
                Some(bytes),
                "{quantity}"
            );
        }
        assert_eq!(
            Redis::new("c".into())
                .build()
                .requested_memory_bytes()
                .unwrap(),
            None
        );
    }

    #[test]
    fn memory_rejects_invalid_values() {
        for quantity in [
            "", "0", "0Gi", "-1Gi", "NaNGi", "infGi", "1e100Gi", "8unknown",
        ] {
            let redis = Redis::new("c".into()).memory(quantity.into()).build();
            assert!(redis.requested_memory_bytes().is_err(), "{quantity}");
        }
    }

    #[test]
    fn update_rejects_id_and_key_changes_and_bad_memory() {
        let original = Redis::new("c".to_string()).build();
        let renamed = Redis::new("d".to_string()).build();
        assert!(original
            .validate_update(&renamed)
            .unwrap_err()
            .to_string()
            .contains("'id' field is immutable"));

        let keyed = Redis::new("c".to_string())
            .encryption_key(ResourceRef::new(crate::Key::RESOURCE_TYPE, "k".to_string()))
            .build();
        assert!(original
            .validate_update(&keyed)
            .unwrap_err()
            .to_string()
            .contains("'encryptionKey' field is immutable"));

        let bad = Redis::new("c".to_string()).memory("lots".into()).build();
        assert!(original.validate_update(&bad).is_err());
        let bigger = Redis::new("c".to_string()).memory("8Gi".into()).build();
        assert!(original.validate_update(&bigger).is_ok());
    }

    #[test]
    fn outputs_roundtrip() {
        let outputs = RedisOutputs {
            endpoint: "127.0.0.1".to_string(),
            port: 6379,
        };
        let json = serde_json::to_string(&outputs).unwrap();
        assert_eq!(
            outputs,
            serde_json::from_str::<RedisOutputs>(&json).unwrap()
        );
    }

    #[test]
    fn elasticache_node_type_picks_smallest_fit_and_rejects_oversize() {
        let redis = |m: Option<&str>| match m {
            Some(m) => Redis::new("r".into()).memory(m.into()).build(),
            None => Redis::new("r".into()).build(),
        };
        assert_eq!(
            redis(None).elasticache_node_type().unwrap().0,
            "cache.t4g.small"
        );
        // 1.37 GiB node -> 1.0275 GiB usable: 1 GiB fits, 1.1 GiB moves up.
        assert_eq!(
            redis(Some("1Gi")).elasticache_node_type().unwrap().0,
            "cache.t4g.small"
        );
        assert_eq!(
            redis(Some("1100Mi")).elasticache_node_type().unwrap().0,
            "cache.t4g.medium"
        );
        assert_eq!(
            redis(Some("300Gi")).elasticache_node_type().unwrap().0,
            "cache.r7g.16xlarge"
        );
        assert!(redis(Some("400Gi")).elasticache_node_type().is_err());
    }

    #[test]
    fn memorystore_node_type_picks_smallest_fit_and_rejects_oversize() {
        let redis = |m: Option<&str>| match m {
            Some(m) => Redis::new("r".into()).memory(m.into()).build(),
            None => Redis::new("r".into()).build(),
        };
        assert_eq!(
            redis(None).memorystore_node_type().unwrap().0,
            "SHARED_CORE_NANO"
        );
        // 1.12 GB usable on the nano node: 1 GiB (1.07 GB) fits, 1.1 GiB (1.18 GB) does not.
        assert_eq!(
            redis(Some("1100Mi")).memorystore_node_type().unwrap().0,
            "STANDARD_SMALL"
        );
        assert_eq!(
            redis(Some("4Gi")).memorystore_node_type().unwrap().0,
            "STANDARD_SMALL"
        );
        assert_eq!(
            redis(Some("80Gi")).memorystore_node_type().unwrap().0,
            "HIGHMEM_2XLARGE"
        );
        assert!(redis(Some("100Gi")).memorystore_node_type().is_err());
    }
}
