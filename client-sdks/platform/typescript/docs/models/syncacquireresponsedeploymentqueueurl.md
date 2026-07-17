# SyncAcquireResponseDeploymentQueueUrl

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentQueueUrl } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentQueueUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                          | [models.SyncAcquireResponseDeploymentQueueUrlSecretRef](../models/syncacquireresponsedeploymentqueueurlsecretref.md) | :heavy_check_mark:                                                                                                   | Reference to a Kubernetes Secret                                                                                     |