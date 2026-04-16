# -----------------------------------------------------------------------------
# IAM Role for Cross-Account Impersonation
# (conditional on enable_impersonation)
# -----------------------------------------------------------------------------

data "aws_iam_policy_document" "impersonation_assume" {
  count = var.enable_impersonation ? 1 : 0

  # Allow the manager task role to assume this role
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "AWS"
      identifiers = [var.principal_arn]
    }
  }

  # Allow trusted external accounts to assume this role
  dynamic "statement" {
    for_each = length(var.impersonation_trusted_accounts) > 0 ? [1] : []
    content {
      actions = ["sts:AssumeRole"]
      principals {
        type        = "AWS"
        identifiers = [for acct in var.impersonation_trusted_accounts : "arn:aws:iam::${acct}:root"]
      }
    }
  }
}

resource "aws_iam_role" "impersonation" {
  count = var.enable_impersonation ? 1 : 0

  name               = "${var.name}-manager-impersonation"
  assume_role_policy = data.aws_iam_policy_document.impersonation_assume[0].json
  tags               = local.common_tags
}

# The impersonation role starts with no permissions. Attach policies as needed
# for your use case (e.g. deploying to ECS, Lambda, etc.).
