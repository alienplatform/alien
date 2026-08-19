# DeploymentConfigQueuePath

## Example Usage

```typescript
import { DeploymentConfigQueuePath } from "@alienplatform/platform-api/models";

let value: DeploymentConfigQueuePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigQueuePathSecretRef](../models/deploymentconfigqueuepathsecretref.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |