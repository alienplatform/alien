# PersistImportedDeploymentRequestDeployerSecretStatus

Whether a deployer secret slot holds a usable value. Alien learns this from
metadata only and never reads the value.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestDeployerSecretStatus } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestDeployerSecretStatus = "missing";
```

## Values

```typescript
"present" | "missing" | "invalid"
```