resource "aws_iam_role" "management" {
  name = length(format("%s-%s", local.resource_prefix, "management")) <= 64 ? format("%s-%s", local.resource_prefix, "management") : format("%s-%s", substr(format("%s-%s", local.resource_prefix, "management"), 0, 55), substr(sha1(format("%s-%s", local.resource_prefix, "management")), 0, 8))
  assume_role_policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "AllowManagingRole", Effect = "Allow", Principal = { AWS = var.managing_role_arn }, Action = "sts:AssumeRole", Condition = { StringEquals = { "aws:PrincipalArn" = var.managing_role_arn } } }] })
  tags = {
    "managed-by" = "setup"
    deployment = local.resource_prefix
    resource = "management"
    "resource-type" = "management"
  }
}

resource "aws_iam_policy" "management_managed_policy_0" {
  name = length(format("%s-%s", local.resource_prefix, "deployment-management-0")) <= 128 ? format("%s-%s", local.resource_prefix, "deployment-management-0") : format("%s-%s", substr(format("%s-%s", local.resource_prefix, "deployment-management-0"), 0, 119), substr(sha1(format("%s-%s", local.resource_prefix, "deployment-management-0")), 0, 8))
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "DenyManagementRoleChanges", Effect = "Deny", Action = ["iam:DeleteRole", "iam:PutRolePolicy", "iam:DeleteRolePolicy", "iam:AttachRolePolicy", "iam:DetachRolePolicy", "iam:UpdateAssumeRolePolicy", "iam:PutRolePermissionsBoundary", "iam:DeleteRolePermissionsBoundary", "iam:UpdateRole", "iam:TagRole", "iam:UntagRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-management"] }] })
}

resource "aws_iam_role_policy_attachment" "management_managed_policy_0_attachment" {
  role = aws_iam_role.management.name
  policy_arn = aws_iam_policy.management_managed_policy_0.arn
}

resource "aws_iam_role_policy" "management_self_read" {
  name = "deployment-management-self-read"
  role = aws_iam_role.management.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ReadOwnManagementRole", Effect = "Allow", Action = "iam:GetRole", Resource = aws_iam_role.management.arn }] })
}
