# Test Lambda Image

A minimalistic Lambda function for testing purposes that responds with "Hello from test Lambda!" on any request.

## Push to ECR

```bash
docker build --platform linux/arm64 -t test-lambda .
docker tag test-lambda:latest <account>.dkr.ecr.<region>.amazonaws.com/test-lambda:latest
aws ecr create-repository --repository-name test-lambda --region <region>
aws ecr get-login-password --region <region> | docker login --username AWS --password-stdin <account>.dkr.ecr.<region>.amazonaws.com
docker push <account>.dkr.ecr.<region>.amazonaws.com/test-lambda:latest
```
