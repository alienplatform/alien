# SyncAcquireResponseDeploymentRuntimeIdentity

Trusted manager endpoint and audience used by a machine to obtain leaf capabilities.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentRuntimeIdentity } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentRuntimeIdentity = {
  audience: "<value>",
  deploymentId: "<id>",
  exchangeUrl: "https://trivial-monster.name/",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `audience`         | *string*           | :heavy_check_mark: | N/A                |
| `deploymentId`     | *string*           | :heavy_check_mark: | N/A                |
| `exchangeUrl`      | *string*           | :heavy_check_mark: | N/A                |