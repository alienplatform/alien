resource "aws_iam_role" "execution_sa" {
  name = length(format("%s-%s", local.resource_prefix, "execution-sa")) <= 64 ? format("%s-%s", local.resource_prefix, "execution-sa") : format("%s-%s", substr(format("%s-%s", local.resource_prefix, "execution-sa"), 0, 55), substr(sha1(format("%s-%s", local.resource_prefix, "execution-sa")), 0, 8))
  assume_role_policy = jsonencode({ Version = "2012-10-17", Statement = [{ Effect = "Allow", Principal = { Service = ["codebuild.amazonaws.com", "ec2.amazonaws.com", "lambda.amazonaws.com"] }, Action = "sts:AssumeRole" }] })
  tags = {
    "managed-by" = "setup"
    deployment = local.resource_prefix
    resource = "execution-sa"
    "resource-type" = "service-account"
  }
}

resource "aws_iam_role_policy" "execution_sa_set_0" {
  name = "q5-policy-read-0"
  role = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "Q5PolicyRead", Effect = "Allow", Action = ["s3:GetObject"], Resource = ["arn:aws:s3:::q5-policy-control/objects/object-0000", "arn:aws:s3:::q5-policy-control/objects/object-0001", "arn:aws:s3:::q5-policy-control/objects/object-0002", "arn:aws:s3:::q5-policy-control/objects/object-0003", "arn:aws:s3:::q5-policy-control/objects/object-0004", "arn:aws:s3:::q5-policy-control/objects/object-0005", "arn:aws:s3:::q5-policy-control/objects/object-0006", "arn:aws:s3:::q5-policy-control/objects/object-0007", "arn:aws:s3:::q5-policy-control/objects/object-0008", "arn:aws:s3:::q5-policy-control/objects/object-0009"] }] })
}
