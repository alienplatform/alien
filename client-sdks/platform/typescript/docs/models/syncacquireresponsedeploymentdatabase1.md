# SyncAcquireResponseDeploymentDatabase1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDatabase1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDatabase1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentDatabaseSecretRef1](../models/syncacquireresponsedeploymentdatabasesecretref1.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |