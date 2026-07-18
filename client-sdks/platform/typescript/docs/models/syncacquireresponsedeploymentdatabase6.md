# SyncAcquireResponseDeploymentDatabase6

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDatabase6 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDatabase6 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentDatabaseSecretRef6](../models/syncacquireresponsedeploymentdatabasesecretref6.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |