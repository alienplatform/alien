# AWS Observe Read Role

Creates a read-only IAM role that Alien can assume to discover and observe existing AWS resources.
Apply this in the AWS account you want to observe.

## Terraform

```hcl
module "alien_observe_read_role" {
  source = "github.com/aliendotdev/alien//infra/aws/observe-read-role"

  name                      = "production-observe"
  manager_managing_role_arn = "arn:aws:iam::123456789012:role/alien-manager"
  external_id               = "replace-with-customer-external-id"
}

output "alien_observe_role_arn" {
  value = module.alien_observe_read_role.role_arn
}
```

## CloudFormation

```bash
aws cloudformation deploy \
  --stack-name alien-observe-read-role \
  --template-file cloudformation.yaml \
  --capabilities CAPABILITY_NAMED_IAM \
  --parameter-overrides \
    Name=production-observe \
    ManagerManagingRoleArn=arn:aws:iam::123456789012:role/alien-manager \
    ExternalId=replace-with-customer-external-id
```

`tag:GetResources` is account-wide and must use `Resource = "*"`. Scope this role by installing it
only in accounts that should be observable.
