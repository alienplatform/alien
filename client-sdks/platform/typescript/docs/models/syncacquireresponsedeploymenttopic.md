# SyncAcquireResponseDeploymentTopic

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTopic } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTopic = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseDeploymentTopicSecretRef](../models/syncacquireresponsedeploymenttopicsecretref.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |