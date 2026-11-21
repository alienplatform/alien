# RegistrationMode

How the installation registers. cloud-record-v1: a verified setup record. enrollment-key-v1: Kubernetes Terraform with a one-use, two-hour setup key. Saved when the deployment's first credential is issued and never changed.

## Example Usage

```typescript
import { RegistrationMode } from "@alienplatform/platform-api/models";

let value: RegistrationMode = "enrollment-key-v1";
```

## Values

```typescript
"cloud-record-v1" | "enrollment-key-v1"
```