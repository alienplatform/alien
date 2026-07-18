# SyncAcquireResponseDeploymentDatabase3

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDatabase3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDatabase3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentDatabaseSecretRef3](../models/syncacquireresponsedeploymentdatabasesecretref3.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |