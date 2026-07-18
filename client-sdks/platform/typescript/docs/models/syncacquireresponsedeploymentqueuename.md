# SyncAcquireResponseDeploymentQueueName

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentQueueName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentQueueName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentQueueNameSecretRef](../models/syncacquireresponsedeploymentqueuenamesecretref.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |