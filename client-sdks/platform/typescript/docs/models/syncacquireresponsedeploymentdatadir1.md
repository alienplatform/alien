# SyncAcquireResponseDeploymentDataDir1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDataDir1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDataDir1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                          | [models.SyncAcquireResponseDeploymentDataDirSecretRef1](../models/syncacquireresponsedeploymentdatadirsecretref1.md) | :heavy_check_mark:                                                                                                   | Reference to a Kubernetes Secret                                                                                     |