# Test Images

Minimal container images used by **cloud unit tests** (`alien-aws-clients`, `alien-gcp-clients`, etc.) to verify that infra controllers can deploy, invoke, and tear down cloud resources.

These are **NOT** the runtime image. The actual runtime image containing `alien-runtime` is built from `docker/Dockerfile.alien-base`.

| Image | Purpose | Target |
|-------|---------|--------|
| `lambda/` | Simple Node.js Lambda handler | AWS Lambda (arm64) |
| `http-server/` | Simple Node.js HTTP server on port 8080 | GCP Cloud Run, Azure Container Apps (amd64) |
