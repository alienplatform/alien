use google_cloud_gax::error::rpc::Code as GaxRpcCode;

pub(crate) fn service_account_resource_name(
    project_id: &str,
    service_account_name: &str,
) -> String {
    if service_account_name.starts_with("projects/") {
        service_account_name.to_string()
    } else {
        format!("projects/{project_id}/serviceAccounts/{service_account_name}")
    }
}

pub(crate) fn full_role_name(project_id: &str, role_name: &str) -> String {
    if role_name.starts_with("projects/") || role_name.starts_with("organizations/") {
        role_name.to_string()
    } else {
        format!("projects/{project_id}/roles/{role_name}")
    }
}

pub(crate) fn field_mask_from_comma_separated(update_mask: &str) -> wkt::FieldMask {
    wkt::FieldMask::default().set_paths(
        update_mask
            .split(',')
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(ToString::to_string),
    )
}

pub(crate) fn iam_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::NOT_FOUND.as_u16())
}

pub(crate) fn iam_error_is_conflict(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::AlreadyExists)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::CONFLICT.as_u16())
}
