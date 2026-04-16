#![cfg(feature = "grpc")]

use crate::grpc::storage_service::alien_bindings::storage::StorageGetRequest;
use crate::grpc::storage_service::alien_bindings::storage::{
    storage_get_range_option::RangeType as ProtoGetRangeOptionType, StorageAttributeKeyValuePair,
    StorageAttributesMap as ProtoAttributesMap, StorageGetOptions as ProtoGetOptions,
    StorageGetRangeOption as ProtoGetRangeOption, StorageHttpMethod as ProtoHttpMethod,
    StorageObjectMeta as ProtoObjectMeta, StoragePutModeEnum as ProtoPutModeEnum,
    StoragePutMultipartOptions as ProtoPutMultipartOptions, StoragePutOptions as ProtoPutOptions,
    StorageRange as ProtoRange, StorageSignedUrlRequest, StorageSignedUrlResponse,
    StorageTag as ProtoTag, StorageTagSet as ProtoTagSet,
    StorageUpdateVersion as ProtoUpdateVersion,
};

use object_store::{
    path::Path, Attribute as OsAttribute, Attributes as OsAttributes, Error as ObjectStoreError,
    GetOptions, GetRange as OsGetRange, ObjectMeta, PutMode as OsPutMode, PutMultipartOpts,
    PutOptions, TagSet as OsTagSet, UpdateVersion as OsUpdateVersion,
};
use prost_types::Timestamp;
use std::borrow::Cow;
use std::ops::Range as StdRange;
use tonic::Status;

// --- Error Mapping ---
pub(crate) fn map_status_to_os_error(status: Status, path: Option<String>) -> ObjectStoreError {
    let path_str = path.unwrap_or_else(|| "unknown_path".to_string());
    match status.code() {
        tonic::Code::NotFound => ObjectStoreError::NotFound {
            path: path_str,
            source: status.message().into(),
        },
        tonic::Code::AlreadyExists => ObjectStoreError::AlreadyExists {
            path: path_str,
            source: status.message().into(),
        },
        tonic::Code::InvalidArgument => ObjectStoreError::Generic {
            store: "gRPC",
            source: format!(
                "Invalid argument for path '{}': {}",
                path_str,
                status.message()
            )
            .into(),
        },
        tonic::Code::PermissionDenied => ObjectStoreError::PermissionDenied {
            path: path_str,
            source: status.message().into(),
        },
        tonic::Code::Unauthenticated => ObjectStoreError::Unauthenticated {
            path: path_str,
            source: status.message().into(),
        },
        tonic::Code::FailedPrecondition => ObjectStoreError::Precondition {
            path: path_str,
            source: status.message().into(),
        },
        tonic::Code::Aborted if status.message().contains("Not modified") => {
            // Heuristic, relies on server message
            ObjectStoreError::NotModified {
                path: path_str,
                source: status.message().into(),
            }
        }
        tonic::Code::Unimplemented => ObjectStoreError::NotImplemented,
        _ => ObjectStoreError::Generic {
            store: "gRPC",
            source: status.into(),
        },
    }
}

// --- Proto to OS Mapping ---
pub(crate) fn map_proto_object_meta_to_os(
    proto_meta: ProtoObjectMeta,
) -> Result<ObjectMeta, ObjectStoreError> {
    let last_modified = match proto_meta.last_modified {
        Some(ts) => {
            chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).ok_or_else(|| {
                ObjectStoreError::Generic {
                    store: "gRPC",
                    source: "Invalid timestamp in ProtoObjectMeta".into(),
                }
            })?
        }
        None => chrono::DateTime::from_timestamp(0, 0).unwrap(), // Default if not present
    };
    Ok(ObjectMeta {
        location: Path::from(proto_meta.location),
        last_modified,
        size: proto_meta.size,
        e_tag: proto_meta.e_tag,
        version: proto_meta.version,
    })
}

// --- OS to Proto Mapping (for requests) ---

fn map_os_get_range_to_proto(os_range: OsGetRange) -> Option<ProtoGetRangeOption> {
    match os_range {
        OsGetRange::Bounded(r) => Some(ProtoGetRangeOption {
            range_type: Some(ProtoGetRangeOptionType::Bounded(ProtoRange {
                start: r.start,
                end: r.end,
            })),
        }),
        OsGetRange::Offset(o) => Some(ProtoGetRangeOption {
            range_type: Some(ProtoGetRangeOptionType::OffsetFromStart(o)),
        }),
        OsGetRange::Suffix(s) => Some(ProtoGetRangeOption {
            range_type: Some(ProtoGetRangeOptionType::SuffixLength(s)),
        }),
    }
}

fn is_get_options_default(os_opts: &GetOptions) -> bool {
    os_opts.if_match.is_none()
        && os_opts.if_none_match.is_none()
        && os_opts.if_modified_since.is_none()
        && os_opts.if_unmodified_since.is_none()
        && os_opts.range.is_none()
        && os_opts.version.is_none()
        && !os_opts.head // head default is false
}

pub(crate) fn map_os_get_options_to_proto_options_type(
    os_opts: GetOptions,
) -> Option<ProtoGetOptions> {
    if is_get_options_default(&os_opts) {
        // Optimization: don't send default options
        return None;
    }
    Some(ProtoGetOptions {
        if_match: os_opts.if_match,
        if_none_match: os_opts.if_none_match,
        if_modified_since: os_opts.if_modified_since.map(|dt| Timestamp {
            seconds: dt.timestamp(),
            nanos: dt.timestamp_subsec_nanos() as i32,
        }),
        if_unmodified_since: os_opts.if_unmodified_since.map(|dt| Timestamp {
            seconds: dt.timestamp(),
            nanos: dt.timestamp_subsec_nanos() as i32,
        }),
        range: os_opts.range.and_then(map_os_get_range_to_proto),
        version: os_opts.version,
        head: os_opts.head,
    })
}

pub(crate) fn map_os_get_options_to_proto_request(
    os_opts: GetOptions,
    binding_name: String,
    path: String,
) -> StorageGetRequest {
    StorageGetRequest {
        binding_name,
        path,
        options: map_os_get_options_to_proto_options_type(os_opts),
    }
}

fn map_os_put_mode_to_proto(
    os_mode: object_store::PutMode,
) -> (ProtoPutModeEnum, Option<ProtoUpdateVersion>) {
    match os_mode {
        object_store::PutMode::Overwrite => (ProtoPutModeEnum::PutModeOverwrite, None),
        object_store::PutMode::Create => (ProtoPutModeEnum::PutModeCreate, None),
        object_store::PutMode::Update(uv) => (
            ProtoPutModeEnum::PutModeUpdate,
            Some(ProtoUpdateVersion {
                e_tag: uv.e_tag,
                version: uv.version,
            }),
        ),
    }
}

fn map_os_tag_set_to_proto(os_tags: OsTagSet) -> Option<ProtoTagSet> {
    let encoded_tags = os_tags.encoded();
    if encoded_tags.is_empty() {
        return None;
    }

    let proto_tags: Vec<ProtoTag> = url::form_urlencoded::parse(encoded_tags.as_bytes())
        .map(|(key, value)| ProtoTag {
            key: key.into_owned(),
            value: value.into_owned(),
        })
        .collect();

    if proto_tags.is_empty() {
        // This case might occur if encoded_tags was not empty but parsing resulted in no pairs.
        // This path ensures that we don't send an empty ProtoTagSet, aligning with the
        // optimization to send None for default/empty options.
        None
    } else {
        Some(ProtoTagSet { tags: proto_tags })
    }
}

fn map_os_attributes_to_proto(os_attrs: OsAttributes) -> Option<ProtoAttributesMap> {
    if os_attrs.is_empty() {
        return None;
    }
    let pairs: Vec<StorageAttributeKeyValuePair> = os_attrs.iter().filter_map(|(attr, value)| {
        let key = match attr {
            OsAttribute::ContentDisposition => "content-disposition".to_string(),
            OsAttribute::ContentEncoding => "content-encoding".to_string(),
            OsAttribute::ContentLanguage => "content-language".to_string(),
            OsAttribute::ContentType => "content-type".to_string(),
            OsAttribute::CacheControl => "cache-control".to_string(),
            OsAttribute::Metadata(cow_key) => format!("metadata:{}", cow_key),
            #[allow(unreachable_patterns)]
            _ => {
                eprintln!("Warning: Unhandled OsAttribute variant encountered during proto mapping: {:?}", attr);
                return None;
            }
        };
        // object_store::AttributeValue wraps a Cow<'static, str>.
        // We can get its content using as_ref().
        let value_str = value.as_ref().to_string();
        Some(StorageAttributeKeyValuePair{ key, value: value_str })
    }).collect();

    if pairs.is_empty() {
        None
    } else {
        Some(ProtoAttributesMap { pairs })
    }
}

pub(crate) fn map_os_put_options_to_proto(os_opts: PutOptions) -> Option<ProtoPutOptions> {
    let is_default_mode = os_opts.mode == object_store::PutMode::default();
    // For TagSet (0.11.2), default is an empty encoded string.
    let are_tags_default = os_opts.tags == OsTagSet::default();
    let are_attributes_empty = os_opts.attributes.is_empty();

    if is_default_mode && are_tags_default && are_attributes_empty {
        return None;
    }

    let (mode_enum, update_details) = map_os_put_mode_to_proto(os_opts.mode);

    Some(ProtoPutOptions {
        mode: mode_enum.into(),
        update_version_details: update_details,
        // Since map_os_tag_set_to_proto currently always returns None, this will be None.
        tags: if are_tags_default {
            None
        } else {
            map_os_tag_set_to_proto(os_opts.tags)
        },
        attributes: if are_attributes_empty {
            None
        } else {
            map_os_attributes_to_proto(os_opts.attributes)
        },
    })
}

pub(crate) fn map_os_put_multipart_opts_to_proto(
    os_opts: PutMultipartOpts,
) -> Option<ProtoPutMultipartOptions> {
    let are_tags_default = os_opts.tags == OsTagSet::default();
    let are_attributes_empty = os_opts.attributes.is_empty();

    if are_tags_default && are_attributes_empty {
        return None;
    }
    Some(ProtoPutMultipartOptions {
        // Since map_os_tag_set_to_proto currently always returns None, this will be None.
        tags: if are_tags_default {
            None
        } else {
            map_os_tag_set_to_proto(os_opts.tags)
        },
        attributes: if are_attributes_empty {
            None
        } else {
            map_os_attributes_to_proto(os_opts.attributes)
        },
    })
}

// --- Proto to OS Mapping (for server-side request processing & client-side response processing) ---

pub(crate) fn map_proto_get_options_to_os(proto_opts: ProtoGetOptions) -> GetOptions {
    GetOptions {
        if_match: proto_opts.if_match,
        if_none_match: proto_opts.if_none_match,
        if_modified_since: proto_opts.if_modified_since.map(|ts| {
            chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default()
        }),
        if_unmodified_since: proto_opts.if_unmodified_since.map(|ts| {
            chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default()
        }),
        range: proto_opts.range.and_then(map_proto_get_range_option_to_os),
        version: proto_opts.version,
        head: proto_opts.head,
        extensions: Default::default(),
    }
}

pub(crate) fn map_proto_get_range_option_to_os(
    proto_range_opt: ProtoGetRangeOption,
) -> Option<OsGetRange> {
    match proto_range_opt.range_type {
        Some(ProtoGetRangeOptionType::Bounded(r)) => Some(OsGetRange::Bounded(StdRange {
            start: r.start,
            end: r.end,
        })),
        Some(ProtoGetRangeOptionType::OffsetFromStart(o)) => Some(OsGetRange::Offset(o)),
        Some(ProtoGetRangeOptionType::SuffixLength(s)) => Some(OsGetRange::Suffix(s)),
        None => None,
    }
}

pub(crate) fn map_proto_put_options_to_os(proto_opts: ProtoPutOptions) -> PutOptions {
    let mode = match proto_opts.mode() {
        ProtoPutModeEnum::PutModeOverwrite => OsPutMode::Overwrite,
        ProtoPutModeEnum::PutModeCreate => OsPutMode::Create,
        ProtoPutModeEnum::PutModeUpdate => {
            if let Some(v) = proto_opts.update_version_details {
                OsPutMode::Update(OsUpdateVersion {
                    e_tag: v.e_tag,
                    version: v.version,
                })
            } else {
                OsPutMode::Overwrite
            }
        }
    };
    PutOptions {
        mode,
        tags: proto_opts
            .tags
            .map_or_else(OsTagSet::default, map_proto_tag_set_to_os),
        attributes: map_proto_attributes_to_os(proto_opts.attributes),
        extensions: Default::default(),
    }
}

pub(crate) fn map_proto_put_multipart_options_to_os(
    proto_opts: ProtoPutMultipartOptions,
) -> PutMultipartOpts {
    PutMultipartOpts {
        tags: proto_opts
            .tags
            .map_or_else(OsTagSet::default, map_proto_tag_set_to_os),
        attributes: map_proto_attributes_to_os(proto_opts.attributes),
        extensions: Default::default(),
    }
}

pub(crate) fn map_proto_tag_set_to_os(proto_tags: ProtoTagSet) -> OsTagSet {
    let mut os_tags = OsTagSet::default();
    for tag in proto_tags.tags {
        os_tags.push(&tag.key, &tag.value);
    }
    os_tags
}

pub(crate) fn map_proto_attributes_to_os(
    proto_attrs_map: Option<ProtoAttributesMap>,
) -> OsAttributes {
    let mut os_attributes = OsAttributes::new();
    if let Some(map) = proto_attrs_map {
        for pair in map.pairs {
            let key_str = pair.key.to_lowercase();
            let value = object_store::AttributeValue::from(pair.value); // Use object_store::AttributeValue directly

            let _ = match key_str.as_str() {
                "content-disposition" => {
                    os_attributes.insert(OsAttribute::ContentDisposition, value)
                }
                "content-encoding" => os_attributes.insert(OsAttribute::ContentEncoding, value),
                "content-language" => os_attributes.insert(OsAttribute::ContentLanguage, value),
                "content-type" => os_attributes.insert(OsAttribute::ContentType, value),
                "cache-control" => os_attributes.insert(OsAttribute::CacheControl, value),
                s if s.starts_with("metadata:") => {
                    if let Some(metadata_key) = s.strip_prefix("metadata:") {
                        if !metadata_key.is_empty() {
                            os_attributes.insert(
                                OsAttribute::Metadata(Cow::Owned(metadata_key.to_string())),
                                value,
                            )
                        } else {
                            eprintln!(
                                "Warning: Empty metadata key after 'metadata:' prefix for key '{}'",
                                pair.key
                            );
                            None
                        }
                    } else {
                        eprintln!("Warning: Could not strip 'metadata:' prefix from key '{}', though it starts with it.", pair.key);
                        None
                    }
                }
                _ => {
                    eprintln!(
                        "Warning: Unknown attribute key '{}' not mapped to object_store::Attribute. Consider using a standard key or prefixing with 'metadata:' for user-defined attributes.",
                        pair.key
                    );
                    None
                }
            };
        }
    }
    os_attributes
}

// --- OS to Proto Mapping (for server-side response generation) ---

pub(crate) fn map_os_object_meta_to_proto(meta: ObjectMeta) -> ProtoObjectMeta {
    ProtoObjectMeta {
        location: meta.location.to_string(),
        last_modified: Some(Timestamp {
            seconds: meta.last_modified.timestamp(),
            nanos: meta.last_modified.timestamp_subsec_nanos() as i32,
        }),
        size: meta.size,
        e_tag: meta.e_tag,
        version: meta.version,
    }
}

// --- HTTP Method Conversion ---

use reqwest::Method;

pub(crate) fn map_reqwest_method_to_proto(method: Method) -> ProtoHttpMethod {
    match method {
        Method::GET => ProtoHttpMethod::HttpMethodGet,
        Method::PUT => ProtoHttpMethod::HttpMethodPut,
        Method::POST => ProtoHttpMethod::HttpMethodPost,
        Method::DELETE => ProtoHttpMethod::HttpMethodDelete,
        Method::HEAD => ProtoHttpMethod::HttpMethodHead,
        _ => ProtoHttpMethod::HttpMethodGet, // Default to GET for unsupported methods
    }
}

pub(crate) fn map_proto_method_to_reqwest(proto_method: ProtoHttpMethod) -> Method {
    match proto_method {
        ProtoHttpMethod::HttpMethodGet => Method::GET,
        ProtoHttpMethod::HttpMethodPut => Method::PUT,
        ProtoHttpMethod::HttpMethodPost => Method::POST,
        ProtoHttpMethod::HttpMethodDelete => Method::DELETE,
        ProtoHttpMethod::HttpMethodHead => Method::HEAD,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike as _;
    use object_store::path::Path; // Already imported at top level, but good practice for test module
    use object_store::Error as ObjectStoreError;
    use tonic::Status;

    #[test]
    fn test_map_status_to_os_error_not_found() {
        let status = Status::new(tonic::Code::NotFound, "entity not found");
        let path = "test/path/object.txt".to_string();
        let os_error = map_status_to_os_error(status, Some(path.clone()));
        match os_error {
            ObjectStoreError::NotFound {
                path: p,
                source: _s,
            } => {
                assert_eq!(p, path);
            }
            _ => panic!("Expected ObjectStoreError::NotFound, got {:?}", os_error),
        }
    }

    #[test]
    fn test_map_status_to_os_error_already_exists() {
        let status = Status::new(tonic::Code::AlreadyExists, "entity already there");
        let path = "test/path/object.txt".to_string();
        let os_error = map_status_to_os_error(status, Some(path.clone()));
        match os_error {
            ObjectStoreError::AlreadyExists {
                path: p,
                source: _s,
            } => {
                assert_eq!(p, path);
            }
            _ => panic!(
                "Expected ObjectStoreError::AlreadyExists, got {:?}",
                os_error
            ),
        }
    }

    #[test]
    fn test_map_status_to_os_error_permission_denied() {
        let status = Status::new(tonic::Code::PermissionDenied, "access denied");
        let path = "test/path/object.txt".to_string();
        let os_error = map_status_to_os_error(status, Some(path.clone()));
        match os_error {
            ObjectStoreError::PermissionDenied {
                path: p,
                source: _s,
            } => {
                assert_eq!(p, path);
            }
            _ => panic!(
                "Expected ObjectStoreError::PermissionDenied, got {:?}",
                os_error
            ),
        }
    }

    #[test]
    fn test_map_status_to_os_error_unauthenticated() {
        let status = Status::new(tonic::Code::Unauthenticated, "auth needed");
        let path = "test/path/object.txt".to_string();
        let os_error = map_status_to_os_error(status, Some(path.clone()));
        match os_error {
            ObjectStoreError::Unauthenticated {
                path: p,
                source: _s,
            } => {
                assert_eq!(p, path);
            }
            _ => panic!(
                "Expected ObjectStoreError::Unauthenticated, got {:?}",
                os_error
            ),
        }
    }

    #[test]
    fn test_map_status_to_os_error_generic() {
        let status = Status::new(tonic::Code::Internal, "internal server error");
        let os_error = map_status_to_os_error(status, None);
        match os_error {
            ObjectStoreError::Generic {
                store: s,
                source: _,
            } => {
                assert_eq!(s, "gRPC");
            }
            _ => panic!("Expected ObjectStoreError::Generic, got {:?}", os_error),
        }
    }

    #[test]
    fn test_map_status_to_os_error_not_modified() {
        let status = Status::new(tonic::Code::Aborted, "bla bla Not modified bla");
        let path = "test/path/object.txt".to_string();
        let os_error = map_status_to_os_error(status, Some(path.clone()));
        match os_error {
            ObjectStoreError::NotModified {
                path: p,
                source: _s,
            } => {
                assert_eq!(p, path);
            }
            _ => panic!("Expected ObjectStoreError::NotModified, got {:?}", os_error),
        }
    }

    #[test]
    fn test_map_status_to_os_error_unimplemented() {
        let status = Status::new(tonic::Code::Unimplemented, "not there yet");
        let os_error = map_status_to_os_error(status, None);
        match os_error {
            ObjectStoreError::NotImplemented => {} // Good
            _ => panic!(
                "Expected ObjectStoreError::NotImplemented, got {:?}",
                os_error
            ),
        }
    }

    #[test]
    fn test_map_proto_object_meta_to_os_conversion() {
        let proto_meta = ProtoObjectMeta {
            location: "test/location.txt".to_string(),
            last_modified: Some(Timestamp {
                seconds: 1678886400,
                nanos: 123456789,
            }), // 2023-03-15T12:00:00.123456789Z
            size: 1024,
            e_tag: Some("etag123".to_string()),
            version: Some("v1".to_string()),
        };

        let os_meta_result = map_proto_object_meta_to_os(proto_meta.clone());
        assert!(os_meta_result.is_ok());
        let os_meta = os_meta_result.unwrap();

        assert_eq!(os_meta.location, Path::from("test/location.txt"));
        assert_eq!(os_meta.last_modified.timestamp(), 1678886400);
        assert_eq!(os_meta.last_modified.timestamp_subsec_nanos(), 123456789);
        assert_eq!(os_meta.size, 1024);
        assert_eq!(os_meta.e_tag.as_deref(), Some("etag123"));
        assert_eq!(os_meta.version.as_deref(), Some("v1"));
    }

    #[test]
    fn test_map_proto_object_meta_to_os_invalid_timestamp() {
        let proto_meta = ProtoObjectMeta {
            location: "test/location.txt".to_string(),
            last_modified: Some(Timestamp {
                seconds: i64::MAX,
                nanos: 2_000_000_000,
            }), // Invalid nanos
            size: 1024,
            e_tag: None,
            version: None,
        };
        let os_meta_result = map_proto_object_meta_to_os(proto_meta);
        assert!(os_meta_result.is_err());
        match os_meta_result.err().unwrap() {
            ObjectStoreError::Generic { store, source: s } => {
                assert_eq!(store, "gRPC");
                assert!(s.to_string().contains("Invalid timestamp"));
            }
            e => panic!("Expected Generic error, got {:?}", e),
        }
    }

    #[test]
    fn test_map_proto_object_meta_to_os_missing_timestamp() {
        let proto_meta = ProtoObjectMeta {
            location: "test/location.txt".to_string(),
            last_modified: None,
            size: 1024,
            e_tag: None,
            version: None,
        };
        let os_meta_result = map_proto_object_meta_to_os(proto_meta);
        assert!(os_meta_result.is_ok());
        let os_meta = os_meta_result.unwrap();
        assert_eq!(os_meta.last_modified.timestamp(), 0); // Default value
        assert_eq!(os_meta.last_modified.timestamp_subsec_nanos(), 0);
    }

    #[test]
    fn test_map_os_object_meta_to_proto_conversion() {
        use chrono::{TimeZone, Utc};
        let last_modified_dt = Utc
            .with_ymd_and_hms(2023, 3, 15, 12, 0, 0)
            .unwrap()
            .with_nanosecond(123456789)
            .unwrap();
        let os_meta = ObjectMeta {
            location: Path::from("test/os_location.txt"),
            last_modified: last_modified_dt,
            size: 2048,
            e_tag: Some("os_etag".to_string()),
            version: Some("os_v2".to_string()),
        };

        let proto_meta = map_os_object_meta_to_proto(os_meta.clone());

        assert_eq!(proto_meta.location, "test/os_location.txt");
        assert!(proto_meta.last_modified.is_some());
        let ts = proto_meta.last_modified.unwrap();
        assert_eq!(ts.seconds, last_modified_dt.timestamp());
        assert_eq!(ts.nanos, last_modified_dt.timestamp_subsec_nanos() as i32);
        assert_eq!(proto_meta.size, 2048);
        assert_eq!(proto_meta.e_tag.as_deref(), Some("os_etag"));
        assert_eq!(proto_meta.version.as_deref(), Some("os_v2"));
    }

    #[test]
    fn test_map_os_get_range_to_proto() {
        // Bounded
        let os_range_bounded = OsGetRange::Bounded(StdRange { start: 10, end: 20 });
        let proto_range_opt = map_os_get_range_to_proto(os_range_bounded);
        assert!(proto_range_opt.is_some());
        match proto_range_opt.unwrap().range_type {
            Some(ProtoGetRangeOptionType::Bounded(r)) => {
                assert_eq!(r.start, 10);
                assert_eq!(r.end, 20);
            }
            _ => panic!("Expected Bounded range"),
        }

        // Offset
        let os_range_offset = OsGetRange::Offset(50);
        let proto_range_opt = map_os_get_range_to_proto(os_range_offset);
        assert!(proto_range_opt.is_some());
        match proto_range_opt.unwrap().range_type {
            Some(ProtoGetRangeOptionType::OffsetFromStart(o)) => assert_eq!(o, 50),
            _ => panic!("Expected OffsetFromStart range"),
        }

        // Suffix
        let os_range_suffix = OsGetRange::Suffix(100);
        let proto_range_opt = map_os_get_range_to_proto(os_range_suffix);
        assert!(proto_range_opt.is_some());
        match proto_range_opt.unwrap().range_type {
            Some(ProtoGetRangeOptionType::SuffixLength(s)) => assert_eq!(s, 100),
            _ => panic!("Expected SuffixLength range"),
        }
    }

    #[test]
    fn test_map_proto_get_range_option_to_os() {
        // Bounded
        let proto_range_bounded = ProtoGetRangeOption {
            range_type: Some(ProtoGetRangeOptionType::Bounded(ProtoRange {
                start: 10,
                end: 20,
            })),
        };
        let os_range_opt = map_proto_get_range_option_to_os(proto_range_bounded);
        assert!(os_range_opt.is_some());
        match os_range_opt.unwrap() {
            OsGetRange::Bounded(r) => {
                assert_eq!(r.start, 10);
                assert_eq!(r.end, 20);
            }
            _ => panic!("Expected OsGetRange::Bounded"),
        }

        // Offset
        let proto_range_offset = ProtoGetRangeOption {
            range_type: Some(ProtoGetRangeOptionType::OffsetFromStart(50)),
        };
        let os_range_opt = map_proto_get_range_option_to_os(proto_range_offset);
        assert!(os_range_opt.is_some());
        match os_range_opt.unwrap() {
            OsGetRange::Offset(o) => assert_eq!(o, 50),
            _ => panic!("Expected OsGetRange::Offset"),
        }

        // Suffix
        let proto_range_suffix = ProtoGetRangeOption {
            range_type: Some(ProtoGetRangeOptionType::SuffixLength(100)),
        };
        let os_range_opt = map_proto_get_range_option_to_os(proto_range_suffix);
        assert!(os_range_opt.is_some());
        match os_range_opt.unwrap() {
            OsGetRange::Suffix(s) => assert_eq!(s, 100),
            _ => panic!("Expected OsGetRange::Suffix"),
        }

        // None
        let proto_range_none = ProtoGetRangeOption { range_type: None };
        let os_range_opt = map_proto_get_range_option_to_os(proto_range_none);
        assert!(os_range_opt.is_none());
    }

    #[test]
    fn test_is_get_options_default() {
        assert!(is_get_options_default(&GetOptions::default()));

        let mut opts_non_default = GetOptions::default();
        opts_non_default.if_match = Some("match".to_string());
        assert!(!is_get_options_default(&opts_non_default));

        opts_non_default = GetOptions::default();
        opts_non_default.if_none_match = Some("none_match".to_string());
        assert!(!is_get_options_default(&opts_non_default));

        opts_non_default = GetOptions::default();
        opts_non_default.if_modified_since = Some(chrono::DateTime::from_timestamp(1, 0).unwrap());
        assert!(!is_get_options_default(&opts_non_default));

        opts_non_default = GetOptions::default();
        opts_non_default.if_unmodified_since =
            Some(chrono::DateTime::from_timestamp(1, 0).unwrap());
        assert!(!is_get_options_default(&opts_non_default));

        opts_non_default = GetOptions::default();
        opts_non_default.range = Some(OsGetRange::Offset(10));
        assert!(!is_get_options_default(&opts_non_default));

        opts_non_default = GetOptions::default();
        opts_non_default.version = Some("v1".to_string());
        assert!(!is_get_options_default(&opts_non_default));

        opts_non_default = GetOptions::default();
        opts_non_default.head = true; // default is false
        assert!(!is_get_options_default(&opts_non_default));
    }

    #[test]
    fn test_map_os_get_options_to_proto_options_type() {
        use chrono::TimeZone; // Import TimeZone trait for with_ymd_and_hms

        // Default options should return None
        let os_opts_default = GetOptions::default();
        assert!(map_os_get_options_to_proto_options_type(os_opts_default.clone()).is_none());

        // Non-default options
        let modified_date = chrono::Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
        let os_opts_full = GetOptions {
            if_match: Some("match_val".to_string()),
            if_none_match: Some("none_match_val".to_string()),
            if_modified_since: Some(modified_date),
            if_unmodified_since: Some(modified_date),
            range: Some(OsGetRange::Bounded(StdRange { start: 5, end: 15 })),
            version: Some("ver1".to_string()),
            head: true,
            extensions: Default::default(),
        };
        let proto_opts_opt = map_os_get_options_to_proto_options_type(os_opts_full.clone());
        assert!(proto_opts_opt.is_some());
        let proto_opts = proto_opts_opt.unwrap();

        assert_eq!(proto_opts.if_match.as_deref(), Some("match_val"));
        assert_eq!(proto_opts.if_none_match.as_deref(), Some("none_match_val"));
        assert_eq!(
            proto_opts.if_modified_since.as_ref().unwrap().seconds,
            modified_date.timestamp()
        );
        assert_eq!(
            proto_opts.if_unmodified_since.as_ref().unwrap().seconds,
            modified_date.timestamp()
        );
        assert!(proto_opts.range.is_some());
        match proto_opts.range.unwrap().range_type {
            Some(ProtoGetRangeOptionType::Bounded(r)) => {
                assert_eq!(r.start, 5);
                assert_eq!(r.end, 15);
            }
            _ => panic!("Expected Bounded range in proto options"),
        }
        assert_eq!(proto_opts.version.as_deref(), Some("ver1"));
        assert!(proto_opts.head);
    }

    #[test]
    fn test_map_proto_get_options_to_os() {
        use chrono::TimeZone; // Import TimeZone trait for with_ymd_and_hms
                              // Default proto options
        let proto_opts_default = ProtoGetOptions {
            if_match: None,
            if_none_match: None,
            if_modified_since: None,
            if_unmodified_since: None,
            range: None,
            version: None,
            head: false,
        };
        let os_opts = map_proto_get_options_to_os(proto_opts_default.clone());
        assert!(os_opts.if_match.is_none());
        assert!(os_opts.if_none_match.is_none());
        assert!(os_opts.if_modified_since.is_none());
        assert!(os_opts.if_unmodified_since.is_none());
        assert!(os_opts.range.is_none());
        assert!(os_opts.version.is_none());
        assert!(!os_opts.head);

        // Full proto options
        let modified_ts = Timestamp {
            seconds: chrono::Utc
                .with_ymd_and_hms(2023, 1, 1, 0, 0, 0)
                .unwrap()
                .timestamp(),
            nanos: 0,
        };
        let proto_opts_full = ProtoGetOptions {
            if_match: Some("match_val_proto".to_string()),
            if_none_match: Some("none_match_val_proto".to_string()),
            if_modified_since: Some(modified_ts),
            if_unmodified_since: Some(modified_ts),
            range: Some(ProtoGetRangeOption {
                range_type: Some(ProtoGetRangeOptionType::OffsetFromStart(25)),
            }),
            version: Some("ver_proto".to_string()),
            head: true,
        };
        let os_opts_full = map_proto_get_options_to_os(proto_opts_full.clone());

        assert_eq!(os_opts_full.if_match.as_deref(), Some("match_val_proto"));
        assert_eq!(
            os_opts_full.if_none_match.as_deref(),
            Some("none_match_val_proto")
        );
        assert_eq!(
            os_opts_full.if_modified_since.unwrap().timestamp(),
            modified_ts.seconds
        );
        assert_eq!(
            os_opts_full.if_unmodified_since.unwrap().timestamp(),
            modified_ts.seconds
        );
        assert!(os_opts_full.range.is_some());
        match os_opts_full.range.unwrap() {
            OsGetRange::Offset(o) => assert_eq!(o, 25),
            _ => panic!("Expected Offset range in OS options"),
        }
        assert_eq!(os_opts_full.version.as_deref(), Some("ver_proto"));
        assert!(os_opts_full.head);
    }

    #[test]
    fn test_map_os_put_mode_to_proto() {
        // Overwrite
        let (mode_enum, update_details) = map_os_put_mode_to_proto(OsPutMode::Overwrite);
        assert_eq!(mode_enum, ProtoPutModeEnum::PutModeOverwrite);
        assert!(update_details.is_none());

        // Create
        let (mode_enum, update_details) = map_os_put_mode_to_proto(OsPutMode::Create);
        assert_eq!(mode_enum, ProtoPutModeEnum::PutModeCreate);
        assert!(update_details.is_none());

        // Update
        let os_update_version = OsUpdateVersion {
            e_tag: Some("etag_update".to_string()),
            version: Some("v_update".to_string()),
        };
        let (mode_enum, update_details) =
            map_os_put_mode_to_proto(OsPutMode::Update(os_update_version.clone()));
        assert_eq!(mode_enum, ProtoPutModeEnum::PutModeUpdate);
        assert!(update_details.is_some());
        let proto_update_version = update_details.unwrap();
        assert_eq!(proto_update_version.e_tag, os_update_version.e_tag);
        assert_eq!(proto_update_version.version, os_update_version.version);
    }

    // test_map_proto_put_options_to_os will cover the reverse for PutMode

    #[test]
    fn test_map_os_tag_set_to_proto() {
        // 1. Test with an empty OsTagSet
        let os_tags_empty = OsTagSet::default();
        let proto_tags_opt_empty = map_os_tag_set_to_proto(os_tags_empty);
        // An empty OsTagSet should result in None, as its encoded form is an empty string.
        assert!(proto_tags_opt_empty.is_none());

        // 2. Test with a non-empty OsTagSet containing valid tags
        let mut os_tags_non_empty = OsTagSet::default();
        os_tags_non_empty.push("key1", "value1");
        os_tags_non_empty.push("key Space", "value Space"); // Test with spaces
        let proto_tags_opt_non_empty = map_os_tag_set_to_proto(os_tags_non_empty.clone());

        // Non-empty OsTagSet with valid tags should result in Some(ProtoTagSet).
        assert!(proto_tags_opt_non_empty.is_some());
        let proto_tags_set = proto_tags_opt_non_empty.unwrap();
        assert_eq!(
            proto_tags_set.tags.len(),
            2,
            "Expected two tags to be parsed"
        );
        assert!(
            proto_tags_set.tags.contains(&ProtoTag {
                key: "key1".to_string(),
                value: "value1".to_string()
            }),
            "Tag 'key1':'value1' not found"
        );
        assert!(
            proto_tags_set.tags.contains(&ProtoTag {
                key: "key Space".to_string(),
                value: "value Space".to_string()
            }),
            "Tag 'key Space':'value Space' not found"
        );

        // Note: The function `map_os_tag_set_to_proto` also handles a theoretical case where
        // `os_tags.encoded()` is non-empty, but `form_urlencoded::parse` yields no valid key-value pairs.
        // In such a scenario (e.g., if `encoded_tags` was "&=&="), `proto_tags` would be empty,
        // and the function would correctly return `None`.
        // Constructing such an `OsTagSet` directly via `OsTagSet::push` is difficult because `push`
        // properly URL-encodes. Tags with empty keys are ignored by `form_urlencoded::parse`.
        // For example, if one were to manually create an OsTagSet whose `encoded()` string is `"="`,
        // `parse("=".as_bytes())` yields an empty iterator, and `map_os_tag_set_to_proto` would correctly return `None`.
        // The current tests with empty and valid non-empty tags cover the primary expected behaviors.
    }

    #[test]
    fn test_map_proto_tag_set_to_os() {
        // Empty proto tags
        let proto_tags_empty = ProtoTagSet { tags: vec![] };
        let os_tags = map_proto_tag_set_to_os(proto_tags_empty);
        assert_eq!(os_tags.encoded(), "");

        // Non-empty proto tags
        let proto_tags_non_empty = ProtoTagSet {
            tags: vec![
                ProtoTag {
                    key: "protoKey1".to_string(),
                    value: "protoValue1".to_string(),
                },
                ProtoTag {
                    key: "protoKey Space".to_string(),
                    value: "protoValue Space".to_string(),
                },
            ],
        };
        let os_tags_non_empty = map_proto_tag_set_to_os(proto_tags_non_empty);

        let mut expected_os_tags = OsTagSet::default();
        expected_os_tags.push("protoKey1", "protoValue1");
        expected_os_tags.push("protoKey Space", "protoValue Space");

        assert_eq!(os_tags_non_empty.encoded(), expected_os_tags.encoded());
    }

    #[test]
    fn test_map_os_attributes_to_proto() {
        // Empty attributes
        let os_attrs_empty = OsAttributes::new();
        let proto_attrs_opt = map_os_attributes_to_proto(os_attrs_empty);
        assert!(proto_attrs_opt.is_none());

        // Non-empty attributes
        let mut os_attrs_non_empty = OsAttributes::new();
        os_attrs_non_empty.insert(OsAttribute::ContentType, "application/json".into());
        os_attrs_non_empty.insert(OsAttribute::CacheControl, "no-cache".into());
        os_attrs_non_empty.insert(
            OsAttribute::Metadata(Cow::Borrowed("custom-key")),
            "custom-value".into(),
        );

        let proto_attrs_opt_non_empty = map_os_attributes_to_proto(os_attrs_non_empty.clone());
        assert!(proto_attrs_opt_non_empty.is_some());
        let proto_attrs_map = proto_attrs_opt_non_empty.unwrap();
        assert_eq!(proto_attrs_map.pairs.len(), 3);

        let mut found_content_type = false;
        let mut found_cache_control = false;
        let mut found_custom_key = false;

        for pair in proto_attrs_map.pairs {
            match pair.key.as_str() {
                "content-type" => {
                    assert_eq!(pair.value, "application/json");
                    found_content_type = true;
                }
                "cache-control" => {
                    assert_eq!(pair.value, "no-cache");
                    found_cache_control = true;
                }
                "metadata:custom-key" => {
                    assert_eq!(pair.value, "custom-value");
                    found_custom_key = true;
                }
                _ => panic!("Unexpected attribute key: {}", pair.key),
            }
        }
        assert!(found_content_type && found_cache_control && found_custom_key);

        // Test that if all attributes are unhandled, it results in None
        // This requires an OsAttribute variant not handled by the map function's match.
        // As of object_store 0.11.2, all variants seem handled or caught by Metadata.
        // The eprintln in the original code indicates a design to skip unhandled ones.
        // If such an attribute existed and was the *only* attribute, the current logic might give Some({pairs:[]})
        // However, the `if pairs.is_empty() && !os_attrs.is_empty()` check should handle this.
        // For now, we assume standard attributes are handled.
    }

    #[test]
    fn test_map_proto_attributes_to_os() {
        // None proto attributes map
        let os_attrs_empty = map_proto_attributes_to_os(None);
        assert!(os_attrs_empty.is_empty());

        // Empty pairs in proto attributes map
        let proto_attrs_empty_pairs = Some(ProtoAttributesMap { pairs: vec![] });
        let os_attrs_empty_pairs = map_proto_attributes_to_os(proto_attrs_empty_pairs);
        assert!(os_attrs_empty_pairs.is_empty());

        // Non-empty proto attributes
        let proto_attrs_non_empty = Some(ProtoAttributesMap {
            pairs: vec![
                StorageAttributeKeyValuePair {
                    key: "content-type".to_string(),
                    value: "text/plain".to_string(),
                },
                StorageAttributeKeyValuePair {
                    key: "Cache-Control".to_string(),
                    value: "max-age=3600".to_string(),
                }, // Test case insensitivity for key
                StorageAttributeKeyValuePair {
                    key: "metadata:user-id".to_string(),
                    value: "12345".to_string(),
                },
                StorageAttributeKeyValuePair {
                    key: "unknown-key".to_string(),
                    value: "some-value".to_string(),
                }, // Test unhandled key
                StorageAttributeKeyValuePair {
                    key: "metadata:".to_string(),
                    value: "empty-meta-key-val".to_string(),
                }, // Test empty metadata key part
                StorageAttributeKeyValuePair {
                    key: "metadata:empty-val-key".to_string(),
                    value: "".to_string(),
                },
            ],
        });
        let os_attrs = map_proto_attributes_to_os(proto_attrs_non_empty);

        // We expect 4 attributes to be mapped: content-type, cache-control, metadata:user-id, metadata:empty-val-key
        // "unknown-key" and "metadata:" will be logged as warnings and skipped.
        assert_eq!(os_attrs.len(), 4);
        assert_eq!(
            os_attrs.get(&OsAttribute::ContentType).map(|v| v.as_ref()),
            Some("text/plain")
        );
        assert_eq!(
            os_attrs.get(&OsAttribute::CacheControl).map(|v| v.as_ref()),
            Some("max-age=3600")
        );
        assert_eq!(
            os_attrs
                .get(&OsAttribute::Metadata(Cow::Borrowed("user-id")))
                .map(|v| v.as_ref()),
            Some("12345")
        );
        assert_eq!(
            os_attrs
                .get(&OsAttribute::Metadata(Cow::Borrowed("empty-val-key")))
                .map(|v| v.as_ref()),
            Some("")
        );

        // Verify that "unknown-key" and "metadata:" are not present
        let mut has_unknown = false;
        let mut has_empty_meta_prefix = false;
        for (attr, _val) in os_attrs.iter() {
            if let OsAttribute::Metadata(key_cow) = attr {
                if key_cow.as_ref() == "unknown-key" {
                    has_unknown = true;
                }
                if key_cow.is_empty() {
                    has_empty_meta_prefix = true;
                } // after stripping "metadata:"
            }
        }
        assert!(!has_unknown, "unknown-key should not be mapped");
        assert!(
            !has_empty_meta_prefix,
            "empty metadata key (after prefix) should not be mapped"
        );
    }

    #[test]
    fn test_map_os_put_options_to_proto() {
        // Default options
        let os_put_opts_default = PutOptions::default();
        let proto_put_opts_opt = map_os_put_options_to_proto(os_put_opts_default.clone());
        assert!(proto_put_opts_opt.is_none()); // Default mode, empty tags, empty attributes

        // Non-default mode
        let mut os_put_opts_create = PutOptions::default();
        os_put_opts_create.mode = OsPutMode::Create;
        let proto_put_opts_opt = map_os_put_options_to_proto(os_put_opts_create);
        assert!(proto_put_opts_opt.is_some());
        assert_eq!(
            proto_put_opts_opt.unwrap().mode(),
            ProtoPutModeEnum::PutModeCreate
        );

        // Non-default tags
        let mut os_put_opts_tags = PutOptions::default();
        os_put_opts_tags.tags.push("tagkey", "tagvalue");
        let proto_put_opts_opt = map_os_put_options_to_proto(os_put_opts_tags);
        assert!(proto_put_opts_opt.is_some());
        assert!(proto_put_opts_opt.unwrap().tags.is_some());

        // Non-default attributes
        let mut os_put_opts_attrs = PutOptions::default();
        os_put_opts_attrs
            .attributes
            .insert(OsAttribute::ContentType, "image/png".into());
        let proto_put_opts_opt = map_os_put_options_to_proto(os_put_opts_attrs);
        assert!(proto_put_opts_opt.is_some());
        assert!(proto_put_opts_opt.unwrap().attributes.is_some());

        // All non-default
        let mut os_put_opts_full = PutOptions {
            mode: OsPutMode::Update(OsUpdateVersion {
                e_tag: Some("e1".to_string()),
                version: Some("v1".to_string()),
            }),
            tags: OsTagSet::default(),
            attributes: OsAttributes::new(),
            extensions: Default::default(),
        };
        os_put_opts_full.tags.push("fullkey", "fullvalue");
        os_put_opts_full
            .attributes
            .insert(OsAttribute::ContentEncoding, "gzip".into());

        let proto_put_opts_opt = map_os_put_options_to_proto(os_put_opts_full);
        assert!(proto_put_opts_opt.is_some());
        let proto_opts = proto_put_opts_opt.unwrap();
        assert_eq!(proto_opts.mode(), ProtoPutModeEnum::PutModeUpdate);
        assert!(proto_opts.update_version_details.is_some());
        assert_eq!(
            proto_opts
                .update_version_details
                .as_ref()
                .unwrap()
                .e_tag
                .as_deref(),
            Some("e1")
        );
        assert!(proto_opts.tags.is_some());
        assert_eq!(proto_opts.tags.as_ref().unwrap().tags.len(), 1);
        assert!(proto_opts.attributes.is_some());
        assert_eq!(proto_opts.attributes.as_ref().unwrap().pairs.len(), 1);
    }

    #[test]
    fn test_map_proto_put_options_to_os() {
        // Default proto options (mode OVERWRITE, no version, no tags, no attributes)
        let proto_opts_default = ProtoPutOptions {
            mode: ProtoPutModeEnum::PutModeOverwrite.into(),
            update_version_details: None,
            tags: None,
            attributes: None,
        };
        let os_opts = map_proto_put_options_to_os(proto_opts_default);
        assert_eq!(os_opts.mode, OsPutMode::Overwrite);
        assert!(os_opts.tags.encoded().is_empty());
        assert!(os_opts.attributes.is_empty());

        // Full proto options
        let proto_opts_full = ProtoPutOptions {
            mode: ProtoPutModeEnum::PutModeUpdate.into(),
            update_version_details: Some(ProtoUpdateVersion {
                e_tag: Some("e_proto".to_string()),
                version: Some("v_proto".to_string()),
            }),
            tags: Some(ProtoTagSet {
                tags: vec![ProtoTag {
                    key: "ptk".to_string(),
                    value: "ptv".to_string(),
                }],
            }),
            attributes: Some(ProtoAttributesMap {
                pairs: vec![StorageAttributeKeyValuePair {
                    key: "content-type".to_string(),
                    value: "app/proto".to_string(),
                }],
            }),
        };
        let os_opts_full = map_proto_put_options_to_os(proto_opts_full);

        match os_opts_full.mode {
            OsPutMode::Update(uv) => {
                assert_eq!(uv.e_tag.as_deref(), Some("e_proto"));
                assert_eq!(uv.version.as_deref(), Some("v_proto"));
            }
            _ => panic!("Expected OsPutMode::Update"),
        }
        assert!(!os_opts_full.tags.encoded().is_empty());
        assert!(os_opts_full.tags.encoded().contains("ptk=ptv"));
        assert!(!os_opts_full.attributes.is_empty());
        assert_eq!(
            os_opts_full
                .attributes
                .get(&OsAttribute::ContentType)
                .map(|v| v.as_ref()),
            Some("app/proto")
        );

        // Proto Update mode without details should default to Overwrite in OS
        let proto_opts_update_no_details = ProtoPutOptions {
            mode: ProtoPutModeEnum::PutModeUpdate.into(),
            update_version_details: None,
            tags: None,
            attributes: None,
        };
        let os_opts_update_no_details = map_proto_put_options_to_os(proto_opts_update_no_details);
        assert_eq!(os_opts_update_no_details.mode, OsPutMode::Overwrite);
    }

    #[test]
    fn test_map_os_put_multipart_opts_to_proto() {
        // Default options
        let os_mp_opts_default = PutMultipartOpts::default();
        assert!(map_os_put_multipart_opts_to_proto(os_mp_opts_default.clone()).is_none());

        // Non-default tags
        let mut os_mp_opts_tags = PutMultipartOpts::default();
        os_mp_opts_tags.tags.push("mpkey", "mpvalue");
        let proto_mp_opts_opt = map_os_put_multipart_opts_to_proto(os_mp_opts_tags);
        assert!(proto_mp_opts_opt.is_some());
        assert!(proto_mp_opts_opt.unwrap().tags.is_some());

        // Non-default attributes
        let mut os_mp_opts_attrs = PutMultipartOpts::default();
        os_mp_opts_attrs
            .attributes
            .insert(OsAttribute::ContentLanguage, "en".into());
        let proto_mp_opts_opt = map_os_put_multipart_opts_to_proto(os_mp_opts_attrs);
        assert!(proto_mp_opts_opt.is_some());
        assert!(proto_mp_opts_opt.unwrap().attributes.is_some());

        // All non-default
        let mut os_mp_opts_full = PutMultipartOpts::default();
        os_mp_opts_full.tags.push("full_mp_key", "full_mp_value");
        os_mp_opts_full
            .attributes
            .insert(OsAttribute::ContentDisposition, "attachment".into());
        let proto_mp_opts_opt = map_os_put_multipart_opts_to_proto(os_mp_opts_full);
        assert!(proto_mp_opts_opt.is_some());
        let proto_opts = proto_mp_opts_opt.unwrap();
        assert!(proto_opts.tags.is_some());
        assert_eq!(proto_opts.tags.as_ref().unwrap().tags.len(), 1);
        assert!(proto_opts.attributes.is_some());
        assert_eq!(proto_opts.attributes.as_ref().unwrap().pairs.len(), 1);
    }

    #[test]
    fn test_map_proto_put_multipart_options_to_os() {
        // Default options (None for tags and attributes)
        let proto_mp_opts_default = ProtoPutMultipartOptions {
            tags: None,
            attributes: None,
        };
        let os_mp_opts = map_proto_put_multipart_options_to_os(proto_mp_opts_default);
        assert!(os_mp_opts.tags.encoded().is_empty());
        assert!(os_mp_opts.attributes.is_empty());

        // Full options
        let proto_mp_opts_full = ProtoPutMultipartOptions {
            tags: Some(ProtoTagSet {
                tags: vec![ProtoTag {
                    key: "p_mp_tk".to_string(),
                    value: "p_mp_tv".to_string(),
                }],
            }),
            attributes: Some(ProtoAttributesMap {
                pairs: vec![StorageAttributeKeyValuePair {
                    key: "content-type".to_string(),
                    value: "audio/mpeg".to_string(),
                }],
            }),
        };
        let os_mp_opts_full = map_proto_put_multipart_options_to_os(proto_mp_opts_full);
        assert!(!os_mp_opts_full.tags.encoded().is_empty());
        assert!(os_mp_opts_full.tags.encoded().contains("p_mp_tk=p_mp_tv"));
        assert!(!os_mp_opts_full.attributes.is_empty());
        assert_eq!(
            os_mp_opts_full
                .attributes
                .get(&OsAttribute::ContentType)
                .map(|v| v.as_ref()),
            Some("audio/mpeg")
        );
    }
}
