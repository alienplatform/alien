# SyncAcquireResponseDeploymentDatabase2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDatabase2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDatabase2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentDatabaseSecretRef2](../models/syncacquireresponsedeploymentdatabasesecretref2.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |