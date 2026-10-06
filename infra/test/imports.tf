# The state bucket was created by hand before its first apply. Remove this
# block once an apply has adopted it.
import {
  to = module.aws.aws_s3_bucket.e2e_terraform_state
  id = module.aws.e2e_terraform_state_bucket
}
