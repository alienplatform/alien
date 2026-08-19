# TargetDeploymentQueueUrl

## Example Usage

```typescript
import { TargetDeploymentQueueUrl } from "@alienplatform/platform-api/models";

let value: TargetDeploymentQueueUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.TargetDeploymentQueueUrlSecretRef](../models/targetdeploymentqueueurlsecretref.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |