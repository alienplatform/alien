# SyncAcquireResponseDeploymentDataDir3

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDataDir3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDataDir3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                          | [models.SyncAcquireResponseDeploymentDataDirSecretRef3](../models/syncacquireresponsedeploymentdatadirsecretref3.md) | :heavy_check_mark:                                                                                                   | Reference to a Kubernetes Secret                                                                                     |