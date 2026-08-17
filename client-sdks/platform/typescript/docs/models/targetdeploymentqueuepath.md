# TargetDeploymentQueuePath

## Example Usage

```typescript
import { TargetDeploymentQueuePath } from "@alienplatform/platform-api/models";

let value: TargetDeploymentQueuePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentQueuePathSecretRef](../models/targetdeploymentqueuepathsecretref.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |