# alien-aws-clients

Custom HTTP client for AWS APIs. Makes direct API calls using `reqwest` with AWS SigV4 request signing — not a wrapper around the official AWS SDK.

Services: Lambda, S3, DynamoDB, SQS, ECR, IAM, STS, CloudFormation, CodeBuild, EC2, Secrets Manager, EventBridge, ACM, API Gateway V2, AutoScaling, ELBv2, SSM.

Trait-based API design with `mockall` support for unit testing. WASM-compatible.
