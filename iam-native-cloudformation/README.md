# Native CloudFormation IAM setup proof

Real AWS E2E target, eu-west-1. Full generated templates: original aa42e2f38
oversized (400 synthetic S3 object grants) and saved feature f60521756 small
control (10 grants). Templates are unchanged; stack names supply q5-c59 prefixes.
No bucket/object writes, network creates, quota changes, or application import.

Supply a private parameter file containing ManagingRoleArn, then run the commands
in the console logs. create-stack uses CAPABILITY_NAMED_IAM and
CAPABILITY_AUTO_EXPAND for AWS::LanguageExtensions, proof tags, and disabled
rollback to capture partial setup. No resources were omitted or targeted away.

Parent create-stack is accepted; the stack later reaches CREATE_FAILED.
ExecutionSaRole fails with IAM409 / ServiceLimitExceeded, maximum policy size
10240 bytes, after two roles and one managed policy exist. The rejected inline
policy is absent (GetRolePolicy NoSuchEntity). These are native CloudFormation
handler events; raw customer CloudFormation execution cannot invoke Alien's
preflight runner or client error mapper.

Current feature 4f621b3d4 rejects the same oversized input during
`alien render --format cloudformation`, exit1 / zero artifact files, without
cloud credentials: 21317 characters versus the per-role limit10240.

The full small stack reaches CREATE_COMPLETE. AWS GetRolePolicy confirms
647 characters and ten resource grants. This proves native setup and generated
registration outputs, not an imported application deployment.

Normal delete-stack for both stacks succeeds. Subsequent describe-stacks returns
"does not exist" and IAM lists confirm zero owned roles/managed policies.
Every non-owned RoleId/PolicyId captured at the parent failure is preserved.
The harness overwrote the initial inventory filenames with the final read;
therefore preservation is compared against the parent-failure snapshot, not
claimed against a pre-create snapshot. Owned cleanup is independently verified.

Console images render real provider events chronologically; account IDs are
redacted. proof-idfree.json contains assertions from private provider receipts.
