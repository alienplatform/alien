# SyncAcquireResponseDeploymentDataDir2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDataDir2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDataDir2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                          | [models.SyncAcquireResponseDeploymentDataDirSecretRef2](../models/syncacquireresponsedeploymentdatadirsecretref2.md) | :heavy_check_mark:                                                                                                   | Reference to a Kubernetes Secret                                                                                     |