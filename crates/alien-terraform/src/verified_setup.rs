//! Verified setup for self-registering AWS and GCP modules.
//!
//! A module applied with `setup_reservation_id` (and, on first apply, the
//! one-use `enrollment_code`) registers without a long-lived install token:
//!
//! 1. `this` reserves the deployment ID with the enrollment code.
//! 2. The module writes a setup record (JSON) to a bucket it owns, at
//!    `setup/<setup_reservation_id>/intent.json`. Only the managing identity
//!    may read that object and list that exact key.
//! 3. `setup_observe` asks the manager to read the record and activate the
//!    deployment. It carries no token and no code.
//!
//! On destroy, Terraform deletes the record before `this`, so the manager can
//! confirm the record is gone before it removes the deployment. The bucket
//! and its grants outlive `this` for that check.
//!
//! A module applied with `token` keeps the original registration unchanged.

use alien_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use hcl::{
    expr::Expression,
    structure::{Block, BlockLabel, Body, Structure},
    Identifier,
};

use crate::{
    block::{attr, block, nested, resource_block},
    expr, TerraformRegistration, TerraformTarget,
};

/// README section for modules that offer verified setup.
pub(crate) const README_INPUTS: &str = "Verified setup (instead of `token`):\n\n- `setup_reservation_id`: deployment ID reserved for this setup.\n- `enrollment_code`: one-use code issued with the reservation. It is valid for one hour and needed only until the first apply succeeds.\n- `setup_record_bucket`: name of a new bucket this module creates. It holds the setup record at `setup/<setup_reservation_id>/intent.json`, which only the managing identity can read. The manager reads it to activate the deployment, and checks that it is gone before it deletes the deployment on `terraform destroy`.\n\nSet either `token` or `setup_reservation_id`. A module applied with `token` keeps registering with it.";

/// Resource label of the post-record activation resource.
pub(crate) const OBSERVE_LABEL: &str = "setup_observe";

/// Whether a module offers verified setup. The manager reads setup records on
/// AWS and GCP only, and modules for automatic setup keep their own-environment
/// token check.
pub(crate) fn supported(
    target: TerraformTarget,
    setup_management: alien_core::SetupManagement,
    registration: Option<&TerraformRegistration>,
) -> bool {
    registration.is_some()
        && matches!(target, TerraformTarget::Aws | TerraformTarget::Gcp)
        && setup_management == alien_core::SetupManagement::Manual
}

/// Variables of the verified path. `token` stays declared, but optional.
pub(crate) fn variables(target: TerraformTarget) -> Vec<Block> {
    let bucket_kind = match target.cloud_platform() {
        alien_core::Platform::Gcp => "Cloud Storage",
        _ => "S3",
    };
    vec![
        optional_variable(
            "token",
            "Legacy install token from the application setup page. Leave it unset when setup_reservation_id is set.",
            true,
            None,
        ),
        optional_variable(
            "setup_reservation_id",
            "Deployment ID reserved for verified setup. Set it with setup_record_bucket, and with enrollment_code on the first apply, instead of token.",
            false,
            Some((
                "var.setup_reservation_id == null || can(regex(\"^[A-Za-z0-9_-]{1,128}$\", var.setup_reservation_id))",
                "setup_reservation_id must be 1-128 letters, numbers, underscores or hyphens.",
            )),
        ),
        optional_variable(
            "enrollment_code",
            "One-use enrollment code issued with setup_reservation_id. It is valid for one hour and needed only until the first apply succeeds.",
            true,
            None,
        ),
        optional_variable(
            "setup_record_bucket",
            &format!("Name of a new {bucket_kind} bucket that this module creates to hold the setup record. Required with setup_reservation_id."),
            false,
            Some((
                "var.setup_record_bucket == null || can(regex(\"^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$\", var.setup_record_bucket))",
                "setup_record_bucket must be 3-63 lowercase letters, numbers, dots or hyphens, starting and ending with a letter or number.",
            )),
        ),
    ]
}

fn optional_variable(
    name: &str,
    description: &str,
    sensitive: bool,
    validation: Option<(&str, &str)>,
) -> Block {
    let mut body = vec![
        attr("type", expr::raw("string")),
        attr("description", Expression::String(description.to_string())),
        attr("default", expr::raw("null")),
    ];
    if sensitive {
        body.push(attr("sensitive", Expression::Bool(true)));
    }
    if let Some((condition, message)) = validation {
        body.push(nested(block(
            "validation",
            [
                attr("condition", expr::raw(condition)),
                attr("error_message", Expression::String(message.to_string())),
            ],
        )));
    }
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(name.to_string())],
        body: Body::from(body),
    }
}

/// Attributes that select the registration path on the `this` resource.
pub(crate) fn reserve_attributes() -> Vec<Structure> {
    vec![
        attr(
            "token",
            expr::raw("var.setup_reservation_id == null ? var.token : null"),
        ),
        attr(
            "setup_reservation_id",
            expr::raw("var.setup_reservation_id"),
        ),
        attr(
            "setup_stage",
            expr::raw("var.setup_reservation_id == null ? null : \"registration\""),
        ),
        attr("enrollment_code", expr::raw("var.enrollment_code")),
    ]
}

/// Preconditions on `this`: exactly one registration path, and the inputs
/// the verified path needs.
pub(crate) fn reserve_preconditions(target: TerraformTarget) -> Vec<Block> {
    let managing_identity = match target.cloud_platform() {
        alien_core::Platform::Gcp => "var.managing_service_account_email",
        _ => "var.managing_role_arn",
    };
    [
        (
            // `token` is sensitive. Its presence is not.
            "nonsensitive(sensitive(var.token != null)) != (var.setup_reservation_id != null)".to_string(),
            "Set either token or setup_reservation_id, not both.".to_string(),
        ),
        (
            format!(
                "var.setup_reservation_id == null || (var.setup_record_bucket != null && {managing_identity} != \"\")"
            ),
            format!(
                "setup_reservation_id requires setup_record_bucket and {}.",
                managing_identity.trim_start_matches("var.")
            ),
        ),
    ]
    .into_iter()
    .map(|(condition, message)| {
        block(
            "precondition",
            [
                attr("condition", expr::raw(condition)),
                attr("error_message", Expression::String(message)),
            ],
        )
    })
    .collect()
}

/// Bucket resources the manager needs for both activation and the absence
/// check on destroy. `this` depends on them so they outlive it.
pub(crate) fn reserve_dependencies(target: TerraformTarget) -> Vec<Expression> {
    let addresses: &[&str] = match target.cloud_platform() {
        alien_core::Platform::Gcp => &[
            "google_storage_bucket.setup_record",
            "google_storage_bucket_iam_member.setup_record_objects",
            "google_storage_bucket_iam_member.setup_record_bucket",
            "google_project_iam_member.setup_record_project",
        ],
        _ => &[
            "aws_s3_bucket.setup_record",
            "aws_s3_bucket_public_access_block.setup_record",
            "aws_s3_bucket_policy.setup_record",
        ],
    };
    addresses.iter().map(expr::raw).collect()
}

/// The activation resource. It runs after the record exists and carries no
/// token and no code.
pub(crate) fn observe_block(
    target: TerraformTarget,
    registration: &TerraformRegistration,
    mut attributes: Vec<Structure>,
) -> Block {
    let record_object = match target.cloud_platform() {
        alien_core::Platform::Gcp => "google_storage_bucket_object.setup_record",
        _ => "aws_s3_object.setup_record",
    };
    attributes.insert(
        0,
        attr("count", expr::raw("local.setup_record_enabled ? 1 : 0")),
    );
    attributes.push(attr(
        "setup_reservation_id",
        expr::raw("var.setup_reservation_id"),
    ));
    attributes.push(attr("setup_stage", Expression::String("observe".into())));
    attributes.push(attr(
        "depends_on",
        Expression::Array(vec![expr::raw(record_object)]),
    ));
    resource_block(&registration_type(registration), OBSERVE_LABEL, attributes)
}

/// `setup_record.tf`: the bucket, its grants and the record.
pub(crate) fn record_file(
    target: TerraformTarget,
    registration: &TerraformRegistration,
    setup_import_format_version: u32,
) -> Result<String> {
    let scheme = match target.cloud_platform() {
        alien_core::Platform::Gcp => "gs",
        _ => "s3",
    };
    // The manager reads `source` with the field names of the CloudFormation
    // setup record. Typed values travel as JSON strings, and deployer inputs
    // stay out of the record.
    let locals = format!(
        r#"locals {{
  setup_record_enabled = var.setup_reservation_id != null
  setup_record_bucket  = coalesce(var.setup_record_bucket, "unset")
  setup_record_key     = "setup/${{coalesce(var.setup_reservation_id, "unset")}}/intent.json"
  setup_record = jsonencode({{
    version   = "1"
    installId = "{scheme}://${{local.setup_record_bucket}}/${{local.setup_record_key}}"
    audience  = var.management_url
    source = {{
      DeploymentName           = local.deployment_name
      ResourcePrefix           = local.resource_prefix
      Platform                 = local.deployment_platform
      Region                   = local.deployment_region
      SetupTarget              = {setup_target}
      SetupFingerprint         = {setup_fingerprint}
      SetupImportFormatVersion = "{setup_import_format_version}"
      SetupFingerprintVersion  = "{setup_fingerprint_version}"
      StackSettings            = jsonencode(local.deployment_settings)
      ManagementConfig         = jsonencode(local.deployment_management_config)
      Resources                = jsonencode(local.deployment_resources)
    }}
  }})
}}
"#,
        setup_target = quoted(&registration.setup_target),
        setup_fingerprint = quoted(&registration.setup_fingerprint),
        setup_fingerprint_version = registration.setup_fingerprint_version,
    );
    let cloud = match target.cloud_platform() {
        alien_core::Platform::Gcp => GCP_RECORD,
        _ => AWS_RECORD,
    };
    let text = format!(
        "{locals}{}",
        cloud.replace("__REGISTRATION__", &registration_address(registration))
    );
    // Written as text so reviewers read the policy as formatted HCL. Parse it
    // to fail here, not at `terraform init`, if it is ever malformed.
    hcl::from_str::<Body>(&text)
        .into_alien_error()
        .context(ErrorData::GenericError {
            message: "Could not build the verified setup record resources".into(),
        })?;
    Ok(text)
}

fn registration_type(registration: &TerraformRegistration) -> String {
    format!(
        "{}_{}",
        registration.provider_name, registration.resource_type
    )
}

fn registration_address(registration: &TerraformRegistration) -> String {
    format!("{}.this", registration_type(registration))
}

fn quoted(value: &str) -> String {
    // JSON string escapes are valid HCL; template markers must not interpolate.
    serde_json::to_string(value)
        .expect("string serializes")
        .replace("${", "$${")
        .replace("%{", "%%{")
}

/// The managing role reads the exact record and lists only its key. A
/// complete listing without the key is how the manager proves deletion.
const AWS_RECORD: &str = r#"
resource "aws_s3_bucket" "setup_record" {
  count  = local.setup_record_enabled ? 1 : 0
  bucket = local.setup_record_bucket
}

resource "aws_s3_bucket_public_access_block" "setup_record" {
  count                   = local.setup_record_enabled ? 1 : 0
  bucket                  = aws_s3_bucket.setup_record[0].id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_policy" "setup_record" {
  count  = local.setup_record_enabled ? 1 : 0
  bucket = aws_s3_bucket.setup_record[0].id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid       = "ReadSetupRecord"
        Effect    = "Allow"
        Principal = { AWS = var.managing_role_arn }
        Action    = "s3:GetObject"
        Resource  = "${aws_s3_bucket.setup_record[0].arn}/${local.setup_record_key}"
      },
      {
        Sid       = "ListSetupRecord"
        Effect    = "Allow"
        Principal = { AWS = var.managing_role_arn }
        Action    = "s3:ListBucket"
        Resource  = aws_s3_bucket.setup_record[0].arn
        Condition = { StringEquals = { "s3:prefix" = local.setup_record_key } }
      },
    ]
  })
  depends_on = [aws_s3_bucket_public_access_block.setup_record]
}

resource "aws_s3_object" "setup_record" {
  count        = local.setup_record_enabled ? 1 : 0
  bucket       = aws_s3_bucket.setup_record[0].id
  key          = local.setup_record_key
  content      = local.setup_record
  content_type = "application/json"
  depends_on   = [__REGISTRATION__]
}
"#;

/// The manager checks the bucket's project number, then reads the exact
/// object. A missing object proves deletion.
const GCP_RECORD: &str = r#"
resource "google_storage_bucket" "setup_record" {
  count                       = local.setup_record_enabled ? 1 : 0
  project                     = var.gcp_project
  name                        = local.setup_record_bucket
  location                    = var.gcp_region
  uniform_bucket_level_access = true
  public_access_prevention    = "enforced"
}

resource "google_storage_bucket_iam_member" "setup_record_objects" {
  count  = local.setup_record_enabled ? 1 : 0
  bucket = google_storage_bucket.setup_record[0].name
  role   = "roles/storage.objectViewer"
  member = "serviceAccount:${var.managing_service_account_email}"
}

resource "google_storage_bucket_iam_member" "setup_record_bucket" {
  count  = local.setup_record_enabled ? 1 : 0
  bucket = google_storage_bucket.setup_record[0].name
  role   = "roles/storage.legacyBucketReader"
  member = "serviceAccount:${var.managing_service_account_email}"
}

resource "google_project_iam_member" "setup_record_project" {
  count   = local.setup_record_enabled ? 1 : 0
  project = var.gcp_project
  role    = "roles/browser"
  member  = "serviceAccount:${var.managing_service_account_email}"
}

resource "google_storage_bucket_object" "setup_record" {
  count        = local.setup_record_enabled ? 1 : 0
  bucket       = google_storage_bucket.setup_record[0].name
  name         = local.setup_record_key
  content      = local.setup_record
  content_type = "application/json"
  depends_on   = [__REGISTRATION__]
}
"#;
