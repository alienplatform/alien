use alien_extension_sdk::prelude::*;

mod postgres_iam_auth;

fn register(ext: &mut Extension) {
    ext.cloudformation_modifier(postgres_iam_auth::CloudFormation);
    ext.terraform_modifier(postgres_iam_auth::Terraform);
    ext.aws_interceptor(postgres_iam_auth::Rds);
    ext.gcp_interceptor(postgres_iam_auth::CloudSql);
    ext.azure_interceptor(postgres_iam_auth::FlexibleServer);
}

alien_extension_sdk::export!("postgres-iam-auth", register);
