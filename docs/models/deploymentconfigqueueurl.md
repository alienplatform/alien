# DeploymentConfigQueueUrl

## Example Usage

```typescript
import { DeploymentConfigQueueUrl } from "@alienplatform/platform-api/models";

let value: DeploymentConfigQueueUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.DeploymentConfigQueueUrlSecretRef](../models/deploymentconfigqueueurlsecretref.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |