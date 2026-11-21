# TargetDeploymentRuntimeIdentity

Trusted manager endpoint and audience used by a machine to obtain leaf capabilities.

## Example Usage

```typescript
import { TargetDeploymentRuntimeIdentity } from "@alienplatform/platform-api/models";

let value: TargetDeploymentRuntimeIdentity = {
  audience: "<value>",
  deploymentId: "<id>",
  exchangeUrl: "https://male-mantua.org",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `audience`         | *string*           | :heavy_check_mark: | N/A                |
| `deploymentId`     | *string*           | :heavy_check_mark: | N/A                |
| `exchangeUrl`      | *string*           | :heavy_check_mark: | N/A                |