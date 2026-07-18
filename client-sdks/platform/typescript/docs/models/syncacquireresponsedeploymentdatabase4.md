# SyncAcquireResponseDeploymentDatabase4

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDatabase4 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDatabase4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentDatabaseSecretRef4](../models/syncacquireresponsedeploymentdatabasesecretref4.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |