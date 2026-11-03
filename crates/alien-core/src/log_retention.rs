/// Days that every CloudWatch log group Alien creates keeps its events, whether setup
/// (CloudFormation, Terraform) or a runtime controller creates it. A year covers compliance
/// requirements to retain logs for twelve months. CloudWatch accepts only a fixed list of
/// values, and 365 is one of them.
pub const AWS_LOG_RETENTION_DAYS: i32 = 365;
