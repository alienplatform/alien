# SyncAcquireResponseDeploymentQueuePath

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentQueuePath } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentQueuePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentQueuePathSecretRef](../models/syncacquireresponsedeploymentqueuepathsecretref.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |