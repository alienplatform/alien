# TargetDeploymentQueueName

## Example Usage

```typescript
import { TargetDeploymentQueueName } from "@alienplatform/platform-api/models";

let value: TargetDeploymentQueueName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentQueueNameSecretRef](../models/targetdeploymentqueuenamesecretref.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |