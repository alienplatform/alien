# -----------------------------------------------------------------------------
# DynamoDB Table + S3 Bucket for Commands Store
# (conditional on enable_commands_store)
# -----------------------------------------------------------------------------

resource "aws_dynamodb_table" "commands" {
  count = var.enable_commands_store ? 1 : 0

  name         = "${var.name}-commands"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "pk"
  range_key    = "sk"
  tags         = local.common_tags

  attribute {
    name = "pk"
    type = "S"
  }

  attribute {
    name = "sk"
    type = "S"
  }

  ttl {
    attribute_name = "ttl"
    enabled        = true
  }

  point_in_time_recovery {
    enabled = true
  }
}

resource "aws_s3_bucket" "commands" {
  count = var.enable_commands_store ? 1 : 0

  bucket = "${var.name}-commands-store"
  tags   = local.common_tags
}

resource "aws_s3_bucket_versioning" "commands" {
  count = var.enable_commands_store ? 1 : 0

  bucket = aws_s3_bucket.commands[0].id

  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "commands" {
  count = var.enable_commands_store ? 1 : 0

  bucket = aws_s3_bucket.commands[0].id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_public_access_block" "commands" {
  count = var.enable_commands_store ? 1 : 0

  bucket = aws_s3_bucket.commands[0].id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_lifecycle_configuration" "commands" {
  count = var.enable_commands_store ? 1 : 0

  bucket = aws_s3_bucket.commands[0].id

  rule {
    id     = "expire-old-commands"
    status = "Enabled"

    expiration {
      days = 90
    }

    noncurrent_version_expiration {
      noncurrent_days = 30
    }
  }
}
