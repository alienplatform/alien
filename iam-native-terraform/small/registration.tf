resource "terraform_data" "deployment_registration" {
  input = {
    platform = local.deployment_platform
    token = var.token
    name = var.name
    resource_prefix = local.resource_prefix
    setup_target = ""
    setup_import_format_version = 1
    setup_fingerprint = ""
    setup_fingerprint_version = 0
    management_url = var.management_url
    management_config = local.deployment_management_config
    stack_settings = local.deployment_settings
    resources = local.deployment_resources
    inputValues = {}
  }
  depends_on = [
    aws_iam_role.execution_sa,
    aws_iam_role_policy.execution_sa_set_0,
    aws_iam_role.management,
    aws_iam_policy.management_managed_policy_0,
    aws_iam_role_policy_attachment.management_managed_policy_0_attachment,
    aws_iam_role_policy.management_self_read
  ]
}
